/** Tab completion for the : command line. */
import { type ModelIndex, modelCompletions } from "./handoff.js";
import { completeRaceArg } from "./race.js";

/** Every ex command, by the name you'd normally type (short forms still run, they just aren't offered). */
export const EX_COMMANDS = [
	"attach",
	"bNext",
	"bdelete",
	"bnext",
	"bprevious",
	"buffer",
	"buffers",
	"checkhealth",
	"close",
	"colorscheme",
	"cost",
	"dashboard",
	"diff",
	"effort",
	"engine",
	"engines",
	"extensions",
	"help",
	"highlight",
	"history",
	"jumps",
	"kill",
	"ls",
	"marks",
	"merge",
	"model",
	"new",
	"nohlsearch",
	"only",
	"options",
	"pause",
	"permissions",
	"pr",
	"qall",
	"queue",
	"quit",
	"race",
	"resume",
	"review",
	"serve",
	"set",
	"settings",
	"split",
	"templates",
	"tree",
	"trust",
	"usage",
	"vsplit",
	"wincmd",
	"worktree",
	"worktrees",
	"wq",
	"write",
];

const WINCMD_KEYS = ["h", "j", "k", "l", "w", "W", "p", "t", "b", "v", "s", "c", "q", "o", "=", ">", "<", "+", "-", "|", "_", "x", "r"];
const AGENT_COMMANDS = new Set(["b", "buffer", "sp", "split", "vs", "vsp", "vsplit"]);
const THEME_COMMANDS = new Set(["colorscheme", "colo", "theme"]);

export interface CompletionSources {
	agents: string[];
	themes: string[];
	/** what `:race --engines` offers */
	engines?: string[];
	/** what `:race --models` offers: exact model ids, no aliases */
	models?: string[];
	/** what `:model` offers: every ready engine's models, with their names */
	modelIndex?: ModelIndex;
	/** the engine whose models `:model` offers first (the focused agent's) */
	modelOwn?: string;
	/** what `:effort` offers: the levels the focused agent's engine takes */
	efforts?: string[];
}

/** The text before the part being completed, and what that part could be. */
export function completeCommandLine(line: string, sources: CompletionSources): { base: string; items: string[]; hints?: string[] } {
	const m = /^(\s*)(\S*)(\s+)?([\s\S]*)$/.exec(line) ?? ["", "", "", undefined, ""];
	const [, lead, cmd, gap, arg] = m;
	if (gap === undefined) return { base: lead, items: EX_COMMANDS.filter((c) => c.startsWith(cmd)) };
	const base = lead + cmd + gap;
	if (cmd === "race" || cmd === "race!") {
		const r = completeRaceArg(arg, { engines: sources.engines ?? [], models: sources.models ?? [] });
		return r ? { base: base + r.base, items: r.items } : { base, items: [] };
	}
	if (cmd === "model" || cmd === "mo") {
		// exact ids that start with or contain what's typed, each with its name and version
		const found = sources.modelIndex ? modelCompletions(arg, sources.modelOwn, sources.modelIndex) : [];
		return { base, items: found.map((c) => c.id), hints: found.map((c) => c.hint ?? "") };
	}
	if (cmd === "effort" || cmd === "think") return { base, items: (sources.efforts ?? []).filter((e) => e.startsWith(arg.toLowerCase())) };
	const options = AGENT_COMMANDS.has(cmd) ? sources.agents : THEME_COMMANDS.has(cmd) ? sources.themes : cmd === "wincmd" || cmd === "winc" ? WINCMD_KEYS : [];
	const needle = arg.toLowerCase();
	return { base, items: options.filter((o) => (AGENT_COMMANDS.has(cmd) ? o.toLowerCase().includes(needle) : o.toLowerCase().startsWith(needle))) };
}
