#!/usr/bin/env node
/**
 * The registry: check every entry in registry/<kind>/<name>/ with vigent's own
 * loaders, then write website/public/registry/index.json, the index
 * `vigent add` reads and the website lists.
 *
 * usage:
 *   node scripts/registry.mjs           check every entry and write the index (npm run registry)
 *   node scripts/registry.mjs --check   check every entry, and fail if the index is out of date (npm run registry:check)
 *
 * An entry is two files:
 *   entry.toml   the file people add: a theme file, [templates.<name>], [keys.*],
 *                [engines.<name>] or a whole .vigent.toml
 *   meta.toml    description, author, license, tags
 *
 * It's rejected for an unknown kind, a bad name, missing or unknown meta, a
 * file vigent can't load, or any warning vigent would show for it; every
 * problem is listed. What an entry runs (its commands and Claude Code allow
 * rules) is read from the file, the way vigent's trust check reads it, so the
 * index can't say less than the file does.
 *
 * Needs the built TypeScript (npm run build): it uses dist/.
 */
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { parse as parseToml } from "smol-toml";

const root = fileURLToPath(new URL("..", import.meta.url));
export const REGISTRY_DIR = path.join(root, "registry");
export const INDEX_FILE = path.join(root, "website", "public", "registry", "index.json");

// themes extend built-ins only: nobody's own ~/.config/vigent may change what loads here
process.env.XDG_CONFIG_HOME = fs.mkdtempSync(path.join(os.tmpdir(), "vigent-registry-"));
process.on("exit", () => fs.rmSync(process.env.XDG_CONFIG_HOME, { recursive: true, force: true }));

const dist = (f) => import(pathToFileURL(path.join(root, "dist", f)).href);
const { loadConfig } = await dist("config.js");
const { buildKeymap } = await dist("keys.js");
const { BUILTIN_THEMES, buildTheme, normalizeGroups, Painter } = await dist("theme.js");
const { parseStyle } = await dist("style.js");
const { CUSTOM_ENGINES, ENGINE_BINS, ENGINE_IDS } = await dist("engines.js");
const { normalizeStep } = await dist("commands.js");

export const KINDS = ["theme", "template", "keymap", "engine", "setup"];
/** lowercase words joined by -, as in a URL */
export const NAME = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;
const META = { description: "one line on what it does", author: "who made it", license: "an SPDX id, e.g. MIT", tags: 'a list, e.g. ["dark", "layout"]' };
const THEME_KEYS = ["name", "background", "extends", "transparent", "palette", "groups", "highlights", "style"];
const ATTR_KEYS = { fg: "string", bg: "string", link: "string", bold: "boolean", italic: "boolean", underline: "boolean", reverse: "boolean" };
/** the colors the website's preview paints, and the group each comes from (as website/scripts/gen-themes.mjs) */
const ROLES = {
	bg: ["Normal", "bg"],
	fg: ["Normal", "fg"],
	side: ["VigentSidebar", "bg"],
	sep: ["WinSeparator", "fg"],
	bar: ["StatusLine", "bg"],
	barFg: ["StatusLine", "fg"],
	dim: ["Comment", "fg"],
	title: ["Title", "fg"],
	str: ["String", "fg"],
	user: ["VigentUser", "fg"],
	tool: ["VigentTool", "fg"],
	link: ["Function", "fg"],
	spin: ["VigentSpinner", "fg"],
	modeBg: ["VigentModeNormal", "bg"],
	modeFg: ["VigentModeNormal", "fg"],
	float: ["NormalFloat", "bg"],
	sel: ["PmenuSel", "bg"],
	selFg: ["PmenuSel", "fg"],
};

const isTable = (v) => Boolean(v) && typeof v === "object" && !Array.isArray(v) && !(v instanceof Date);

/** Arguments as you'd type them in a shell: quoted only when they need it. */
export function shellWords(args) {
	return args.map((a) => (/^[\w@%+=:,./{}<>-]+$/.test(a) ? a : `'${a.replace(/'/g, `'\\''`)}'`)).join(" ");
}

/**
 * What a config file runs: every part vigent holds back until a project file is
 * trusted (hooks, commands with a shell step, engine commands and arguments,
 * Claude Code allow rules), with the command or rule each one adds.
 */
export function runsOf(raw, gated) {
	const out = [];
	for (const from of gated) {
		const [top, id, key] = from.split(".");
		if (top === "hooks") out.push({ from, command: String(raw.hooks[id]).trim() });
		else if (top === "commands") {
			for (const step of [raw.commands[id]].flat()) {
				const s = normalizeStep(step);
				if (s.startsWith("!")) out.push({ from, command: s.slice(1).trim() });
			}
		} else if (top === "engines" && key === "command") {
			const e = raw.engines[id];
			const prompt = e.command.slice(1).some((a) => a.includes("{prompt}")) ? [] : ["{prompt}"];
			out.push({ from, command: shellWords([...e.command, ...prompt]) });
			for (const k of ["model_args", "resume_args"]) {
				if (Array.isArray(e[k])) out.push({ from: `engines.${id}.${k}`, command: shellWords([...e.command, ...e[k], ...prompt]) });
			}
		} else if (top === "engines" && key === "args") out.push({ from, command: shellWords([ENGINE_BINS[id] ?? id, ...raw.engines[id].args]) });
		else if (top === "engines" && key === "allow") for (const rule of raw.engines[id].allow) out.push({ from, allow: rule });
		// a part vigent gates that this script doesn't know yet: still listed, never left out
		else out.push({ from, command: JSON.stringify(from.split(".").reduce((v, k) => v?.[k], raw)) });
	}
	return out;
}

function checkMeta(text, problems) {
	let meta;
	try {
		meta = parseToml(text);
	} catch (err) {
		problems.push(`meta.toml: ${err.message.split("\n")[0]}`);
		return undefined;
	}
	for (const k of Object.keys(meta)) if (!(k in META)) problems.push(`meta.toml: unknown key '${k}' (${Object.keys(META).join(", ")})`);
	for (const [k, what] of Object.entries(META)) {
		const v = meta[k];
		if (v === undefined) problems.push(`meta.toml: '${k}' is missing: ${what}`);
		else if (k === "tags") {
			if (!Array.isArray(v) || v.some((t) => typeof t !== "string" || !/^[a-z0-9][a-z0-9 .+-]{0,23}$/.test(t))) problems.push("meta.toml: 'tags' should be a list of short lowercase words");
			else if (v.length > 6) problems.push("meta.toml: at most 6 tags");
		} else if (typeof v !== "string" || !v.trim()) problems.push(`meta.toml: '${k}' should be a string: ${what}`);
		else if (k === "description" && (v.length > 120 || v.includes("\n"))) problems.push("meta.toml: 'description' should be one line of at most 120 characters");
	}
	return meta;
}

function checkTheme(name, raw, problems) {
	for (const k of Object.keys(raw)) if (!THEME_KEYS.includes(k)) problems.push(`unknown key '${k}' (a theme file has ${THEME_KEYS.join(", ")})`);
	if (raw.name !== undefined && raw.name !== name) problems.push(`name = ${JSON.stringify(raw.name)} should be "${name}", the entry's name, or left out`);
	if (raw.extends !== undefined && !(typeof raw.extends === "string" && BUILTIN_THEMES[raw.extends])) {
		problems.push(`extends = ${JSON.stringify(raw.extends)}: a registry theme extends a built-in theme (${Object.keys(BUILTIN_THEMES).join(", ")})`);
		return undefined;
	}
	if (raw.background !== undefined && raw.background !== "dark" && raw.background !== "light") problems.push(`background should be "dark" or "light"`);
	if (raw.transparent !== undefined && typeof raw.transparent !== "boolean") problems.push("transparent should be true or false");
	for (const table of ["groups", "highlights"]) {
		if (raw[table] === undefined) continue;
		if (!isTable(raw[table])) {
			problems.push(`[${table}] should be a table of highlight groups`);
			continue;
		}
		for (const [group, v] of Object.entries(raw[table])) {
			const where = `[${table}] ${group}`;
			if (typeof v === "string") {
				if (!normalizeGroups({ [group]: v })[group]) problems.push(`${where}: vigent can't read ${JSON.stringify(v)} (a group to link to, or "guifg=#rrggbb guibg=#rrggbb gui=bold")`);
				continue;
			}
			if (!isTable(v)) {
				problems.push(`${where} should be a table, e.g. { fg = "#c0caf5", bold = true }`);
				continue;
			}
			for (const [k, x] of Object.entries(v)) {
				if (!(k in ATTR_KEYS)) problems.push(`${where}: unknown attribute '${k}' (${Object.keys(ATTR_KEYS).join(", ")})`);
				else if (typeof x !== ATTR_KEYS[k]) problems.push(`${where}: '${k}' should be ${ATTR_KEYS[k] === "boolean" ? "true or false" : "a string"}`);
				else if ((k === "fg" || k === "bg") && !/^(#[0-9a-fA-F]{6}|NONE)$/.test(x)) problems.push(`${where}: ${k} = ${JSON.stringify(x)} should be "#rrggbb" or "NONE"`);
			}
		}
	}
	if (raw.style !== undefined) {
		const warnings = [];
		parseStyle(raw.style, `theme '${name}'`, warnings);
		problems.push(...warnings);
	}
	try {
		return buildTheme(raw, name);
	} catch (err) {
		problems.push(err.message);
		return undefined;
	}
}

/** The preview's colors, as vigent paints the theme. */
export function themeColors(theme) {
	const p = new Painter(theme);
	const t = { background: theme.background ?? "dark", transparent: Boolean(theme.transparent) };
	for (const [role, [group, key]] of Object.entries(ROLES)) {
		const v = p.attr(group)[key];
		if (typeof v === "string" && v.startsWith("#")) t[role] = v;
	}
	return t;
}

/** Only the tables a kind adds: [templates.<name>], [engines.<name>], [keys.*]. */
function checkShape(kind, name, raw, problems) {
	const only = { template: "templates", keymap: "keys", engine: "engines" }[kind];
	if (!only) return;
	for (const k of Object.keys(raw)) if (k !== only) problems.push(`a ${kind} entry has only [${only}${kind === "keymap" ? ".*" : `.${name}`}], not '${k}' (a setup can set anything)`);
	if (!isTable(raw[only])) {
		problems.push(`[${only}${kind === "keymap" ? ".normal" : `.${name}`}] is missing`);
		return;
	}
	if (kind === "keymap") {
		for (const [mode, table] of Object.entries(raw.keys)) {
			for (const [lhs, rhs] of Object.entries(isTable(table) ? table : { [mode]: table })) {
				// :command defines a command that can run a shell step: that's for setups, which wait for :trust
				const steps = [rhs].flat().filter((s) => typeof s === "string");
				if (steps.some((s) => /(^|<CR>|<cr>|\|)\s*:\s*com(m(a(nd?)?)?)?!?\s/.test(s))) problems.push(`[keys.${mode}] "${lhs}": a keymap can't define commands (a setup's [commands] can)`);
			}
		}
		return;
	}
	const names = Object.keys(raw[only]);
	if (names.length !== 1 || names[0] !== name) problems.push(`a ${kind} entry defines one table, [${only}.${name}] (this one has ${names.map((n) => `[${only}.${n}]`).join(", ") || "none"})`);
	if (kind === "engine") {
		if (ENGINE_IDS.includes(name)) problems.push(`'${name}' is a built-in engine; an entry adds a new one`);
		else if (!isTable(raw.engines[name]) || !("command" in raw.engines[name])) problems.push(`[engines.${name}] needs a command = [...]`);
	}
}

/**
 * Check one entry. Returns its problems (each naming the file) and, when there
 * are none, its line in the index.
 */
export function checkEntry(kind, name, files) {
	const problems = [];
	const at = `registry/${kind}/${name}`;
	if (!KINDS.includes(kind)) return { problems: [`registry/${kind}: unknown kind '${kind}' (${KINDS.join(", ")})`] };
	if (!NAME.test(name) || name.length > 40) problems.push(`${at}: '${name}' isn't a name: lowercase letters and digits, joined by -, at most 40 characters`);
	if (BUILTIN_THEMES[name]) problems.push(`${at}: '${name}' is a built-in theme's name (:colorscheme ${name}); pick another`);
	for (const f of Object.keys(files)) if (f !== "entry.toml" && f !== "meta.toml") problems.push(`${at}/${f}: an entry is entry.toml and meta.toml, nothing else`);
	if (files["meta.toml"] === undefined) problems.push(`${at}/meta.toml is missing (${Object.keys(META).join(", ")})`);
	if (files["entry.toml"] === undefined) problems.push(`${at}/entry.toml is missing`);

	const metaProblems = [];
	const meta = files["meta.toml"] === undefined ? undefined : checkMeta(files["meta.toml"], metaProblems);
	problems.push(...metaProblems.map((p) => `${at}/${p}`));

	const text = files["entry.toml"];
	const own = [];
	let raw;
	try {
		if (text !== undefined) raw = parseToml(text);
	} catch (err) {
		own.push(err.message.split("\n")[0]);
	}
	let colors;
	let runs = [];
	if (raw && kind === "theme") {
		const theme = checkTheme(name, raw, own);
		if (theme && !own.length) colors = themeColors(theme);
	} else if (raw) {
		checkShape(kind, name, raw, own);
		// vigent's own loader, as config.toml (trusted) and as an untrusted .vigent.toml (what waits for :trust)
		const label = kind === "setup" ? ".vigent.toml" : "config";
		const tmp = path.join(process.env.XDG_CONFIG_HOME, `${kind}-${name}.toml`);
		fs.writeFileSync(tmp, text);
		CUSTOM_ENGINES.clear();
		const { config, warnings } = loadConfig(tmp, { label });
		own.push(...warnings, ...buildKeymap(config.leader, config.keys).warnings);
		if (config.theme && !BUILTIN_THEMES[config.theme]) own.push(`theme = ${JSON.stringify(config.theme)}: a setup can only pick a built-in theme`);
		CUSTOM_ENGINES.clear();
		runs = runsOf(raw, loadConfig(tmp, { label, trusted: false }).gated);
		CUSTOM_ENGINES.clear();
		fs.rmSync(tmp);
	}
	problems.push(...own.map((p) => `${at}/entry.toml: ${p}`));
	if (problems.length) return { problems };
	return {
		problems,
		entry: {
			name,
			kind,
			description: meta.description.trim(),
			author: meta.author.trim(),
			license: meta.license.trim(),
			tags: meta.tags,
			file: text,
			runs,
			...(colors ? { colors } : {}),
		},
	};
}

/** Read and check every entry under dir. */
export function buildIndex(dir = REGISTRY_DIR) {
	const problems = [];
	const entries = [];
	for (const kind of fs.readdirSync(dir).sort()) {
		const kdir = path.join(dir, kind);
		if (!fs.statSync(kdir).isDirectory()) {
			if (kind !== "README.md") problems.push(`registry/${kind}: the registry holds a folder per kind (${KINDS.join(", ")})`);
			continue;
		}
		if (!KINDS.includes(kind)) {
			problems.push(`registry/${kind}: unknown kind '${kind}' (${KINDS.join(", ")})`);
			continue;
		}
		for (const name of fs.readdirSync(kdir).sort()) {
			const edir = path.join(kdir, name);
			if (!fs.statSync(edir).isDirectory()) {
				problems.push(`registry/${kind}/${name}: an entry is a folder, registry/${kind}/<name>/ with entry.toml and meta.toml`);
				continue;
			}
			const files = Object.fromEntries(fs.readdirSync(edir).map((f) => [f, fs.readFileSync(path.join(edir, f), "utf8")]));
			const r = checkEntry(kind, name, files);
			problems.push(...r.problems);
			if (r.entry) entries.push(r.entry);
		}
	}
	entries.sort((a, b) => KINDS.indexOf(a.kind) - KINDS.indexOf(b.kind) || a.name.localeCompare(b.name));
	return { problems, index: { version: 1, entries } };
}

export const indexText = (index) => `${JSON.stringify(index, null, 2)}\n`;

function main(argv) {
	const check = argv.includes("--check");
	const { problems, index } = buildIndex();
	if (problems.length) {
		for (const p of problems) console.error(p);
		console.error(`\n${problems.length} problem${problems.length === 1 ? "" : "s"} in the registry`);
		return 1;
	}
	const text = indexText(index);
	const rel = path.relative(root, INDEX_FILE);
	if (check) {
		const now = fs.existsSync(INDEX_FILE) ? fs.readFileSync(INDEX_FILE, "utf8") : undefined;
		if (now !== text) {
			console.error(`${rel} is out of date: run npm run registry and commit it`);
			return 1;
		}
		console.log(`${index.entries.length} entries ok; ${rel} is up to date`);
		return 0;
	}
	fs.mkdirSync(path.dirname(INDEX_FILE), { recursive: true });
	fs.writeFileSync(INDEX_FILE, text);
	console.log(`${index.entries.length} entries ok; wrote ${rel}`);
	return 0;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
	process.exitCode = main(process.argv.slice(2));
}
