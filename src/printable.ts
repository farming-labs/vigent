/**
 * An agent's text made safe to draw inside a window. A terminal acts on control
 * characters instead of showing them: a tab jumps to the next tab stop, `\r`
 * goes back to the start of the row, and a cursor or erase sequence moves or
 * clears the whole row. Any of them puts text outside the window it belongs to,
 * over the window beside it or, once a row wraps, pushing the windows below down.
 * Other sequences do worse: OSC 52 writes the clipboard, OSC 0 the window title.
 *
 * Colors (SGR) stay, but not conceal (8), which would hide the text. Links
 * (OSC 8) go and their text stays: a link's text can name another place than
 * the one it opens, and the markdown renderer makes its own links, to http(s)
 * only, where the text shows where they go. Tabs become two spaces, like the
 * diff view. A lone `\r` keeps what came after it, which is what a progress bar
 * ends up showing. Every other escape sequence and control character is
 * dropped: C0, DEL, C1 (U+0080-U+009F, which some terminals read as ESC
 * sequences) and the bidi controls that reorder what a row shows.
 *
 * Text with an SGR the agent sent can still show it in any color, its
 * background's too; that hides nothing it couldn't hide with spaces.
 */
export function printable(text: string): string {
	if (!UNSAFE.test(text)) return text;
	return text
		.replace(/\r\n/g, "\n")
		.replace(ESCAPE, (seq) => (SGR.test(seq) ? withoutConceal(seq) : ""))
		.split("\n")
		.map((line) => line.slice(line.lastIndexOf("\r") + 1))
		.join("\n")
		.replace(/\t/g, "  ")
		.replace(/(\x1b\[[0-9;:]*m)|[\x00-\x09\x0b-\x1f\x7f-\x9f\u061c\u200e\u200f\u202a-\u202e\u2066-\u2069]/g, (m, keep) => keep ?? "");
}

/** printable with no colors either: file contents, names and titles, drawn in vigent's own colors */
export function plainText(text: string): string {
	return printable(text).replace(/\x1b\[[0-9;:]*m/g, "");
}

/** plainText on one line: runs of whitespace become one space */
export function oneLine(text: string): string {
	return plainText(text).replace(/\s+/g, " ").trim();
}

/** A control character a terminal acts on, or a bidi control: what config values and names may not hold. */
export const CONTROL = /[\x00-\x08\x0b\x0c\x0e-\x1f\x7f-\x9f\u061c\u200e\u200f\u202a-\u202e\u2066-\u2069]/;

/**
 * Text for a plain terminal with every character it would act on written out
 * instead: `\x1b`, `\x9b`, `\r`, `\u202e`. Tabs and newlines stay. For what
 * vigent prints from somewhere else (the registry's index) outside its own
 * screen, and file contents in a diff.
 */
export function escapeControls(text: string): string {
	return text.replace(new RegExp(`${CONTROL.source}|\\r`, "g"), (c) => {
		if (c === "\r") return "\\r";
		const n = c.codePointAt(0) as number;
		return n < 0x100 ? `\\x${n.toString(16).padStart(2, "0")}` : `\\u${n.toString(16).padStart(4, "0")}`;
	});
}

/**
 * A line of a file (a diff's) as vigent draws it: what a terminal would act on
 * is written out (`\x1b]52;…`), so you see what the file holds; tabs are two
 * spaces and a CRLF line's `\r` goes.
 */
export function fileText(line: string): string {
	return escapeControls(line.replace(/\r$/, "")).replace(/\t/g, "  ");
}

/**
 * Every escape sequence and control character gone, newlines and tabs aside:
 * what's left of a command once nothing in it can be a terminal's.
 */
export function withoutEscapes(text: string): string {
	return text.replace(ESCAPE, "").replace(/[\x00-\x08\x0b-\x1f\x7f-\x9f\u061c\u200e\u200f\u202a-\u202e\u2066-\u2069]/g, "");
}

const UNSAFE = /[\x00-\x09\x0b-\x1f\x7f-\x9f\u061c\u200e\u200f\u202a-\u202e\u2066-\u2069]/;

/** CSI, OSC, DCS/SOS/PM/APC strings, and two-byte escapes, in their 7-bit forms */
const ESCAPE = /\x1b\[[0-?]*[ -/]*[@-~]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)?|\x1b[PX^_][^\x1b]*(?:\x1b\\)?|\x1b[ -/]*[0-~]?/g;
const SGR = /^\x1b\[[0-9;:]*m$/;

/**
 * An SGR without conceal (8). The colors' own numbers stay: 38;5;8 is color 8,
 * not conceal. With nothing left it goes, since an empty SGR resets.
 */
function withoutConceal(seq: string): string {
	const params = seq.slice(2, -1).split(";");
	if (!params.includes("8")) return seq;
	const out: string[] = [];
	for (let i = 0; i < params.length; i++) {
		const p = params[i];
		if ((p === "38" || p === "48" || p === "58") && (params[i + 1] === "5" || params[i + 1] === "2")) {
			const n = params[i + 1] === "5" ? 3 : 5;
			out.push(...params.slice(i, i + n));
			i += n - 1;
		} else if (p !== "8") out.push(p);
	}
	return out.length ? `\x1b[${out.join(";")}m` : "";
}
