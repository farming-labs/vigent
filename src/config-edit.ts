/**
 * Edit config.toml in place for `:settings` and `vigent add`: change one
 * value, or add a registry entry's tables, and keep everything else
 * (comments, order, other keys) as the user wrote it.
 */
import { isDeepStrictEqual } from "node:util";
import { parse as parseToml } from "smol-toml";

/** A TOML value: strings quoted, booleans and numbers as they are. */
export function tomlValue(v: string | number | boolean): string {
	return typeof v === "string" ? JSON.stringify(v) : String(v);
}

/** A TOML key: bare when it can be, quoted otherwise (`"<leader>n"`). */
export function tomlKey(k: string): string {
	return /^[A-Za-z0-9_-]+$/.test(k) ? k : JSON.stringify(k);
}

const header = /^\s*\[([^\][]+)\]\s*(#.*)?$/;

/**
 * Set `key = value` in `[section]` (or at the top level when section is
 * undefined). A commented-out line for the same key (`# key = …`) is replaced
 * in place, so the setting stays where the template documents it.
 */
export function setTomlValue(text: string, section: string | undefined, key: string, value: string): string {
	const lines = text.length ? text.replace(/\n$/, "").split("\n") : [];
	const line = `${tomlKey(key)} = ${value}`;
	const keyRe = new RegExp(`^(\\s*)(#\\s*)?${escape(tomlKey(key))}\\s*=`);
	// the range of lines that belong to the section
	let start = 0;
	let end = lines.findIndex((l) => header.test(l));
	if (end < 0) end = lines.length;
	if (section) {
		const at = lines.findIndex((l) => header.exec(l)?.[1].trim() === section);
		if (at < 0) {
			// a new section at the end
			const out = [...lines];
			if (out.length && out[out.length - 1].trim() !== "") out.push("");
			out.push(`[${section}]`, line);
			return `${out.join("\n")}\n`;
		}
		start = at + 1;
		end = lines.findIndex((l, i) => i >= start && header.test(l));
		if (end < 0) end = lines.length;
	}
	const live = lines.findIndex((l, i) => i >= start && i < end && keyRe.test(l) && !/^\s*#/.test(l));
	const commented = lines.findIndex((l, i) => i >= start && i < end && keyRe.test(l) && /^\s*#/.test(l));
	const out = [...lines];
	const at = live >= 0 ? live : commented;
	if (at >= 0) {
		out[at] = line;
	} else {
		// after the last non-blank line of the section
		let insert = end;
		while (insert > start && out[insert - 1].trim() === "") insert--;
		out.splice(insert, 0, line);
	}
	return `${out.join("\n")}\n`;
}

function escape(s: string): string {
	return s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

// ---------------------------------------------------------------------------
// Adding tables (vigent add): a registry entry's [templates.x], [keys.normal] ...
// ---------------------------------------------------------------------------

/** One statement of a TOML file, by line: a [table] header, a key = value (as many lines as its value takes), or a comment or blank line. */
export interface TomlStatement {
	kind: "header" | "kv" | "other";
	/** its first line, and the line after its last */
	start: number;
	end: number;
	/** a header's table, or a key's path within its table */
	path: string[];
	/** [[an array of tables]] */
	array?: boolean;
}

const lineList = (text: string) => (text.length ? text.replace(/\n$/, "").split("\n") : []);

/** A dotted key at s[i]: its parts, and where it stops (after the spaces that follow it). */
function readKey(s: string, i: number): { path: string[]; at: number } | undefined {
	const parts: string[] = [];
	for (;;) {
		while (s[i] === " " || s[i] === "\t") i++;
		if (s[i] === '"') {
			let j = i + 1;
			while (j < s.length && s[j] !== '"') j += s[j] === "\\" ? 2 : 1;
			if (j >= s.length) return undefined;
			try {
				parts.push(JSON.parse(s.slice(i, j + 1)) as string);
			} catch {
				return undefined;
			}
			i = j + 1;
		} else if (s[i] === "'") {
			const j = s.indexOf("'", i + 1);
			if (j < 0) return undefined;
			parts.push(s.slice(i + 1, j));
			i = j + 1;
		} else {
			const m = /^[A-Za-z0-9_-]+/.exec(s.slice(i));
			if (!m) return undefined;
			parts.push(m[0]);
			i += m[0].length;
		}
		while (s[i] === " " || s[i] === "\t") i++;
		if (s[i] !== ".") return { path: parts, at: i };
		i++;
	}
}

/** The line after the one a value starting at lines[line][col] ends on: strings, arrays and inline tables can go on for lines. */
function valueEnd(lines: string[], line: number, col: number): number {
	let depth = 0;
	let str: string | undefined;
	for (let l = line; l < lines.length; l++) {
		const s = lines[l];
		for (let i = l === line ? col : 0; i < s.length; i++) {
			const c = s[i];
			if (str) {
				if (str[0] === '"' && c === "\\") i++;
				else if (s.startsWith(str, i)) {
					// """a""""" ends with the last three quotes
					if (str.length === 3) while (s[i + 3] === str[0]) i++;
					i += str.length - 1;
					str = undefined;
				}
			} else if (c === "#") break;
			else if (c === '"' || c === "'") {
				str = s.startsWith(c.repeat(3), i) ? c.repeat(3) : c;
				i += str.length - 1;
			} else if (c === "[" || c === "{") depth++;
			else if (c === "]" || c === "}") depth--;
		}
		// a one-line string ends with its line, closed or not
		if (str?.length === 1) str = undefined;
		if (!str && depth <= 0) return l + 1;
	}
	return lines.length;
}

/** Split TOML text into statements, by line. */
export function tomlStatements(text: string): TomlStatement[] {
	const lines = lineList(text);
	const out: TomlStatement[] = [];
	for (let i = 0; i < lines.length; ) {
		const t = lines[i].trim();
		if (t.startsWith("[")) {
			const array = t.startsWith("[[");
			out.push({ kind: "header", start: i, end: i + 1, path: readKey(t, array ? 2 : 1)?.path ?? [], array });
			i++;
			continue;
		}
		const k = t && !t.startsWith("#") ? readKey(lines[i], 0) : undefined;
		if (!k || lines[i][k.at] !== "=") {
			out.push({ kind: "other", start: i, end: i + 1, path: [] });
			i++;
			continue;
		}
		const end = valueEnd(lines, i, k.at + 1);
		out.push({ kind: "kv", start: i, end, path: k.path });
		i = end;
	}
	return out;
}

/** What an entry adds and owns: one name for messages, and every path that means it ([keys.normal] "x" is also [keys] "x"). */
export interface TomlUnit {
	name: string;
	paths: string[][];
}

export type MergeResult = { text: string; replaced: string[] } | { conflicts: string[] } | { error: string };

const isTable = (v: unknown): v is Record<string, unknown> => Boolean(v) && typeof v === "object" && !Array.isArray(v) && !(v instanceof Date);
const startsWith = (p: string[], prefix: string[]) => prefix.length <= p.length && prefix.every((k, i) => k === p[i]);
const CANT_ADD = "its tables are written in a way this edit can't add to";
const sameText = (a: string[], b: string[]) => a.length === b.length && startsWith(a, b);

/**
 * Add an entry's TOML to config.toml's text, keeping everything else as it's
 * written: a table the file already has gets the entry's keys at its end, a
 * new one goes at the end of the file. A unit the file already has is a
 * conflict, unless `force`, which takes the file's own out first. The result
 * is read back, and must be the file's settings plus the entry's, or nothing changes.
 */
export function mergeToml(text: string, entry: string, units: TomlUnit[], force = false): MergeResult {
	let mine: Record<string, unknown>;
	let adds: Record<string, unknown>;
	try {
		mine = parseToml(text) as Record<string, unknown>;
	} catch (err) {
		return { error: (err as Error).message.split("\n")[0] };
	}
	try {
		adds = parseToml(entry) as Record<string, unknown>;
	} catch (err) {
		return { error: `the entry: ${(err as Error).message.split("\n")[0]}` };
	}
	const at = (obj: unknown, p: string[]) => p.reduce<unknown>((v, k) => (isTable(v) ? v[k] : undefined), obj);
	const taken = units.filter((u) => u.paths.some((p) => at(mine, p) !== undefined));
	if (taken.length && !force) return { conflicts: taken.map((u) => u.name) };
	const owned = taken.flatMap((u) => u.paths);
	const under = (p: string[]) => owned.some((u) => startsWith(p, u));

	// the file's own, out
	let lines = lineList(text);
	if (owned.length) {
		const keep = lines.map(() => true);
		const stmts = tomlStatements(text);
		let section: string[] = [];
		stmts.forEach((s, i) => {
			if (s.kind === "header") {
				section = s.path;
				if (s.array || !under(section)) return;
				// the whole table, but not the comments after its last key: they're about the next one
				let last = i;
				for (let j = i + 1; j < stmts.length && stmts[j].kind !== "header"; j++) if (stmts[j].kind === "kv") last = j;
				keep.fill(false, s.start, stmts[last].end);
			} else if (s.kind === "kv" && under([...section, ...s.path])) keep.fill(false, s.start, s.end);
		});
		// no two blank lines where something came out
		for (let i = 0; i < lines.length; i++) {
			if (keep[i]) continue;
			let j = i;
			while (j < lines.length && !keep[j]) j++;
			if ((i === 0 || !lines[i - 1].trim()) && j < lines.length && !lines[j].trim()) keep[j] = false;
			i = j;
		}
		lines = lines.filter((_, i) => keep[i]);
	}

	// the entry, in blocks: what comes before its first header, then each table with the comments just above it
	const elines = lineList(entry);
	const blocks: { header?: number; path?: string[]; lead: string[]; body: string[] }[] = [{ lead: [], body: [] }];
	let pending: number[] = [];
	for (const s of tomlStatements(entry)) {
		const range = Array.from({ length: s.end - s.start }, (_, i) => s.start + i);
		if (s.kind === "other") pending.push(...range);
		else if (s.kind === "header") {
			blocks.push({ header: s.start, path: s.path, lead: pending.map((i) => elines[i]), body: [] });
			pending = [];
		} else {
			blocks[blocks.length - 1].body.push(...pending.map((i) => elines[i]), ...range.map((i) => elines[i]));
			pending = [];
		}
	}
	blocks[blocks.length - 1].body.push(...pending.map((i) => elines[i]));
	const trim = (l: string[]) => {
		let a = 0;
		let b = l.length;
		while (a < b && !l[a].trim()) a++;
		while (b > a && !l[b - 1].trim()) b--;
		return l.slice(a, b);
	};

	for (const b of blocks) {
		const lead = trim(b.lead);
		const body = trim(b.body);
		if (b.header === undefined && !body.length) continue;
		const stmts = tomlStatements(lines.length ? `${lines.join("\n")}\n` : "");
		const firstHeader = stmts.find((s) => s.kind === "header");
		const own = b.path && stmts.find((s) => s.kind === "header" && !s.array && sameText(s.path, b.path!));
		if (b.header === undefined || own) {
			// at the end of that table (or of the top, before the first table), after its last line that isn't blank
			const start = own ? own.end : 0;
			const next = stmts.find((s) => s.kind === "header" && s.start >= start);
			let insert = next ? next.start : lines.length;
			while (insert > start && !lines[insert - 1].trim()) insert--;
			// a comment the table already has (from adding this entry before) isn't repeated
			const there = new Set(lines.slice(start, insert).map((l) => l.trim()));
			const add = [...lead.filter((l) => !there.has(l.trim())), ...body];
			// a blank line between the top's settings and the first table
			if (!own && firstHeader && insert === firstHeader.start) add.push("");
			lines.splice(insert, 0, ...add);
		} else {
			// a table some key of the file already defines can't be given a header of its own
			let section: string[] = [];
			for (const s of stmts) {
				if (s.kind === "header") section = s.path;
				else if (s.kind === "kv" && startsWith(b.path!, [...section, ...s.path])) return { error: CANT_ADD };
			}
			if (lines.length && lines[lines.length - 1].trim()) lines.push("");
			lines.push(...lead, elines[b.header], ...body);
		}
	}
	const out = lines.length ? `${lines.join("\n")}\n` : "";

	// read back: the file's settings (without what was replaced) and the entry's, nothing else
	let got: unknown;
	try {
		got = parseToml(out);
	} catch (err) {
		return { error: (err as Error).message.split("\n")[0] };
	}
	const want = parseToml(text) as Record<string, unknown>;
	for (const p of owned) {
		const parent = at(want, p.slice(0, -1));
		if (isTable(parent)) delete parent[p[p.length - 1]];
	}
	const merge = (into: Record<string, unknown>, from: Record<string, unknown>) => {
		for (const [k, v] of Object.entries(from)) {
			if (isTable(v) && isTable(into[k])) merge(into[k] as Record<string, unknown>, v);
			else into[k] = v;
		}
	};
	merge(want, adds);
	if (!isDeepStrictEqual(got, want)) return { error: CANT_ADD };
	return { text: out, replaced: taken.map((u) => u.name) };
}
