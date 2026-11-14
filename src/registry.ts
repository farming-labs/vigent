/**
 * The registry: themes, templates, keymaps, engines and setups people share,
 * from the index the website publishes (scripts/registry.mjs writes it from
 * registry/<kind>/<name>/ in the vigent repo).
 *
 *   vigent add <name>            show the file (and what it runs), ask, then add it
 *   vigent add <kind>/<name>     when two kinds share a name
 *   vigent add --list [kind]     every entry
 *   vigent search <words>        entries whose name, description or tags have them
 *
 * VIGENT_REGISTRY points at another index: a URL, or a local index.json (or
 * the folder it's in), for tests and offline use.
 *
 * Nothing is added before you've seen the file and said yes (--yes skips the
 * question). A theme is a file in your themes folder; a template, keymap or
 * engine goes into config.toml beside what's there; a setup is the
 * repository's .vigent.toml, and what it runs waits for :trust.
 */
import * as fs from "node:fs";
import * as path from "node:path";
import { createInterface } from "node:readline";
import { fileURLToPath } from "node:url";
import { parse as parseToml } from "smol-toml";
import { configPath, PROJECT_CONFIG } from "./config.js";
import { mergeToml, tomlKey, type TomlUnit } from "./config-edit.js";
import { KEY_MODES } from "./keys.js";
import { BUILTIN_THEMES, themesDir } from "./theme.js";
import { repoRoot } from "./worktree.js";

export const REGISTRY_URL = "https://vigent.farming-labs.dev/registry/index.json";
export const KINDS = ["theme", "template", "keymap", "engine", "setup"] as const;
export type Kind = (typeof KINDS)[number];
const PLURAL: Record<Kind, string> = { theme: "themes", template: "templates", keymap: "keymaps", engine: "engines", setup: "setups" };

/** One thing an entry runs: a command, or a rule that lets Claude Code run one without asking. */
export interface RegistryRun {
	/** where in the file it comes from: hooks.before_merge, engines.claude.allow ... */
	from: string;
	command?: string;
	allow?: string;
}

export interface RegistryEntry {
	name: string;
	kind: Kind;
	description: string;
	author: string;
	license?: string;
	tags: string[];
	/** the file it adds, exactly */
	file: string;
	runs: RegistryRun[];
}

export interface RegistryIndex {
	version: number;
	entries: RegistryEntry[];
}

/** Where the index comes from: VIGENT_REGISTRY, else the website. */
export function registrySource(env: NodeJS.ProcessEnv = process.env): string {
	return env.VIGENT_REGISTRY?.trim() || REGISTRY_URL;
}

/** Read the index from a URL or a local file (or the folder holding index.json). */
export async function loadIndex(source: string): Promise<RegistryIndex> {
	let text: string;
	if (/^https?:\/\//i.test(source)) {
		let res: Response;
		try {
			res = await fetch(source, { signal: AbortSignal.timeout(30000) });
		} catch (err) {
			const e = err as Error & { cause?: { message?: string } };
			throw new Error(`couldn't fetch the registry from ${source}: ${e.cause?.message ?? e.message}`);
		}
		if (!res.ok) throw new Error(`couldn't fetch the registry from ${source}: HTTP ${res.status}`);
		text = await res.text();
	} else {
		let file = source.startsWith("file://") ? fileURLToPath(source) : path.resolve(source);
		try {
			if (fs.statSync(file).isDirectory()) file = path.join(file, "index.json");
			text = fs.readFileSync(file, "utf8");
		} catch (err) {
			throw new Error(`couldn't read the registry at ${file}: ${(err as NodeJS.ErrnoException).code === "ENOENT" ? "no such file" : (err as Error).message}`);
		}
	}
	return parseIndex(text, source);
}

/** The index's JSON, checked: entries it can't use are left out. */
export function parseIndex(text: string, source: string): RegistryIndex {
	let raw: unknown;
	try {
		raw = JSON.parse(text);
	} catch {
		throw new Error(`the registry at ${source} isn't an index vigent can read`);
	}
	const r = raw as Partial<RegistryIndex>;
	if (!r || typeof r !== "object" || typeof r.version !== "number" || !Array.isArray(r.entries)) throw new Error(`the registry at ${source} isn't an index vigent can read`);
	if (r.version > 1) throw new Error(`the registry at ${source} is newer than this vigent (format ${r.version}): update vigent`);
	const str = (v: unknown) => typeof v === "string";
	const entries = r.entries.filter(
		(e): e is RegistryEntry =>
			Boolean(e) &&
			str(e.name) &&
			(KINDS as readonly string[]).includes(e.kind) &&
			str(e.description) &&
			str(e.author) &&
			str(e.file) &&
			Array.isArray(e.tags) &&
			Array.isArray(e.runs) &&
			e.runs.every((x) => x && str(x.from) && (str(x.command) || str(x.allow))),
	);
	return { version: r.version, entries };
}

function distance(a: string, b: string): number {
	const dp = Array.from({ length: b.length + 1 }, (_, i) => i);
	for (let i = 1; i <= a.length; i++) {
		let prev = dp[0];
		dp[0] = i;
		for (let j = 1; j <= b.length; j++) {
			const tmp = dp[j];
			dp[j] = Math.min(dp[j] + 1, dp[j - 1] + 1, prev + (a[i - 1] === b[j - 1] ? 0 : 1));
			prev = tmp;
		}
	}
	return dp[b.length];
}

/** Up to three names close to the one asked for: a few edits away, or one inside the other. */
export function closeNames(query: string, names: string[]): string[] {
	const q = query.toLowerCase();
	const limit = Math.max(2, Math.floor(q.length / 3));
	return [...new Set(names)]
		.map((n) => ({ n, d: distance(q, n.toLowerCase()) }))
		.filter(({ n, d }) => d <= limit || (q.length >= 3 && n.includes(q)) || (n.length >= 3 && q.includes(n)))
		.sort((a, b) => a.d - b.d || (a.n < b.n ? -1 : a.n > b.n ? 1 : 0))
		.slice(0, 3)
		.map(({ n }) => n);
}

/** `kind/name` or `name`, and whether the kind is one. */
export function parseQuery(query: string): { kind?: Kind; name: string; error?: string } {
	const slash = query.indexOf("/");
	if (slash < 0) return { name: query };
	const kind = query.slice(0, slash);
	const name = query.slice(slash + 1);
	const k = kindOf(kind);
	if (!k) return { name, error: `unknown kind '${kind}' (${KINDS.join(", ")})` };
	return { kind: k, name };
}

/** theme or themes → theme */
function kindOf(word: string): Kind | undefined {
	return KINDS.find((k) => k === word || PLURAL[k] === word);
}

/** A built-in theme needs nothing added. */
export function builtinThemeMessage(query: string): string | undefined {
	const q = parseQuery(query);
	if (q.error || (q.kind && q.kind !== "theme") || !BUILTIN_THEMES[q.name]) return undefined;
	return `'${q.name}' is a built-in theme: nothing to add, use :colorscheme ${q.name}`;
}

/** The entry a query names, or why there isn't one. */
export function findEntry(index: RegistryIndex, query: string): { entry: RegistryEntry } | { error: string } {
	const q = parseQuery(query);
	if (q.error) return { error: q.error };
	const found = index.entries.filter((e) => e.name === q.name && (!q.kind || e.kind === q.kind));
	if (found.length === 1) return { entry: found[0] };
	if (found.length > 1) {
		const kinds = found.map((e) => e.kind);
		return { error: `'${q.name}' is a ${kinds.slice(0, -1).join(", a ")} and a ${kinds[kinds.length - 1]}: ${found.map((e) => `vigent add ${e.kind}/${e.name}`).join(" or ")}` };
	}
	const close = closeNames(q.name, index.entries.filter((e) => !q.kind || e.kind === q.kind).map((e) => e.name));
	return { error: `no ${q.kind ? `${q.kind} ` : ""}'${q.name}' in the registry ${close.length ? `(did you mean ${close.join(", ")}?)` : "(vigent add --list shows them all)"}` };
}

/** What an entry owns in config.toml: [templates.x], [engines.x], each [keys.<mode>] mapping. */
export function entryUnits(raw: Record<string, unknown>): TomlUnit[] {
	const isTable = (v: unknown): v is Record<string, unknown> => Boolean(v) && typeof v === "object" && !Array.isArray(v);
	const units: TomlUnit[] = [];
	for (const [top, value] of Object.entries(raw)) {
		if (!isTable(value)) {
			units.push({ name: tomlKey(top), paths: [[top]] });
			continue;
		}
		for (const [k, v] of Object.entries(value)) {
			if (top === "keys" && (KEY_MODES as string[]).includes(k) && isTable(v)) {
				// keys right under [keys] are normal-mode mappings
				for (const lhs of Object.keys(v)) units.push({ name: `[keys.${k}] ${tomlKey(lhs)}`, paths: [["keys", k, lhs], ...(k === "normal" ? [["keys", lhs]] : [])] });
			} else if (top === "keys") units.push({ name: `[keys] ${tomlKey(k)}`, paths: [["keys", k], ["keys", "normal", k]] });
			else units.push({ name: `[${tomlKey(top)}.${tomlKey(k)}]`, paths: [[top, k]] });
		}
	}
	return units;
}

/** Where an entry goes, and the text that goes there; or why it can't. */
export interface AddPlan {
	file: string;
	text: string;
	/** "It writes" or "It adds to" */
	verb: string;
	replaced: string[];
	done: string[];
}

export function planAdd(entry: RegistryEntry, { force = false, cwd = process.cwd() }: { force?: boolean; cwd?: string } = {}): AddPlan | { error: string } {
	const exists = (file: string) => `${file} already exists (--force replaces it)`;
	if (entry.kind === "theme") {
		const file = path.join(themesDir(), `${entry.name}.toml`);
		if (fs.existsSync(file) && !force) return { error: exists(file) };
		return { file, text: entry.file, verb: "It writes", replaced: fs.existsSync(file) ? [file] : [], done: [`added ${file}`, `use it with :colorscheme ${entry.name} (or theme = "${entry.name}" in config.toml)`] };
	}
	if (entry.kind === "setup") {
		const root = repoRoot(cwd);
		if (!root) return { error: `not inside a git repository: a setup is the ${PROJECT_CONFIG} at a repository's root` };
		const file = path.join(root, PROJECT_CONFIG);
		if (fs.existsSync(file) && !force) return { error: exists(file) };
		return {
			file,
			text: entry.file,
			verb: "It writes",
			replaced: fs.existsSync(file) ? [file] : [],
			done: [`added ${file}`, "nothing in it runs until you trust it: :trust in vigent, or vigent trust"],
		};
	}
	const file = configPath();
	let text = "";
	try {
		text = fs.existsSync(file) ? fs.readFileSync(file, "utf8") : "";
	} catch (err) {
		return { error: `couldn't read ${file}: ${(err as Error).message}` };
	}
	let raw: Record<string, unknown>;
	try {
		raw = parseToml(entry.file) as Record<string, unknown>;
	} catch (err) {
		return { error: `the entry's file doesn't parse: ${(err as Error).message}` };
	}
	const r = mergeToml(text, entry.file, entryUnits(raw), force);
	if ("conflicts" in r) return { error: `${file} already has ${r.conflicts.join(", ")} (--force replaces ${r.conflicts.length === 1 ? "it" : "them"})` };
	if ("error" in r) return { error: `couldn't add it to ${file} (${r.error}); add it by hand, or fix the file first (vigent config check)` };
	const units = entryUnits(raw).map((u) => u.name);
	const done = [`added ${entry.kind === "keymap" ? `${units.length} mapping${units.length === 1 ? "" : "s"}` : units.join(", ")} to ${file}`];
	if (entry.kind === "template") done.push(`use it with :new -t ${entry.name} <task> (vigent reads it when it starts)`);
	else if (entry.kind === "engine") done.push(`use it with :new <task> --engine ${entry.name}, or engine = "${entry.name}" in config.toml (vigent reads it when it starts)`);
	else done.push("vigent reads them when it starts");
	return { file, text: r.text, verb: "It adds to", replaced: r.replaced, done };
}

/** What `vigent add` shows before it asks: the entry, the file, and everything it runs. */
export function describe(entry: RegistryEntry, plan: AddPlan): string {
	const out = [`${entry.kind}/${entry.name} by ${entry.author}${entry.license ? ` (${entry.license})` : ""}`, entry.description, "", `${plan.verb} ${plan.file}:`, ""];
	for (const l of entry.file.replace(/\n$/, "").split("\n")) out.push(l.trim() ? `    ${l}` : "");
	out.push("");
	const commands = entry.runs.filter((r) => r.command);
	const rules = entry.runs.filter((r) => r.allow);
	if (commands.length) {
		out.push("It runs:");
		for (const r of commands) out.push(`    $ ${r.command}    # ${r.from}`);
	}
	if (rules.length) {
		out.push("It lets Claude Code run, without asking:");
		for (const r of rules) out.push(`    ${r.allow}    # ${r.from}`);
	}
	if (commands.length || rules.length) out.push("");
	if (plan.replaced.length) out.push(`It replaces ${plan.replaced.join(", ")}`, "");
	return out.join("\n");
}

/** Ask on the terminal; no answer (the end of the input) is no. */
function ask(question: string): Promise<boolean> {
	return new Promise((resolve) => {
		const rl = createInterface({ input: process.stdin, output: process.stdout, terminal: false });
		let answered = false;
		process.stdout.write(question);
		rl.once("line", (line) => {
			answered = true;
			// a terminal echoed the answer and its Enter; piped in, it wasn't
			if (!process.stdin.isTTY) process.stdout.write("\n");
			rl.close();
			resolve(/^y(es)?$/i.test(line.trim()));
		});
		rl.once("close", () => {
			if (!answered) {
				process.stdout.write("\n");
				resolve(false);
			}
		});
	});
}

const ADD_USAGE = "usage: vigent add <name> | <kind>/<name> [--yes] [--force]   vigent add --list [kind]";

/** `vigent add`. */
export async function addCommand(argv: string[]): Promise<number> {
	const list = argv.includes("--list") || argv.includes("-l");
	const yes = argv.includes("--yes") || argv.includes("-y");
	const force = argv.includes("--force");
	const unknown = argv.find((a) => a.startsWith("-") && !["--list", "-l", "--yes", "-y", "--force"].includes(a));
	const words = argv.filter((a) => !a.startsWith("-"));
	if (unknown) {
		console.error(`vigent: unknown option '${unknown}'\n${ADD_USAGE}`);
		return 1;
	}
	if (list) {
		const kind = words[0] === undefined ? undefined : kindOf(words[0]);
		if (words[0] !== undefined && !kind) {
			console.error(`vigent: unknown kind '${words[0]}' (${KINDS.join(", ")})`);
			return 1;
		}
		const index = await loadOrSay();
		if (!index) return 1;
		const width = Math.max(0, ...index.entries.map((e) => e.name.length)) + 2;
		for (const k of KINDS) {
			const shown = index.entries.filter((e) => e.kind === k);
			if ((kind && k !== kind) || !shown.length) continue;
			console.log(PLURAL[k]);
			for (const e of shown) console.log(`  ${e.name.padEnd(width)}${e.description}`);
		}
		return 0;
	}
	if (words.length !== 1) {
		console.error(ADD_USAGE);
		return 1;
	}
	const query = words[0];
	const builtin = builtinThemeMessage(query);
	if (builtin) {
		console.error(`vigent: ${builtin}`);
		return 1;
	}
	const bad = parseQuery(query).error;
	if (bad) {
		console.error(`vigent: ${bad}`);
		return 1;
	}
	const index = await loadOrSay();
	if (!index) return 1;
	const found = findEntry(index, query);
	if ("error" in found) {
		console.error(`vigent: ${found.error}`);
		return 1;
	}
	const plan = planAdd(found.entry, { force });
	if ("error" in plan) {
		console.error(`vigent: ${plan.error}`);
		return 1;
	}
	process.stdout.write(`${describe(found.entry, plan)}\n`);
	if (!yes && !(await ask("Add it? [y/N] "))) {
		console.log("nothing added");
		return 1;
	}
	try {
		fs.mkdirSync(path.dirname(plan.file), { recursive: true });
		fs.writeFileSync(plan.file, plan.text);
	} catch (err) {
		console.error(`vigent: couldn't write ${plan.file}: ${(err as Error).message}`);
		return 1;
	}
	for (const l of plan.done) console.log(l);
	return 0;
}

/** `vigent search <words>`: entries with every word in their kind, name, description, tags or author. */
export async function searchCommand(argv: string[]): Promise<number> {
	const words = argv.filter((a) => !a.startsWith("-")).map((w) => w.toLowerCase());
	if (!words.length) {
		console.error("usage: vigent search <words>");
		return 1;
	}
	const index = await loadOrSay();
	if (!index) return 1;
	const found = index.entries.filter((e) => {
		const text = `${e.kind} ${e.name} ${e.description} ${e.tags.join(" ")} ${e.author}`.toLowerCase();
		return words.every((w) => text.includes(w));
	});
	if (!found.length) {
		console.error(`vigent: nothing in the registry matches '${words.join(" ")}' (vigent add --list shows them all)`);
		return 1;
	}
	const width = Math.max(...found.map((e) => e.kind.length + 1 + e.name.length)) + 2;
	for (const e of found) console.log(`  ${`${e.kind}/${e.name}`.padEnd(width)}${e.description}`);
	return 0;
}

async function loadOrSay(): Promise<RegistryIndex | undefined> {
	try {
		return await loadIndex(registrySource());
	} catch (err) {
		console.error(`vigent: ${(err as Error).message}`);
		return undefined;
	}
}
