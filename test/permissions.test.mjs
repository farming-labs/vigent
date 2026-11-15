import assert from "node:assert/strict";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import { after, test } from "node:test";
import { allowDetails, allowRule, allowRules, CLI_ADAPTERS, CliSession, commandPrograms, describeDenial, ruleProgram } from "../dist/cli-engines.js";

const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "vigent-permissions-"));
after(() => fs.rmSync(tmp, { recursive: true, force: true }));

const init = { type: "system", subtype: "init", session_id: "s-1", model: "claude-fable-5" };
const blocked = {
	type: "result",
	subtype: "success",
	is_error: false,
	result: "I couldn't run gh.",
	session_id: "s-1",
	permission_denials: [
		{ tool_name: "Bash", tool_use_id: "t1", tool_input: { command: "gh pr view 12 --json title" } },
		{ tool_name: "Bash", tool_use_id: "t2", tool_input: { command: "GH_PAGER= gh pr checks 12" } },
		{ tool_name: "WebFetch", tool_use_id: "t3", tool_input: { url: "https://example.com" } },
	],
};
const done = { type: "result", subtype: "success", is_error: false, result: "ok", session_id: "s-1" };

/**
 * A fake Claude Code: refused until it's run with --allowedTools, then fine.
 * Every command line goes to a log.
 */
function fakeClaude(name, refused = blocked) {
	const dir = path.join(tmp, name);
	fs.mkdirSync(dir, { recursive: true });
	const log = path.join(dir, "argv.log");
	const file = path.join(dir, "claude");
	fs.writeFileSync(
		file,
		`#!/usr/bin/env node
const fs = require("fs");
const argv = process.argv.slice(2);
fs.appendFileSync(${JSON.stringify(log)}, JSON.stringify(argv) + "\\n");
const allowed = argv.includes("--allowedTools");
for (const e of [${JSON.stringify(init)}, allowed ? ${JSON.stringify(done)} : ${JSON.stringify(refused)}]) console.log(JSON.stringify(e));
`,
	);
	fs.chmodSync(file, 0o755);
	return { bin: file, calls: () => fs.readFileSync(log, "utf8").trim().split("\n").map(JSON.parse) };
}

function settle(s) {
	return new Promise((resolve) => {
		const off = s.onChange(() => {
			if (s.state === "idle" || s.state === "error") {
				off();
				resolve();
			}
		});
	});
}

test("a refused call becomes a permission rule: the program for a command, the tool's name otherwise", () => {
	assert.equal(allowRule({ name: "Bash", input: { command: "gh pr view 12" } }), "Bash(gh:*)");
	assert.equal(allowRule({ name: "Bash", input: { command: "  FOO=1 BAR=2 cargo test --all" } }), "Bash(cargo:*)");
	assert.equal(allowRule({ name: "Bash", input: { command: "$(evil)" } }), "Bash(evil:*)", "what $(...) runs is a program too");
	assert.equal(allowRule({ name: "Bash", input: { command: "   " } }), undefined, "no rule: a bare Bash would allow every command");
	assert.equal(allowRule({ name: "WebFetch", input: { url: "https://x.dev" } }), "WebFetch");
	assert.equal(describeDenial({ name: "Bash", input: { command: "gh  pr\nview 12" } }), "gh pr view 12");
	assert.equal(describeDenial({ name: "WebFetch", input: { url: "https://x.dev" } }), "WebFetch https://x.dev");
});

test("Claude Code's permission_denials are read from its result", () => {
	const evs = CLI_ADAPTERS.claude.parser()(blocked);
	const denied = evs.find((e) => e.type === "denied");
	assert.equal(denied.tools.length, 3);
	assert.deepEqual(denied.tools[0], { name: "Bash", input: { command: "gh pr view 12 --json title" } });
	assert.ok(evs.at(-1).type === "done" && evs.at(-1).ok);
	assert.equal(CLI_ADAPTERS.claude.parser()(done).find((e) => e.type === "denied"), undefined);
});

test("allow rules from config go to every run as --allowedTools", async () => {
	const fake = fakeClaude("allow");
	const s = new CliSession("a", "claude", undefined, { cwd: tmp, bin: fake.bin, allow: () => ["Bash(gh:*)"] });
	s.send("check the PR");
	await settle(s);
	const argv = fake.calls()[0];
	assert.deepEqual(argv.slice(argv.indexOf("--allowedTools"), argv.indexOf("--allowedTools") + 2), ["--allowedTools", "Bash(gh:*)"]);
	assert.ok(argv.indexOf("--allowedTools") < argv.indexOf("--"), "before the prompt");
	s.dispose();
});

test("without ask (the default): a refused tool is said, with how to allow it; nothing is asked", async () => {
	const fake = fakeClaude("off");
	const s = new CliSession("b", "claude", undefined, { cwd: tmp, bin: fake.bin });
	let asked = 0;
	s.approver = async () => {
		asked++;
		return "once";
	};
	assert.equal(s.asksToAllow, false);
	s.send("check the PR");
	await settle(s);
	assert.equal(asked, 0);
	assert.equal(fake.calls().length, 1);
	const info = s.items.filter((i) => i.kind === "info").map((i) => i.text);
	assert.ok(info.some((t) => /blocked by Claude Code's permissions: gh pr view 12 --json title · GH_PAGER= gh pr checks 12 · WebFetch https:\/\/example.com/.test(t)), info.join("\n"));
	assert.ok(info.some((t) => t.includes(":permissions allow Bash(gh:*) WebFetch, then ask it to try again") && t.includes("next prompt")), info.join("\n"));
	s.dispose();
});

test("with ask: allow once retries with the rules for that run only, and resumes the conversation", async () => {
	const fake = fakeClaude("once");
	const s = new CliSession("c", "claude", undefined, { cwd: tmp, bin: fake.bin, ask: true });
	const seen = [];
	s.approver = async (denied, rules) => {
		seen.push({ state: s.state, n: denied.length, rules });
		return "once";
	};
	s.send("check the PR");
	await settle(s);
	assert.deepEqual(seen, [{ state: "waiting", n: 3, rules: ["Bash(gh:*)", "WebFetch"] }]);
	const calls = fake.calls();
	assert.equal(calls.length, 2);
	assert.ok(!calls[0].includes("--allowedTools"));
	const retry = calls[1];
	assert.deepEqual(retry.slice(retry.indexOf("--allowedTools"), retry.indexOf("--allowedTools") + 3), ["--allowedTools", "Bash(gh:*)", "WebFetch"]);
	assert.deepEqual(retry.slice(-4, -1), ["--resume", "s-1", "--"]);
	assert.match(retry.at(-1), /permission to use Bash\(gh:\*\), WebFetch/);
	assert.deepEqual(s.items.filter((i) => i.kind === "user").map((i) => i.text), ["check the PR"], "the retry isn't shown as your prompt");
	assert.ok(s.items.some((i) => i.kind === "info" && i.text === "allowed once: Bash(gh:*), WebFetch · retrying"));
	assert.equal(s.state, "idle");

	// once means once: the next prompt runs without them
	s.send("now merge it");
	await settle(s);
	assert.equal(fake.calls().length, 4, "refused again, asked again, retried");
	s.dispose();
});

test("with ask: always allow keeps the rules; no leaves it blocked", async () => {
	const fake = fakeClaude("always");
	const kept = [];
	const s = new CliSession("d", "claude", undefined, { cwd: tmp, bin: fake.bin, ask: true, onAlwaysAllow: (r) => kept.push(...r) });
	s.approver = async () => "always";
	s.send("check the PR");
	await settle(s);
	assert.deepEqual(kept, ["Bash(gh:*)", "WebFetch"]);
	assert.ok(s.items.some((i) => i.kind === "info" && i.text.startsWith("always allowed: ")));

	const no = fakeClaude("no");
	const n = new CliSession("e", "claude", undefined, { cwd: tmp, bin: no.bin, ask: true });
	n.approver = async () => undefined;
	n.send("check the PR");
	await settle(n);
	assert.equal(no.calls().length, 1);
	assert.equal(n.items.at(-1).text, "left blocked");
	assert.equal(n.state, "idle");
	s.dispose();
	n.dispose();
});

test("config: [engines.claude] allow and ask; they're Claude Code's only", async () => {
	const { loadConfig } = await import("../dist/config.js");
	const file = path.join(tmp, "config.toml");
	fs.writeFileSync(
		file,
		`[engines.claude]
allow = ["Bash(gh:*)", "Bash(cargo:*)"]
ask = true
[engines.codex]
ask = true
`,
	);
	const { config, warnings } = loadConfig(file);
	assert.deepEqual(config.engines.claude, { allow: ["Bash(gh:*)", "Bash(cargo:*)"], ask: true });
	assert.deepEqual(warnings, ["config [engines.codex]: 'ask' is for [engines.claude] (Claude Code's permission rules)"]);
	fs.writeFileSync(file, `[engines.claude]\nask = "yes"\nallow = "gh"\n`);
	assert.deepEqual(loadConfig(file).warnings, [
		"config [engines.claude]: 'ask' should be true or false",
		`config [engines.claude]: 'allow' should be a list of permission rules, e.g. ["Bash(gh:*)"]`,
	]);
});

test("the app asks in a picker: allow once / always allow / no, naming the agent and what was blocked", async () => {
	const { App } = await import("../dist/app.js");
	const { DEFAULT_UI } = await import("../dist/config.js");
	const { Keymap } = await import("../dist/keys.js");
	const { BUILTIN_THEMES, Painter } = await import("../dist/theme.js");
	const overlays = [];
	const tui = { requestRender() {}, showOverlay: (c) => (overlays.push(c), { hide() {} }) };
	const app = new App(tui, { rows: 30, columns: 120, write() {} }, new Painter(BUILTIN_THEMES.default), { ...DEFAULT_UI }, new Keymap());
	const fake = fakeClaude("app");
	const s = new CliSession("pr", "claude", undefined, { cwd: tmp, bin: fake.bin, ask: true });
	app.addSession(s);
	assert.equal(typeof s.approver, "function", "the app answers for Claude Code agents");

	s.send("check the PR");
	while (!overlays.length) await new Promise((r) => setTimeout(r, 10));
	const plain = overlays[0].render(100).join("\n").replace(/\x1b\[[0-9;]*m/g, "");
	assert.match(plain, /'pr' was blocked · gh pr view 12/);
	assert.match(plain, /allow once/);
	assert.match(plain, /always allow/);
	assert.match(plain, /saved to \[engines\.claude\] allow/);
	assert.equal(s.state, "waiting");
	overlays[0].handleInput("\r"); // the first choice: allow once
	await settle(s);
	assert.equal(fake.calls().length, 2);
	assert.ok(fake.calls()[1].includes("--allowedTools"));
	s.dispose();
});

test("chained commands: a rule for every program a pipeline, list, loop or $(...) runs", () => {
	const cases = [
		["gh run list --limit 200 --json databaseId,headSha | jq -r '.[] | select(.x)'", ["gh", "jq"]],
		['for id in 42727 409956; do gh run view $id --log-failed | grep -n "ETIMEDOUT"; done', ["gh", "grep"]],
		['echo "--- per job; still text"; gh api "repos/{owner}/{repo}/actions/jobs/129"', ["echo", "gh"]],
		["GH_PAGER= gh pr checks 12", ["gh"]],
		["cd crates && cargo test -p riftri-git 2>&1 | tail -20", ["cd", "cargo", "tail"]],
		["x=$(git rev-parse HEAD) && echo $x", ["git", "echo"]],
		["if [ -f a ]; then rm a; fi", ["rm"]],
		["while [[ -n $x ]]; do sleep 1; done", ["sleep"]],
		["npm test > out.log 2>&1 &", ["npm"]],
	];
	for (const [cmd, progs] of cases) assert.deepEqual(commandPrograms(cmd), progs, cmd);
	assert.deepEqual(allowRules({ name: "Bash", input: { command: "for id in 1 2; do gh run view $id | jq .x; done" } }), ["Bash(gh:*)", "Bash(jq:*)"], "never Bash(for:*)");
});

/** A fake Claude Code that refuses one command whatever it's run with. */
function refusingClaude(name, command) {
	const dir = path.join(tmp, name);
	fs.mkdirSync(dir, { recursive: true });
	const file = path.join(dir, "claude");
	const result = { type: "result", subtype: "success", is_error: false, result: "couldn't", session_id: "s-9", permission_denials: [{ tool_name: "Bash", tool_use_id: "t", tool_input: { command } }] };
	fs.writeFileSync(file, `#!/usr/bin/env node\nfor (const e of [${JSON.stringify(init)}, ${JSON.stringify(result)}]) console.log(JSON.stringify(e));\n`);
	fs.chmodSync(file, 0o755);
	return file;
}

test("the refused-tool message: what to allow (only what isn't yet), from the next prompt; and when it's all allowed already", async () => {
	const loop = "for id in 1 2; do gh run view $id | jq .x; done";
	let allow = ["Bash(gh:*)"];
	const s = new CliSession("loop", "claude", undefined, { cwd: tmp, bin: refusingClaude("loop", loop), allow: () => allow });
	s.send("look at the runs");
	await settle(s);
	let info = s.items.filter((i) => i.kind === "info").map((i) => i.text);
	assert.ok(info.some((t) => t.startsWith(`blocked by Claude Code's permissions: ${loop}`)), info.join("\n"));
	assert.ok(info.some((t) => /^to allow it: :permissions allow Bash\(jq:\*\), then ask it to try again \(it's saved, and applies from its next prompt/.test(t)), `only jq is missing: ${info.join("\n")}`);

	// everything is allowed and Claude Code still refused: say so, and what does work
	allow = ["Bash(gh:*)", "Bash(jq:*)"];
	s.send("again");
	await settle(s);
	info = s.items.filter((i) => i.kind === "info").map((i) => i.text);
	assert.ok(info.some((t) => /^Bash\(gh:\*\) Bash\(jq:\*\) are allowed, and Claude Code still refused: .*bypassPermissions/.test(t)), info.join("\n"));
	s.dispose();
});


/** A fake Claude Code that refuses the same calls whatever it's run with, and logs its argv. */
function stubbornClaude(name, denials) {
	const dir = path.join(tmp, name);
	fs.mkdirSync(dir, { recursive: true });
	const log = path.join(dir, "argv.log");
	const file = path.join(dir, "claude");
	const result = { type: "result", subtype: "success", is_error: false, result: "couldn't", session_id: "s-7", permission_denials: denials };
	fs.writeFileSync(file, `#!/usr/bin/env node\nrequire("fs").appendFileSync(${JSON.stringify(log)}, JSON.stringify(process.argv.slice(2)) + "\\n");\nfor (const e of [${JSON.stringify(init)}, ${JSON.stringify(result)}]) console.log(JSON.stringify(e));\n`);
	fs.chmodSync(file, 0o755);
	return { bin: file, calls: () => fs.readFileSync(log, "utf8").trim().split("\n").map(JSON.parse) };
}
const infos = (s) => s.items.filter((i) => i.kind === "info").map((i) => i.text);

test("with ask: always allow, then refused again anyway: it says so instead of asking about the same rules forever", async () => {
	const fake = stubbornClaude("loop-always", [{ tool_name: "Bash", tool_use_id: "t", tool_input: { command: "gh pr merge 12" } }]);
	let allow = [];
	const s = new CliSession("f", "claude", undefined, { cwd: tmp, bin: fake.bin, ask: true, allow: () => allow, onAlwaysAllow: (r) => (allow = [...allow, ...r]) });
	let asked = 0;
	s.approver = async () => {
		asked++;
		return "always";
	};
	s.send("merge it");
	await settle(s);
	assert.equal(asked, 1, "asked once");
	assert.deepEqual(allow, ["Bash(gh:*)"]);
	assert.equal(fake.calls().length, 2, "the first run and the retry; no third");
	assert.ok(infos(s).some((t) => /^Bash\(gh:\*\) is allowed, and Claude Code still refused/.test(t)), infos(s).join("\n"));
	assert.equal(s.state, "idle");
	s.dispose();
});

test("with ask: only what isn't allowed yet is asked about", async () => {
	const fake = stubbornClaude("partly", [{ tool_name: "Bash", tool_use_id: "t", tool_input: { command: "gh run view 3 | jq .x" } }]);
	const s = new CliSession("g", "claude", undefined, { cwd: tmp, bin: fake.bin, ask: true, allow: () => ["Bash(gh:*)"] });
	const seen = [];
	s.approver = async (_d, rules) => {
		seen.push(rules);
		return undefined;
	};
	s.send("look");
	await settle(s);
	assert.deepEqual(seen, [["Bash(jq:*)"]]);
	s.dispose();
});

test("bypassPermissions: what Claude Code still refuses is said, never asked about", async () => {
	const fake = stubbornClaude("bypass", [{ tool_name: "Write", tool_use_id: "t", tool_input: { file_path: ".git/hooks/pre-commit" } }]);
	const s = new CliSession("h", "claude", undefined, { cwd: tmp, bin: fake.bin, ask: true, extraArgs: () => ["--permission-mode", "bypassPermissions"] });
	let asked = 0;
	s.approver = async () => {
		asked++;
		return "always";
	};
	s.send("add a hook");
	await settle(s);
	assert.equal(asked, 0);
	assert.equal(fake.calls().length, 1);
	assert.ok(infos(s).some((t) => t.startsWith("blocked by Claude Code's permissions: Write .git/hooks/pre-commit")), infos(s).join("\n"));
	assert.ok(infos(s).some((t) => /refused this even with bypassPermissions/.test(t)), infos(s).join("\n"));
	assert.ok(!infos(s).some((t) => t.startsWith("to allow it")), "no rule to add");
	s.dispose();
});

test("AskUserQuestion and ExitPlanMode aren't permissions: the question is shown, nothing is asked or allowed", async () => {
	const question = { questions: [{ question: "Which database?", header: "DB", options: [{ label: "Postgres" }, { label: "SQLite" }], multiSelect: false }] };
	const fake = stubbornClaude("question", [{ tool_name: "AskUserQuestion", tool_use_id: "t", tool_input: question }]);
	const s = new CliSession("i", "claude", undefined, { cwd: tmp, bin: fake.bin, ask: true });
	let asked = 0;
	s.approver = async () => {
		asked++;
		return "always";
	};
	s.send("set up storage");
	await settle(s);
	assert.equal(asked, 0);
	assert.deepEqual(infos(s), ["Claude Code wanted to ask you: Which database? (Postgres / SQLite) · answer with i"]);
	assert.equal(s.state, "idle");

	const plan = stubbornClaude("plan", [{ tool_name: "ExitPlanMode", tool_use_id: "t", tool_input: { plan: "1. do it" } }]);
	const p = new CliSession("j", "claude", undefined, { cwd: tmp, bin: plan.bin, ask: true, extraArgs: () => ["--permission-mode", "plan"] });
	p.approver = async () => "always";
	p.send("plan it");
	await settle(p);
	assert.deepEqual(infos(p), ["Claude Code has a plan ready and wants to start on it: set Mode to something other than plan in :permissions, then tell it to go ahead"]);
	s.dispose();
	p.dispose();
});

const E = "\x1b";
const BEL = "\x07";
/** the security review's command: escapes and a title in it, and more programs than a note can show */
const HOSTILE_COMMAND = `npm test ${E}]52;c;UEVSTQ==${BEL}${E}]0;PERMTITLE${BEL}${E}[2J && echo ok && true && ls && cat x && pwd && date && whoami && id && uname && env && printf x && python3 -c 1`;
const HOSTILE_PROGRAMS = ["npm", "echo", "true", "ls", "cat", "pwd", "date", "whoami", "id", "uname", "env", "printf", "python3"];

test("a refused command with escape sequences: shown written out, its rules computed without them", () => {
	const d = { name: "Bash", input: { command: HOSTILE_COMMAND } };
	assert.ok(describeDenial(d).startsWith("npm test \\x1b]52;c;UEVSTQ==\\x07\\x1b]0;PERMTITLE\\x07\\x1b[2J && echo ok"));
	assert.doesNotMatch(describeDenial(d, Infinity), /[\x00-\x1f\x7f-\x9f]/);
	assert.equal(describeDenial(d).length, 80, "the title stays short");
	assert.ok(describeDenial(d, Infinity).endsWith("python3 -c 1"), "the details have all of it");
	// an escape sequence's ; is no command boundary: no Bash(c:*) or Bash(UEVSTQ==:*)
	assert.deepEqual(commandPrograms(HOSTILE_COMMAND), HOSTILE_PROGRAMS);
	assert.deepEqual(describeDenial({ name: `Web${E}]0;x${BEL}Fetch`, input: { url: "https://x.dev" } }), "Web\\x1b]0;x\\x07Fetch https://x.dev");
});

test("no rule for a program a rule would read as a wildcard, nor for a bare Bash", () => {
	for (const w of ["*", "", "?", "a*", "[ab]", "{a,b}", "x:y", "a)b", "-rf", "e\u009b"]) assert.equal(ruleProgram(w), false, JSON.stringify(w));
	for (const w of ["gh", "./scripts/test.sh", "/usr/bin/env", "~/bin/x", "python3.12", "g++", "mcp__github__get_me"]) assert.equal(ruleProgram(w), true, w);
	const rules = (command) => allowRules({ name: "Bash", input: { command } });
	assert.deepEqual(rules("* -rf /"), [], "Claude Code reads Bash(*:*) as every command");
	assert.deepEqual(rules("ls && ?? x"), [], "one program a rule can't name: none for the command");
	assert.deepEqual(rules("$X"), [], "a program vigent can't tell");
	assert.deepEqual(rules("ls; [ab] x"), []);
	assert.deepEqual(allowRules({ name: "Web*", input: {} }), []);
});

test("refused with no rule that allows only it: it says so, and doesn't ask", async () => {
	const fake = fakeClaude("wildcard", { ...blocked, permission_denials: [{ tool_name: "Bash", tool_use_id: "t1", tool_input: { command: "* -rf /" } }] });
	const s = new CliSession("w", "claude", undefined, { cwd: tmp, bin: fake.bin, ask: true });
	let asked = false;
	s.approver = async () => ((asked = true), "always");
	s.send("go");
	await settle(s);
	assert.equal(asked, false);
	assert.ok(s.items.some((i) => i.kind === "info" && i.text.startsWith("no rule allows only * -rf /")), JSON.stringify(s.items));
	s.dispose();
});

test("the permission prompt shows every rule always allow saves, and no escape reaches the screen", async () => {
	const { App } = await import("../dist/app.js");
	const { DEFAULT_UI } = await import("../dist/config.js");
	const { Keymap } = await import("../dist/keys.js");
	const { BUILTIN_THEMES, Painter } = await import("../dist/theme.js");
	const overlays = [];
	const tui = { requestRender() {}, showOverlay: (c) => (overlays.push(c), { hide() {} }) };
	// a short terminal: the details don't fit, and scroll
	const app = new App(tui, { rows: 16, columns: 80, write() {} }, new Painter(BUILTIN_THEMES.default), { ...DEFAULT_UI }, new Keymap());
	const fake = fakeClaude("hostile", { ...blocked, permission_denials: [{ tool_name: "Bash", tool_use_id: "t1", tool_input: { command: HOSTILE_COMMAND } }] });
	const kept = [];
	const s = new CliSession("pr", "claude", undefined, { cwd: tmp, bin: fake.bin, ask: true, onAlwaysAllow: (r) => kept.push(...r) });
	app.addSession(s);
	s.send("test it");
	while (!overlays.length) await new Promise((r) => setTimeout(r, 10));
	const picker = overlays[0];
	const shown = new Set();
	let screen = "";
	for (let i = 0; i < 20; i++) {
		const out = picker.render(76);
		assert.equal(out.filter((l) => /[\x00-\x1a\x1c-\x1f\x7f-\x9f]|\x1b(?!\[[0-9;]*m)/.test(l)).length, 0, "no control byte but colors");
		screen = out.join("\n").replace(/\x1b\[[0-9;]*m/g, "");
		for (const m of screen.matchAll(/Bash\([^)]*\)/g)) shown.add(m[0]);
		picker.handleInput("\x05"); // Ctrl-e: one row down
	}
	const rules = HOSTILE_PROGRAMS.map((p) => `Bash(${p}:*)`);
	assert.deepEqual([...shown].filter((r) => !rules.includes(r)), [], "no junk rule");
	for (const r of rules) assert.ok(shown.has(r), `${r} shows before you choose`);
	assert.match(screen, /Ctrl-d\/Ctrl-u scroll/);
	picker.handleInput("j");
	picker.handleInput("\r"); // always allow
	await settle(s);
	assert.deepEqual(kept, rules, "it saves what it showed");
	s.dispose();
});

test("allowDetails: each refused call and the rules, in full", () => {
	const d = [{ name: "Bash", input: { command: "gh pr view 12" } }];
	assert.deepEqual(allowDetails(d, ["Bash(gh:*)"]), ["blocked: gh pr view 12", "rule: Bash(gh:*)"]);
	assert.deepEqual(allowDetails([...d, { name: "WebFetch", input: { url: "https://x.dev" } }], ["Bash(gh:*)", "WebFetch"]), ["blocked: gh pr view 12", "blocked: WebFetch https://x.dev", "2 rules: Bash(gh:*) WebFetch"]);
});
