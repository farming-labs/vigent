/**
 * What the registry lists. A prototype: the built-in themes are real, every entry
 * marked `sample` is made up to show what the community side would look like.
 */
import { type ThemeColors, THEMES } from "./themes";

export type Kind = "theme" | "template" | "keymap" | "engine" | "setup";

export const KINDS: { id: Kind; label: string; plural: string; what: string }[] = [
  { id: "theme", label: "theme", plural: "themes", what: "colors, plus layout and icons through [style]" },
  { id: "template", label: "template", plural: "templates", what: "prompts you start agents with: :new -t <name>" },
  { id: "keymap", label: "keymap", plural: "keymaps", what: "key bindings, in vim's notation" },
  { id: "engine", label: "engine", plural: "engines", what: "another agent CLI, in a few lines of config" },
  { id: "setup", label: "setup", plural: "setups", what: "a .vigent.toml for a stack: hooks, commands, permissions" },
];

export interface Item {
  name: string;
  kind: Kind;
  description: string;
  author: string;
  /** made up for the prototype */
  sample?: boolean;
  /** shipped with vigent: nothing to add */
  builtin?: boolean;
  tags: string[];
  /** the file it adds, as it would be read before adding */
  file?: string;
  /** shell commands it can run; empty or missing means data only */
  runs?: string[];
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

const community: Item[] = [
  {
    name: "harbor",
    kind: "theme",
    description: "nord with a card per agent and a brighter mode badge",
    author: "sample",
    sample: true,
    tags: ["dark", "layout"],
    colors: { ...THEMES.nord, modeBg: "#88c0d0", user: "#88c0d0" },
    file: `extends = "nord"\n\n[highlights]\nVigentModeNormal = { fg = "#2e3440", bg = "#88c0d0" }\n\n[style]\nsidebar = "cards"`,
  },
  {
    name: "ember",
    kind: "theme",
    description: "gruvbox with orange prompts and labeled messages",
    author: "sample",
    sample: true,
    tags: ["dark", "layout"],
    colors: { ...THEMES.gruvbox, user: "#fe8019", modeBg: "#fe8019" },
    file: `extends = "gruvbox"\n\n[highlights]\nVigentUser = { fg = "#fe8019", bold = true }\n\n[style]\nmessages = "labeled"`,
  },
  {
    name: "fix-ci",
    kind: "template",
    description: "find why CI failed, fix it, and rerun the same check locally",
    author: "sample",
    sample: true,
    tags: ["ci", "gh"],
    file: `[templates.fix-ci]\ndescription = "fix the failing CI run"\nprompt = """Read the latest failing CI run with\n\`gh run view --log-failed\`, find the cause, fix it,\nand run the same check locally before you stop. {task}"""`,
  },
  {
    name: "test-the-diff",
    kind: "template",
    description: "tests for what this branch changed, each one failing without the change",
    author: "sample",
    sample: true,
    tags: ["tests"],
    file: `[templates.test-the-diff]\ndescription = "tests for this branch's changes"\nprompt = """Write tests for what changed on this branch.\nEach test must fail without the change.\nDon't edit the code under test. {task}"""`,
  },
  {
    name: "race-review",
    kind: "template",
    description: "the same review from three models; keep the one you agree with",
    author: "sample",
    sample: true,
    tags: ["review", "race"],
    file: `[templates.race-review]\ndescription = "review, without changing files"\nprompt = """Review this branch for bugs and missing cases.\nList each finding with file:line. Change nothing. {task}"""\neffort = "high"`,
  },
  {
    name: "leader-land",
    kind: "keymap",
    description: "start, review and land agents from the leader key",
    author: "sample",
    sample: true,
    tags: ["leader"],
    file: `[keys.normal]\n"<leader>nn" = ":new "\n"<leader>dd" = ":diff<CR>"\n"<leader>mm" = ":merge<CR>"\n"<leader>pp" = ":pr<CR>"\n"<Tab>" = "session.next"\n"<S-Tab>" = "session.prev"`,
  },
  {
    name: "aider",
    kind: "engine",
    description: "Aider as an engine, with :model choosing its model",
    author: "sample",
    sample: true,
    tags: ["text output"],
    file: `[engines.aider]\nname = "Aider"\ncommand = ["aider", "--yes-always", "--message", "{prompt}"]\nmodel_args = ["--model", "{model}"]\nmodels = ["sonnet", "gpt-5"]`,
    runs: ["aider --yes-always --message {prompt}"],
  },
  {
    name: "amp",
    kind: "engine",
    description: "Amp, read through its Claude Code-style JSON stream",
    author: "sample",
    sample: true,
    tags: ["stream-json"],
    file: `[engines.amp]\nname = "Amp"\ncommand = ["amp", "--execute", "{prompt}", "--stream-json"]\noutput = "claude"`,
    runs: ["amp --execute {prompt} --stream-json"],
  },
  {
    name: "goose",
    kind: "engine",
    description: "Goose's headless run mode",
    author: "sample",
    sample: true,
    tags: ["text output"],
    file: `[engines.goose]\nname = "Goose"\ncommand = ["goose", "run", "-t", "{prompt}"]`,
    runs: ["goose run -t {prompt}"],
  },
  {
    name: "pnpm-monorepo",
    kind: "setup",
    description: "installs each new worktree, tests before merging, lets Claude Code run pnpm test",
    author: "sample",
    sample: true,
    tags: ["pnpm", "node"],
    file: `[hooks]\nworktree_created = "pnpm install --frozen-lockfile"\nbefore_merge = "pnpm test"\n\n[commands]\nTest = "!pnpm test {args}"\n\n[engines.claude]\nallow = ["Bash(pnpm test:*)", "Bash(pnpm lint:*)"]`,
    runs: ["pnpm install --frozen-lockfile", "pnpm test", "allows Claude Code: Bash(pnpm test:*), Bash(pnpm lint:*)"],
  },
  {
    name: "cargo-workspace",
    kind: "setup",
    description: "fetches crates in new worktrees and runs clippy and tests before merging",
    author: "sample",
    sample: true,
    tags: ["rust"],
    file: `[hooks]\nworktree_created = "cargo fetch --locked"\nbefore_merge = "cargo clippy --all-targets -- -D warnings && cargo test --locked"`,
    runs: ["cargo fetch --locked", "cargo clippy --all-targets -- -D warnings && cargo test --locked"],
  },
];

/** community entries first, then the built-in themes */
export const ITEMS: Item[] = [...community, ...builtins];
