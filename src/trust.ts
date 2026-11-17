/**
 * Which projects' .vigent.toml may run commands.
 *
 * A project config can set the theme, templates, keys and worktree layout
 * freely. The parts that run programs (hooks, engine arguments, custom
 * engine commands, Claude Code permission rules, and commands and mappings
 * that can run a shell command, define commands or mappings, write a file
 * they name or start :serve) only count once you trust that file, like
 * direnv's `allow`: `:trust` in vigent, or `vigent trust` in the project. Trust is for the file as it is now; if it
 * changes, vigent asks again.
 *
 * The same `:trust` trusts the repository itself (its absolute path, in the
 * same store), which lets agent CLIs run there (src/workspace.ts). That lasts
 * until `vigent trust --revoke`.
 */
import { createHash } from "node:crypto";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import type { App } from "./app.js";
import { type Config, loadConfig, PROJECT_CONFIG } from "./config.js";
import { type EngineId, quickEngines } from "./engines.js";
import { KEY_MODES } from "./keys.js";
import { parse as parseToml } from "smol-toml";
import { setProbeDir, type Workspace } from "./workspace.js";
import { configDir } from "./theme.js";

function stateFile(): string {
	const xdg = process.env.XDG_STATE_HOME;
	return path.join(xdg && xdg.length > 0 ? xdg : path.join(os.homedir(), ".local", "state"), "vigent", "trusted.json");
}

const digest = (content: string) => createHash("sha256").update(content).digest("hex");

function read(): Record<string, string> {
	try {
		const v = JSON.parse(fs.readFileSync(stateFile(), "utf8"));
		return v && typeof v === "object" ? v : {};
	} catch {
		return {};
	}
}

/** Whether this file, with exactly this content, is trusted. */
export function isTrusted(file: string, content: string): boolean {
	return read()[path.resolve(file)] === digest(content);
}

function write(all: Record<string, string>): void {
	const f = stateFile();
	fs.mkdirSync(path.dirname(f), { recursive: true });
	fs.writeFileSync(f, `${JSON.stringify(all, null, 2)}\n`);
}

/** Trust this file as it is now. */
export function trustFile(file: string, content: string): void {
	const all = read();
	all[path.resolve(file)] = digest(content);
	write(all);
}

/** A trusted repository's entry in the store (a file's is its sha256). */
const REPOSITORY = "repository";

const same = (a: string, b: string) => {
	if (path.resolve(a) === path.resolve(b)) return true;
	try {
		return fs.realpathSync(a) === fs.realpathSync(b);
	} catch {
		return false;
	}
};

/** Your own folders are never a repository to trust: your home folder and your config folders. */
function yours(dir: string): boolean {
	const home = os.homedir();
	const xdg = process.env.XDG_CONFIG_HOME;
	const config = xdg && xdg.length > 0 ? xdg : path.join(home, ".config");
	return [home, config, configDir()].some((d) => same(dir, d));
}

/** Whether agent CLIs may run in this repository (or folder, outside one). */
export function isRepoTrusted(dir: string): boolean {
	return yours(dir) || read()[path.resolve(dir)] === REPOSITORY;
}

/** Trust this repository: its agent CLIs may run, until revoked. Returns whether it wasn't trusted before. */
export function trustRepo(dir: string): boolean {
	if (isRepoTrusted(dir)) return false;
	const all = read();
	all[path.resolve(dir)] = REPOSITORY;
	write(all);
	return true;
}

/** Forget a repository's trust, and its .vigent.toml's. Returns whether either was trusted. */
export function revokeRepo(dir: string): boolean {
	const all = read();
	const keys = [path.resolve(dir), path.join(path.resolve(dir), PROJECT_CONFIG)].filter((k) => k in all);
	for (const k of keys) delete all[k];
	if (keys.length) write(all);
	return keys.length > 0;
}

/** Everything trusted: repositories, and .vigent.toml files (as they were when trusted). */
export function trustedEntries(): { path: string; kind: "repository" | "file" }[] {
	return Object.entries(read())
		.map(([p, v]) => ({ path: p, kind: v === REPOSITORY ? ("repository" as const) : ("file" as const) }))
		.sort((a, b) => (a.path < b.path ? -1 : a.path > b.path ? 1 : 0));
}

/** Where you stand when vigent starts in `cwd`: the repository (or this folder, outside one), and whether it's trusted. */
export function workspaceAt(cwd: string, root: string | undefined): Workspace {
	const dir = path.resolve(root ?? cwd);
	return { dir, repo: Boolean(root), trusted: isRepoTrusted(dir) };
}

export interface TrustSyncContext {
	app: App;
	config: Config;
	projectFile?: string;
	reloadPermissions?: () => void;
}

/**
 * Execute trust approval for a project's .vigent.toml:
 * 1. Validates that the file exists and is readable/valid.
 * 2. Persists trust via trustFile().
 * 3. Reloads and merges trusted configuration into config.
 * 4. Synchronizes all runtime components: hooks, layout, styles, commands,
 *    engineOptions, permissions, new-agent model/effort defaults, engine discovery,
 *    and the mappings that waited for :trust.
 */
export function executeProjectTrust(ctx: TrustSyncContext): string {
	const { app, config, projectFile, reloadPermissions } = ctx;
	if (!projectFile || !fs.existsSync(projectFile)) {
		return `no ${PROJECT_CONFIG} in this project`;
	}

	let content: string;
	try {
		content = fs.readFileSync(projectFile, "utf8");
	} catch (err) {
		return `can't read ${PROJECT_CONFIG}: ${(err as Error).message}`;
	}

	// Only a file that doesn't parse is refused. Other warnings, like an unknown key, start with
	// the same `.vigent.toml:` prefix but never stopped a file from being trusted.
	try {
		parseToml(content);
	} catch (err) {
		return `can't trust ${PROJECT_CONFIG}: ${(err as Error).message.split("\n")[0]}`;
	}
	// what waits, read over a copy of the config in use: a mapping or command that waits only
	// because it calls one of your commands is named too
	const { gated } = loadConfig(projectFile, { base: structuredClone(config), trusted: false, label: PROJECT_CONFIG });

	trustFile(projectFile, content);
	const keysBefore = Object.fromEntries(KEY_MODES.map((m) => [m, { ...config.keys[m] }]));
	loadConfig(projectFile, { base: config, trusted: true, label: PROJECT_CONFIG });

	// the mappings that waited for :trust work now
	for (const mode of KEY_MODES) {
		for (const [lhs, rhs] of Object.entries(config.keys[mode] ?? {})) {
			if (JSON.stringify(rhs) === JSON.stringify(keysBefore[mode][lhs])) continue;
			try {
				app.keymap.set(mode, lhs, rhs);
			} catch {
				// a bad mapping: vigent warned about it at startup
			}
		}
	}

	app.hooks = config.hooks;
	app.layout = config.layout;
	app.styleConfig = config.style;
	app.commands = config.commands;
	app.commandInfo = config.commandInfo;
	app.engineOptions = config.engines;

	if (reloadPermissions) {
		reloadPermissions();
	}

	// Apply model and effort defaults for engines that don't have an explicit user selection
	for (const [id, opts] of Object.entries(config.engines)) {
		if (opts?.model && !app.newAgentModel[id as EngineId]) {
			app.newAgentModel[id as EngineId] = opts.model;
		}
		if (opts?.effort && !app.newAgentEffort[id as EngineId]) {
			app.newAgentEffort[id as EngineId] = opts.effort;
		}
	}

	// Refresh available engines so newly trusted custom engines appear immediately
	app.setEngines(quickEngines());
	app.detectEngines?.()
		.then((engines) => app.setEngines(engines))
		.catch(() => {});

	return `trusted ${PROJECT_CONFIG}${gated.length ? `: ${gated.join(", ")} now run` : ""}`;
}

/**
 * `:trust`: trust the repository, so its agent CLIs may run (agents that waited
 * for it start), and its .vigent.toml as it is now, if it has one. With
 * nothing left to trust, it says the repository is trusted.
 */
export function executeTrust(ctx: TrustSyncContext & { workspace: Workspace }): string {
	const { app, workspace, projectFile } = ctx;
	const kind = workspace.repo ? "repository" : "folder";
	let newly: boolean;
	try {
		newly = trustRepo(workspace.dir);
	} catch (err) {
		return `can't trust this ${kind}: ${(err as Error).message}`;
	}
	workspace.trusted = true;
	// the engines' own commands may run here now
	setProbeDir(undefined);
	const file = projectFile && fs.existsSync(projectFile) ? executeProjectTrust(ctx) : undefined;
	// the agents that waited go once the file's arguments and hooks are in place
	app.releaseTrustHolds();
	if (!newly) return file ?? `this ${kind} is trusted`;
	return file ? `trusted this ${kind} · ${file}` : `trusted this ${kind}`;
}
