/**
 * Text vigent didn't write (an agent's reply, a file in a diff, a CLI's model
 * name, a project's .vigent.toml) drawn on the screen: nothing in it may reach
 * the terminal as an escape sequence or control character. Each sink has a test.
 */
import assert from "node:assert/strict";
import { test } from "node:test";
import { splitView, unifiedView } from "../dist/diffview.js";
import { TextFloat } from "../dist/float.js";
import { parseDiff } from "../dist/hunks.js";
import { HunkReview } from "../dist/hunk-review.js";
import { PICKER_ICONS } from "../dist/picker.js";
import { BUILTIN_THEMES, Painter } from "../dist/theme.js";

/** What a hostile text carries: OSC 52 (clipboard), OSC 0 (title), an erase, a C1 OSC and a bidi override. */
export const HOSTILE = "A\x1b]52;c;UFdORUQ=\x07B\x1b]0;TITLE\x07C\x1b[2JD\u009d52;c;QzE=\u009cE\u202eF";

/**
 * Bytes a terminal acts on that vigent didn't mean to send: any OSC but a
 * link to http(s) (or a link's end), any CSI but colors, any other ESC
 * sequence, a BEL on its own, C0, C1 and bidi controls.
 */
export function unsafeBytes(text) {
	const rest = text
		.replace(/\x1b\[[0-9;:]*m/g, "")
		.replace(/\x1b\]8;;(?:https?:\/\/[\x21-\x7e]*)?(?:\x07|\x1b\\)/g, "")
		.replace(/\x1b_pi:c\x07/g, "");
	return [...rest.matchAll(/[\x00-\x1f\x7f-\x9f\u061c\u200e\u200f\u202a-\u202e\u2066-\u2069]/g)].map((m) => JSON.stringify(rest.slice(Math.max(0, m.index - 10), m.index + 10)));
}

export function assertSafe(lines, what) {
	const bad = lines.flatMap((l) => unsafeBytes(l));
	assert.deepEqual(bad, [], `${what}: control bytes reach the terminal`);
}

const painter = () => new Painter(BUILTIN_THEMES.default);
const HOSTILE_DIFF = [
	"diff --git a/evil.txt b/evil.txt",
	"index 1..2 100644",
	"--- a/evil.txt",
	"+++ b/evil.txt",
	`@@ -1,2 +1,2 @@ ${HOSTILE}`,
	` ${HOSTILE}`,
	`-${HOSTILE}`,
	`+${HOSTILE}\r`,
	"",
].join("\n");

test(":diff draws a file's escape sequences as text, in both layouts", () => {
	for (const view of [unifiedView(HOSTILE_DIFF), splitView(HOSTILE_DIFF, 200)]) {
		const float = new TextFloat(painter(), PICKER_ICONS.unicode, "diff", view.lines, () => 30, () => {}, () => {});
		const out = float.render(220);
		assertSafe(out, ":diff");
		assert.match(out.join("\n"), /A\\x1b\]52;c;UFdORUQ=\\x07B/, "you see what the file holds");
	}
	assert.deepEqual(unifiedView(HOSTILE_DIFF).jumps, [4]);
});

test(":review draws a hunk's lines, header and file name as text", () => {
	const files = parseDiff(HOSTILE_DIFF);
	const review = new HunkReview(painter(), PICKER_ICONS.unicode, "Review", files, () => 30, { close() {}, requestRender() {} });
	const out = review.render(220);
	assertSafe(out, ":review");
	assert.match(out.join("\n"), /\+A\\x1b\]52;c;UFdORUQ=\\x07B/);
});
