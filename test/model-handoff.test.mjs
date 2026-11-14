/**
 * `:model` moves an open agent to a model on another engine, in the real
 * `vigent` in a pseudo-terminal: Tab completes the model's id, Enter moves
 * the agent from Claude Code to Codex in place, and the first prompt Codex
 * gets carries the conversation so far, also after a restart.
 */
import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import { after, test } from "node:test";
import { Tty } from "./support/terminal.mjs";

const skip = process.platform === "win32" || spawnSync("python3", ["-c", "import pty"]).status !== 0 ? "needs python3 and a Unix pty" : false;
const tmp = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), "vigent-handoff-")));
after(() => fs.rmSync(tmp, { recursive: true, force: true }));

const gitExec = execFileSync("git", ["--exec-path"], { encoding: "utf8" }).trim();
const GIT_IDENTITY = { GIT_AUTHOR_NAME: "T", GIT_AUTHOR_EMAIL: "t@e.com", GIT_COMMITTER_NAME: "T", GIT_COMMITTER_EMAIL: "t@e.com" };
const CATALOG = JSON.stringify({
	models: [
		{ slug: "gpt-5.5", display_name: "GPT-5.5", visibility: "list", supported_reasoning_levels: [{ effort: "high" }] },
		{ slug: "gpt-5.5-codex", display_name: "GPT-5.5 Codex", visibility: "list", supported_reasoning_levels: [{ effort: "high" }] },
	],
});

/** A machine with stand-ins for Claude Code and Codex that write down what they're asked, and a repo. */
function machine(name) {
	const root = path.join(tmp, name);
	const bin = path.join(root, "bin");
	const home = path.join(root, "home");
	fs.mkdirSync(bin, { recursive: true });
	fs.mkdirSync(home);
	fs.symlinkSync(process.execPath, path.join(bin, "node"));
	fs.symlinkSync(path.join(gitExec, "git"), path.join(bin, "git"));
	const logs = { claude: path.join(root, "claude.log"), codex: path.join(root, "codex.log") };
	fs.writeFileSync(
		path.join(bin, "claude"),
		`#!${process.execPath}
const fs = require("fs");
const args = process.argv.slice(2);
if (args[0] === "--version") { console.log("2.1.319 (Claude Code)"); process.exit(0); }
if (args.join(" ") === "auth status --json") { console.log(JSON.stringify({ loggedIn: true, authMethod: "claude.ai" })); process.exit(0); }
fs.appendFileSync(${JSON.stringify(logs.claude)}, JSON.stringify(args) + "\\n");
const say = (o) => console.log(JSON.stringify(o));
say({ type: "system", subtype: "init", session_id: "claude-1", model: "claude-opus-5-5" });
say({ type: "assistant", session_id: "claude-1", uuid: "m1", message: { content: [{ type: "text", text: "the badge counts lines" }] } });
say({ type: "result", subtype: "success", session_id: "claude-1", is_error: false, result: "the badge counts lines" });
`,
		{ mode: 0o755 },
	);
	fs.writeFileSync(
		path.join(bin, "codex"),
		`#!${process.execPath}
const fs = require("fs");
const args = process.argv.slice(2);
if (args[0] === "--version") { console.log("codex-cli 0.1.0"); process.exit(0); }
if (args.join(" ") === "login status") { console.log("Logged in using ChatGPT"); process.exit(0); }
if (args.join(" ") === "debug models") { process.stdout.write(${JSON.stringify(CATALOG)}); process.exit(0); }
const n = fs.existsSync(${JSON.stringify(logs.codex)}) ? fs.readFileSync(${JSON.stringify(logs.codex)}, "utf8").trim().split("\\n").length : 0;
fs.appendFileSync(${JSON.stringify(logs.codex)}, JSON.stringify(args) + "\\n");
const say = (o) => console.log(JSON.stringify(o));
say({ type: "thread.started", thread_id: "thread-1" });
say({ type: "item.completed", item: { id: "i1", type: "agent_message", text: "codex reply " + n } });
say({ type: "turn.completed", usage: { input_tokens: 10, output_tokens: 5 } });
`,
		{ mode: 0o755 },
	);
	const repo = path.join(root, "shop");
	fs.mkdirSync(repo);
	const env = { ...GIT_IDENTITY, PATH: bin, HOME: home, XDG_CONFIG_HOME: path.join(home, ".config"), TERM: "xterm-256color", LANG: "en_US.UTF-8", RIFTRI_BINARY: path.join(root, "no-riftri") };
	const git = (...args) => execFileSync("git", args, { cwd: repo, env: { ...process.env, ...GIT_IDENTITY }, stdio: "pipe" });
	git("init", "-q", "-b", "main");
	fs.writeFileSync(path.join(repo, "a.js"), "1\n");
	git("add", "a.js");
	git("commit", "-qm", "init");
	const calls = (engine) => (fs.existsSync(logs[engine]) ? fs.readFileSync(logs[engine], "utf8").trim().split("\n").map(JSON.parse) : []);
	return { repo, env, calls };
}

const HANDOFF =
	"You're continuing a conversation that ran on Claude Code (claude-opus-5-5) in this same worktree. Here it is so far:\n\n" +
	"<user>fix the badge</user>\n<assistant>the badge counts lines</assistant>\n\n" +
	"Continue from here. The files in this worktree are already as that conversation left them.\n\n";

async function waitCalls(calls, engine, n) {
	const until = Date.now() + 15000;
	while (calls(engine).length < n && Date.now() < until) await new Promise((r) => setTimeout(r, 50));
	return calls(engine);
}

/** Start an agent on Claude Code, and move it to Codex with :model gpt-<Tab>, as you would. */
async function startAndSwitch(t) {
	await t.waitFor(/● Claude Code +default/);
	await t.waitFor(/● Codex/);
	await t.type("i");
	await t.type("fix the badge\r");
	await t.waitFor(/the badge counts lines/);
	await t.waitFor(/Claude Code · claude-opus-5-5/);
	await t.type(":model gpt-");
	// Codex's models, with the names Codex gives them
	await t.waitFor(/gpt-5\.5 {2}GPT-5\.5 .*gpt-5\.5-codex {2}GPT-5\.5 Codex/);
	await t.type("\t");
	await t.waitFor(/:model gpt-5\.5\s*$/m);
	await t.type("\r");
	await t.waitFor("'fix badge' now runs gpt-5.5 (GPT-5.5, Codex)");
	await t.waitFor("switched from claude-opus-5-5 (Claude Opus 5.5, Claude Code) to gpt-5.5 (GPT-5.5, Codex); the next prompt carries the conversation over");
	await t.waitFor(/Codex · gpt-5\.5 \(GPT-5\.5\)/);
}

test(":model gpt-<Tab> moves a Claude Code agent to Codex in place; Codex's first prompt carries the conversation", { skip, timeout: 90000 }, async () => {
	const { repo, env, calls } = machine("live");
	const t = new Tty([], { cwd: repo, env, cols: 200 });
	try {
		await startAndSwitch(t);
		assert.match(t.screen(), /❯ fix the badge/, "the transcript stays");
		assert.equal((t.screen().match(/fix badge/g) ?? []).length >= 2, true, "same agent, same place in the sidebar");
		await t.type("i");
		await t.type("now the tests\r");
		await t.waitFor(/codex reply 0/);
		const [first] = await waitCalls(calls, "codex", 1);
		assert.equal(first.at(-1), `${HANDOFF}now the tests`, "the conversation, then the prompt");
		assert.deepEqual(first.slice(first.indexOf("-m"), first.indexOf("-m") + 2), ["-m", "gpt-5.5"]);
		assert.ok(!first.includes("resume"), "a new Codex conversation");
		assert.doesNotMatch(t.screen(), /You're continuing/, "the transcript shows only what you typed");
		await t.type("i");
		await t.type("and more\r");
		await t.waitFor(/codex reply 1/);
		const second = (await waitCalls(calls, "codex", 2))[1];
		assert.equal(second.at(-1), "and more", "only the first prompt carries it");
		assert.deepEqual(second.slice(1, 2), ["resume"]);
		assert.ok(second.includes("thread-1"), "then Codex's own conversation");
		assert.equal(calls("claude").length, 1, "Claude Code isn't asked again");
		await t.ex("q");
		assert.equal(await t.exited, 0);
	} finally {
		t.kill();
	}
});

test("moved but not prompted yet: after a restart it reopens on Codex and still hands the conversation over", { skip, timeout: 90000 }, async () => {
	const { repo, env, calls } = machine("restart");
	const t = new Tty([], { cwd: repo, env, cols: 200 });
	try {
		await startAndSwitch(t);
		await t.ex("q");
		assert.equal(await t.exited, 0);
	} finally {
		t.kill();
	}
	const again = new Tty([], { cwd: repo, env, cols: 200 });
	try {
		await again.waitFor(/reopened 1 agent from last time/);
		await again.waitFor(/Codex · gpt-5\.5 \(GPT-5\.5\)/);
		await again.type("i");
		await again.type("carry on\r");
		await again.waitFor(/codex reply 0/);
		const [first] = await waitCalls(calls, "codex", 1);
		assert.equal(first.at(-1), `${HANDOFF}carry on`);
		await again.ex("q");
		assert.equal(await again.exited, 0);
	} finally {
		again.kill();
	}
	// and once it went out, a restart doesn't send it again
	const third = new Tty([], { cwd: repo, env, cols: 200 });
	try {
		await third.waitFor(/reopened 1 agent from last time/);
		await third.type("i");
		await third.type("next\r");
		await third.waitFor(/codex reply 1/);
		const second = (await waitCalls(calls, "codex", 2))[1];
		assert.equal(second.at(-1), "next");
		assert.ok(second.includes("thread-1"), "Codex's conversation, saved");
		await third.ex("q");
		assert.equal(await third.exited, 0);
	} finally {
		third.kill();
	}
});
