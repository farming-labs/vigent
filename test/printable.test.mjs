import assert from "node:assert/strict";
import { test } from "node:test";
import { escapeControls, fileText, oneLine, plainText, printable } from "../dist/printable.js";

test("printable: text with nothing to fix comes back as it was", () => {
	const s = "plain text, 中文, 🙂 and \x1b[31mcolor\x1b[0m\nsecond line";
	assert.equal(printable(s), s);
});

test("printable: tabs are two spaces and CRLF is a newline", () => {
	assert.equal(printable("\tmodified:\tsrc/app.ts\r\nnext"), "  modified:  src/app.ts\nnext");
});

test("printable: a carriage return keeps what came after it, like a progress bar ends up", () => {
	assert.equal(printable("10%\r50%\r100%\ndone"), "100%\ndone");
});

test("printable: colors stay, links keep only their text; cursor moves, erases, titles and bells go", () => {
	const link = "\x1b]8;;https://example.com\x07site\x1b]8;;\x07";
	assert.equal(printable(`\x1b[1;32mok\x1b[0m ${link}`), "\x1b[1;32mok\x1b[0m site");
	assert.equal(printable("\x1b[2K\x1b[1Gcleared\x1b[3A\x1b[?25l"), "cleared");
	assert.equal(printable("\x1b]0;title\x07after\x07"), "after");
	assert.equal(printable("back\bspace\x00\x7f\x9b2J"), "backspace2J");
});

test("printable: a link never reaches the terminal, whatever its target or text", () => {
	assert.equal(printable("\x1b]8;;file:///etc/passwd\x07https://github.com\x1b]8;;\x07"), "https://github.com", "text that names another place");
	assert.equal(printable("\x1b]8;id=x;ssh://evil/\x1b\\label\x1b]8;;\x1b\\"), "label");
	// U+009C ends an OSC on a terminal that reads C1 in UTF-8: inside a kept link it started an OSC 52
	assert.equal(printable("\x1b]8;;http://a\u009c\u009d52;c;aGFja2Vk\x07after"), "after");
	assert.equal(printable("\x1b]8;;\u009b2J\u009b6n\x07x"), "x");
});

test("printable: C1 and bidi controls go; conceal goes and the colors around it stay", () => {
	assert.equal(printable("\u009d52;c;aGk=\u009c"), "52;c;aGk=");
	assert.equal(printable("abc\u202edcba\u2066x\u2069\u200e\u061c"), "abcdcbax");
	assert.equal(printable("\x1b[8mhidden\x1b[0m visible"), "hidden\x1b[0m visible");
	assert.equal(printable("\x1b[1;8;31mx\x07"), "\x1b[1;31mx");
	assert.equal(printable("\x1b[38;5;8;48;2;8;8;8mx\x07"), "\x1b[38;5;8;48;2;8;8;8mx", "color 8 isn't conceal");
});

test("plainText, oneLine and escapeControls", () => {
	assert.equal(plainText("\x1b[31mred\x1b[0m\tx\x1b]52;c;eA==\x07"), "red  x");
	assert.equal(oneLine("m-MODEL\x1b]0;T\x07\u009d0;C\u009c \n x"), "m-MODEL0;C x");
	assert.equal(escapeControls("ok\tname\n"), "ok\tname\n");
	assert.equal(escapeControls("a\x1b]52;c;eA==\x07b\u009b2J\u202e"), "a\\x1b]52;c;eA==\\x07b\\x9b2J\\u202e");
	assert.equal(fileText("+\tx \x1b]52;c;eA==\x07 a\rb\r"), "+  x \\x1b]52;c;eA==\\x07 a\\rb");
});
