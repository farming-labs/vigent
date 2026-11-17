/**
 * The repository agents run in, and whether you've trusted it.
 *
 * vigent runs each agent CLI headless, and headless CLIs skip their own
 * "trust this folder?" question: Claude Code reads the repository's
 * .claude/settings.json (hooks, env, permissions, MCP servers), pi loads
 * .pi/extensions, and the others read their own project files. So vigent
 * starts no agent CLI in a repository you haven't trusted (`:trust`, or
 * `vigent trust`), and runs the engine commands of its own (sign-in checks,
 * model lists) outside it until then. Outside a git repository, the folder
 * vigent was started in counts as the repository, as agents run there.
 */
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

/** Where you stand: the folder agents run in, whether it's a git repository, and whether it's trusted. */
export interface Workspace {
	dir: string;
	repo: boolean;
	trusted: boolean;
}

/** The project files of agent CLIs that run code or change what an agent may do, as the message names them. */
export const AGENT_CONFIGS = [".claude/", ".mcp.json", ".pi/", "opencode.json", "opencode.jsonc", ".opencode/", ".gemini/", ".cursor/", ".codex/"];

/** Which of them this folder has. */
export function agentConfigs(dir: string): string[] {
	return AGENT_CONFIGS.filter((name) => {
		try {
			const st = fs.statSync(path.join(dir, name.replace(/\/$/, "")));
			return name.endsWith("/") ? st.isDirectory() : st.isFile();
		} catch {
			return false;
		}
	});
}

/** What starting an agent says while the repository isn't trusted. */
export function untrustedMessage(w: Pick<Workspace, "dir" | "repo">): string {
	const found = agentConfigs(w.dir);
	return `this ${w.repo ? "repository" : "folder"} isn't trusted yet: its agent CLIs would load its own settings (${found.length ? found.join(", ") : ".claude/, .pi/, …"}). :trust to allow it`;
}

/** The start screen's line while the repository isn't trusted. */
export function untrustedNote(w: Pick<Workspace, "repo">): string {
	return `this ${w.repo ? "repository" : "folder"} isn't trusted yet: :trust lets its agent CLIs run here`;
}

/** What a reopened agent waits for, shown in the sidebar instead of "paused". */
export const TRUST_HOLD = "waiting for :trust";

/** A folder no repository's settings come from: your home folder, else the temp folder. */
export function neutralDir(): string {
	const home = os.homedir();
	try {
		if (home && fs.statSync(home).isDirectory()) return home;
	} catch {
		// no home folder
	}
	return os.tmpdir();
}

let probeDir: string | undefined;

/**
 * Where vigent runs engine commands of its own (`claude auth status`,
 * `pi --list-models`, `opencode models`, ...): undefined is the current
 * folder, as before; while the repository isn't trusted, a neutral folder,
 * so none of them loads the repository's settings.
 */
export function setProbeDir(dir: string | undefined): void {
	probeDir = dir;
}

export function probeCwd(): string | undefined {
	return probeDir;
}
