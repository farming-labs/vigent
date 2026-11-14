/**
 * What the registry lists: the entries in the vigent repo's registry/ folder,
 * from the index `npm run registry` writes (public/registry/index.json, read
 * here at build time, and by `vigent add`), then the built-in themes.
 */
import index from "../../public/registry/index.json";
import { type ThemeColors, THEMES } from "./themes";

export type Kind = "theme" | "template" | "keymap" | "engine" | "setup";

export const KINDS: { id: Kind; label: string; plural: string; what: string }[] = [
  { id: "theme", label: "theme", plural: "themes", what: "colors, plus layout and icons through [style]" },
  { id: "template", label: "template", plural: "templates", what: "prompts you start agents with: :new -t <name>" },
  { id: "keymap", label: "keymap", plural: "keymaps", what: "key bindings, in vim's notation" },
  { id: "engine", label: "engine", plural: "engines", what: "another agent CLI, in a few lines of config" },
  { id: "setup", label: "setup", plural: "setups", what: "a .vigent.toml for a stack: hooks, commands, permissions" },
];

export interface Run {
  /** where in the file it comes from: hooks.before_merge, engines.claude.allow ... */
  from: string;
  command?: string;
  /** a Claude Code permission rule it allows without asking */
  allow?: string;
}

export interface Item {
  name: string;
  kind: Kind;
  description: string;
  author: string;
  /** shipped with vigent: nothing to add */
  builtin?: boolean;
  tags: string[];
  /** the file it adds, exactly as vigent add shows it */
  file?: string;
  /** every command it runs and Claude Code rule it allows, read from the file; empty means data only */
  runs?: Run[];
  /** colors for the preview (themes) */
  colors?: ThemeColors;
}

const LAYOUTS: Record<string, string> = {
  nebula: "a lualine-style statusline, the agent in a pill, Nerd Font icons",
  neon: "a card per agent, your prompts in chat bubbles",
  zen: "one quiet row per agent, labeled messages",
  lazyvim: "LazyVim's look, drawn on your terminal's background",
  default: "Neovim's own colors",
};

const builtins: Item[] = Object.entries(THEMES).map(([name, t]) => ({
  name,
  kind: "theme",
  description: LAYOUTS[name] ?? `${name.replace(/-/g, " ")}, as its Neovim theme draws it`,
  author: "vigent",
  builtin: true,
  tags: [t.background, ...(t.transparent ? ["transparent"] : []), ...(name in LAYOUTS && !["default", "lazyvim"].includes(name) ? ["layout"] : [])],
  colors: t,
}));

/** registry/<kind>/<name>/ in the vigent repo, checked and written by `npm run registry` */
const community: Item[] = (index as { entries: Item[] }).entries.map(({ name, kind, description, author, tags, file, runs, colors }) => ({ name, kind, description, author, tags, file, runs, colors }));

/** community entries first, then the built-in themes */
export const ITEMS: Item[] = [...community, ...builtins];
