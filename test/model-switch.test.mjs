/**
 * `:model` with another engine's model moves an open agent there, in place:
 * the same agent, a session on the new engine, and the conversation so far
 * carried over in the first prompt it sends.
 */
import assert from "node:assert/strict";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import { after, test } from "node:test";
import { App } from "../dist/app.js";
import { modelChoices } from "../dist/choices.js";
import { CliSession } from "../dist/cli-engines.js";
import { completeCommandLine } from "../dist/completion.js";
import { DEFAULT_UI } from "../dist/config.js";
import { Keymap } from "../dist/keys.js";
import { PiSession } from "../dist/pi-session.js";
import { BUILTIN_THEMES, Painter } from "../dist/theme.js";
import { realPi, until } from "./support/pi.mjs";

const tmp = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), "vigent-switch-")));
after(() => fs.rmSync(tmp, { recursive: true, force: true }));
const plain = (s) => s.replace(/\x1b\[[0-9;]*m/g, "");
const tick = () => new Promise((r) => setTimeout(r, 10));
const fixture = path.join(import.meta.dirname, "fixtures", "claude-tool.jsonl");

// a stand-in Claude Code: writes down its arguments, answers with a recorded turn
const log = path.join(tmp, "claude.log");
const claude = path.join(tmp, "claude");
fs.writeFileSync(claude, `#!${process.execPath}\nconst fs = require("fs");\nfs.appendFileSync(${JSON.stringify(log)}, JSON.stringify(process.argv.slice(2)) + "\\n");\nprocess.stdout.write(fs.readFileSync(${JSON.stringify(fixture)}, "utf8"));\n`, { mode: 0o755 });
const calls = () => (fs.existsSync(log) ? fs.readFileSync(log, "utf8").trim().split("\n").map(JSON.parse) : []);
const prompt = (args) => args.at(-1);

const ENGINES = [
	{ id: "pi", name: "pi", installed: true, connected: true, detail: "" },
	{ id: "claude", name: "Claude Code", installed: true, connected: true, detail: "" },
	{ id: "codex", name: "Codex", installed: true, connected: true, detail: "" },
	{ id: "opencode", name: "OpenCode", installed: true, connected: true, detail: "" },
	{ id: "gemini", name: "Gemini CLI", installed: true, connected: true, detail: "" },
];
const CODEX = [
	{ value: "default", label: "Default" },
	{ value: "gpt-5.5", label: "GPT-5.5", note: "gpt-5.5" },
];

/** An app with one agent on `engine`, and what a switch makes (Claude Code is the stand-in above). */
function setup(engine = "codex", model = "gpt-5.5") {
	const overlays = [];
	const tui = { requestRender() {}, showOverlay: (c) => (overlays.push(c), { hide() {} }) };
	const app = new App(tui, { rows: 40, columns: 140, write() {} }, new Painter(BUILTIN_THEMES.default), { ...DEFAULT_UI }, new Keymap());
	app.setEngines(ENGINES.map((e) => ({ ...e })));
	app.modelChoices = async (e) => (e === "codex" ? CODEX : e === "pi" || e === "opencode" ? [] : modelChoices(e, { PATH: "" }));
	const made = [];
	app.switchSession = (old, to) => {
		made.push(to);
		return new CliSession(old.name, to.engine, to.model, { cwd: tmp, bin: to.engine === "claude" ? claude : "/nonexistent", effort: to.effort });
	};
	const persisted = [];
	app.persist = (s) => persisted.push({ engine: s.engine, model: s.chosenModel ?? s.model, effort: s.effort, handoff: s.handoff });
	// pi isn't a CLI: a stand-in that says it runs on pi
	const s = new CliSession("badge", engine === "pi" ? "claude" : engine, model, { cwd: tmp, bin: "/nonexistent" });
	s.engine = engine;
	s.items.push(
		{ kind: "user", text: "fix the badge" },
		{ kind: "tool", text: "", tool: { name: "Bash", summary: "npm test", output: "", folded: true } },
		{ kind: "agent", text: "It counts lines; it should sum quantities." },
	);
	app.addSession(s, true);
	const ex = (cmd) => [":", ...cmd, "\r"].forEach((k) => app.handleInput(k));
	return { app, s, ex, made, persisted, overlays };
}

const HANDOFF =
	"You're continuing a conversation that ran on Codex (gpt-5.5) in this same worktree. Here it is so far:\n\n" +
	"<user>fix the badge</user>\nBash: npm test\n<assistant>It counts lines; it should sum quantities.</assistant>\n\n" +
	"Continue from here. The files in this worktree are already as that conversation left them.\n\n";

const settled = (s) => until(() => s.state !== "running" && !s.finishing, 10000, "the turn to end");

test("an exact id another engine lists moves the agent there, in place, and its first prompt carries the conversation", async () => {
	fs.rmSync(log, { force: true });
	const { app, s, ex, made, persisted } = setup();
	const fork = new CliSession("badge (fork)", "codex", undefined, { cwd: tmp, bin: "/nonexistent" });
	fork.forkOf = s;
	app.addSession(fork, false);
	s.effort = "xhigh";
	ex("model claude-opus-5-5");
	await until(() => app.focusedSession !== s, 5000, "the switch");
	const moved = app.focusedSession;
	assert.deepEqual([moved.name, moved.engine, moved.chosenModel, moved.effort], ["badge", "claude", "claude-opus-5-5", "xhigh"], "same name; Claude Code takes xhigh too");
	assert.equal(app.sessions.indexOf(moved), 0, "its place in the sidebar");
	assert.equal(app.sessions.length, 2);
	assert.equal(fork.forkOf, moved, "a fork of it follows it");
	assert.deepEqual(made[0], { engine: "claude", model: "claude-opus-5-5", effort: "xhigh", handoff: HANDOFF });
	assert.deepEqual(moved.items.slice(0, 3).map((i) => i.kind), ["user", "tool", "agent"], "the transcript stays");
	assert.equal(moved.items.at(-1).text, "switched from gpt-5.5 (GPT-5.5, Codex) to claude-opus-5-5 (Claude Opus 5.5, Claude Code); the next prompt carries the conversation over");
	assert.equal(app.message, "'badge' now runs claude-opus-5-5 (Claude Opus 5.5, Claude Code)");
	assert.match(plain(app.render(140).join("\n")), /Claude Code · claude-opus-5-5 \(Claude Opus 5\.5\) · xhigh/, "the winbar");
	assert.deepEqual(persisted.at(-1), { engine: "claude", model: "claude-opus-5-5", effort: "xhigh", handoff: HANDOFF }, "saved with the handoff still to send");

	["i", ..."and the tests", "\r"].forEach((k) => app.handleInput(k));
	await settled(moved);
	assert.equal(prompt(calls()[0]), `${HANDOFF}and the tests`, "the first prompt carries it");
	assert.ok(!calls()[0].includes("--resume"), "a conversation of its own on Claude Code");
	assert.equal(moved.items.filter((i) => i.kind === "user").at(-1).text, "and the tests", "the transcript shows what you typed");
	assert.equal(persisted.at(-1).handoff, undefined, "saved again once it went out");
	moved.submit("more");
	await settled(moved);
	assert.equal(prompt(calls()[1]), "more", "only the first");
	assert.deepEqual(calls()[1].slice(calls()[1].indexOf("--resume"), calls()[1].indexOf("--resume") + 2), ["--resume", "3b248a3e-43fd-4fd1-98b5-f36e06e02d85"], "then its new engine's own conversation");
	for (const x of app.sessions) x.dispose();
});

test("an effort the new engine doesn't take goes back to its default", async () => {
	const { app, s, ex } = setup("pi", "fake/test");
	s.effort = "minimal";
	ex("model claude-sonnet-5-5");
	await until(() => app.focusedSession !== s, 5000, "the switch");
	assert.equal(app.focusedSession.effort, undefined);
	assert.equal(app.focusedSession.items.at(-1).text, "switched from fake/test (pi) to claude-sonnet-5-5 (Claude Sonnet 5.5, Claude Code); the next prompt carries the conversation over");
	app.focusedSession.dispose();
});

test("during a turn the switch waits for it to end; the last pick wins, and a queued prompt carries the conversation", async () => {
	fs.rmSync(log, { force: true });
	const { app, s, ex, made } = setup("gemini", "pro");
	s.setState("running");
	ex("model gpt-5.5");
	await until(() => /after this turn/.test(app.message), 5000, "the message");
	assert.equal(app.message, "switches to gpt-5.5 (GPT-5.5, Codex) after this turn");
	ex("model claude-haiku-5-5");
	await tick();
	assert.equal(app.message, "switches to claude-haiku-5-5 (Claude Haiku 5.5, Claude Code) after this turn");
	assert.equal(app.focusedSession, s, "nothing changes mid-turn");
	assert.equal(made.length, 0);
	s.submit("then the docs");
	s.setState("idle");
	await until(() => app.focusedSession !== s, 5000, "the switch");
	const moved = app.focusedSession;
	assert.deepEqual([made.length, moved.engine, moved.chosenModel], [1, "claude", "claude-haiku-5-5"], "the last pick");
	await until(() => calls().length === 1, 5000, "the queued prompt");
	await settled(moved);
	assert.match(prompt(calls()[0]), /^You're continuing a conversation that ran on Gemini CLI \(pro\) in this same worktree\./);
	assert.ok(prompt(calls()[0]).endsWith("left them.\n\nthen the docs"));
	assert.equal(moved.queue.length, 0);
	moved.dispose();
});

test("a pick on the agent's own engine while a switch waits calls the switch off", async () => {
	const { app, s, ex } = setup("claude", "claude-opus-5-5");
	s.setState("running");
	ex("model gpt-5.5");
	await until(() => /after this turn/.test(app.message), 5000, "the message");
	ex("model claude-sonnet-5-5");
	await until(() => s.chosenModel === "claude-sonnet-5-5", 5000, "the model");
	s.setState("idle");
	await tick();
	assert.equal(app.focusedSession, s, "it stays on Claude Code");
	s.dispose();
});

test("refused: an engine that isn't ready, and a racer", async () => {
	const { app, s, ex, made } = setup("claude", "claude-opus-5-5");
	app.setEngines(ENGINES.map((e) => (e.id === "codex" ? { ...e, connected: false, detail: "not signed in", connect: "codex login" } : e)));
	ex("model gpt-5.5");
	await until(() => /codex/.test(app.message), 5000, "the refusal");
	assert.equal(app.message, "can't switch to Codex: not signed in · connect: codex login", ":checkhealth's words");
	app.setEngines(ENGINES.map((e) => (e.id === "codex" ? { ...e, installed: false, connected: false, detail: "not installed", install: "npm install -g @openai/codex" } : e)));
	ex("model gpt-5.5");
	await tick();
	assert.equal(app.message, "can't switch to Codex: not installed · install: npm install -g @openai/codex");
	app.setEngines(ENGINES.map((e) => ({ ...e })));
	app.races = [{ id: 1, task: "x", startedAt: 0, racers: [s], ran: new Set(), finishedAt: new Map(), announced: false }];
	ex("model gpt-5.5");
	await tick();
	assert.equal(app.message, "a racer keeps its engine; :new starts an agent on another one");
	assert.equal(app.focusedSession, s);
	assert.equal(made.length, 0, "nothing changed");
	assert.equal(s.chosenModel, "claude-opus-5-5");
	// a racer still changes its model on its own engine
	ex("model claude-sonnet-5-5");
	await until(() => s.chosenModel === "claude-sonnet-5-5", 5000, "the model");
	s.dispose();
});

test("the agent's own engine: :model works as before, with no switch", async () => {
	const { app, s, ex, made } = setup("claude", "claude-opus-5-5");
	ex("model claude-sonnet-5-5");
	await until(() => s.chosenModel === "claude-sonnet-5-5", 5000, "the model");
	assert.equal(app.focusedSession, s);
	assert.equal(app.message, "model for 'badge': Claude Sonnet 5.5 (from the next prompt)");
	assert.equal(s.items.length, 3, "no line in the transcript");
	ex("model my-own-model");
	await until(() => s.chosenModel === "my-own-model", 5000, "the model");
	assert.equal(made.length, 0, "a name no engine lists is tried on its own engine");
	s.dispose();
});

test("a handoff saved before a restart goes out with the next prompt", async () => {
	fs.rmSync(log, { force: true });
	const s = new CliSession("badge", "claude", "claude-opus-5-5", { cwd: tmp, bin: claude });
	s.handoff = HANDOFF;
	let sent = 0;
	s.onHandoffSent = () => sent++;
	s.send("go on");
	await settled(s);
	assert.equal(prompt(calls()[0]), `${HANDOFF}go on`);
	assert.equal(sent, 1);
	assert.equal(s.handoff, undefined);
	s.dispose();
});

test("to pi and back: pi gets the conversation in its first prompt, in a new conversation of its own", { timeout: 60000 }, async () => {
	const real = await realPi(tmp, [{ text: "on it" }]);
	const { app, s, ex } = setup();
	try {
		const dir = path.join(tmp, "pi-sessions");
		fs.mkdirSync(dir, { recursive: true });
		app.modelChoices = async (e) => (e === "pi" ? [{ value: "fake/test", label: "Test Model" }] : e === "codex" ? CODEX : e === "claude" ? modelChoices(e, { PATH: "" }) : []);
		app.switchSession = (old, to) =>
			to.engine === "pi" ? new PiSession(old.name, to.model, { cwd: tmp, env: real.env, sessionDir: dir, fresh: true }) : new CliSession(old.name, to.engine, to.model, { cwd: tmp, bin: claude });
		ex("model fake/test");
		await until(() => app.focusedSession !== s, 5000, "the switch");
		const pi = app.focusedSession;
		assert.equal(pi.engine, "pi");
		assert.equal(app.message, "'badge' now runs fake/test (Test Model, pi)");
		await pi.ready;
		pi.submit("carry on");
		await until(() => real.model.requests.length > 0 && pi.state === "idle", 20000, "pi's reply");
		const said = JSON.stringify(real.model.requests[0].messages);
		assert.ok(said.includes(JSON.stringify(`${HANDOFF}carry on`).slice(1, -1)), "the handoff, then the prompt");
		// and back to Claude Code: the conversation includes what pi said
		fs.rmSync(log, { force: true });
		ex("model claude-opus-5-5");
		await until(() => app.focusedSession !== pi, 5000, "the switch back");
		const back = app.focusedSession;
		back.submit("done?");
		await settled(back);
		assert.match(prompt(calls()[0]), /ran on pi \(fake\/test\)/);
		assert.match(prompt(calls()[0]), /<user>carry on<\/user>\n<assistant>on it<\/assistant>\n\nContinue from here/);
	} finally {
		for (const x of app.sessions) x.dispose();
		await real.close();
	}
});

test("Tab after :model offers exact ids with their names; :effort the levels the agent's engine takes", async () => {
	const index = new Map([
		["claude", [{ value: "claude-opus-5-5", label: "Claude Opus 5.5" }, { value: "claude-opus-5-1", label: "Claude Opus 5.1" }]],
		["codex", [{ value: "gpt-5.5", label: "GPT-5.5" }]],
	]);
	assert.deepEqual(completeCommandLine("model opus", { agents: [], themes: [], modelIndex: index, modelOwn: "codex" }), {
		base: "model ",
		items: ["claude-opus-5-5", "claude-opus-5-1"],
		hints: ["Claude Opus 5.5", "Claude Opus 5.1"],
	});
	assert.deepEqual(completeCommandLine("model gpt", { agents: [], themes: [], modelIndex: index, modelOwn: "claude" }).items, ["gpt-5.5"]);
	assert.deepEqual(completeCommandLine("effort h", { agents: [], themes: [], efforts: ["default", "low", "high"] }).items, ["high"]);

	const { app, s } = setup("claude", "claude-opus-5-5");
	[":", ..."model gpt-"].forEach((k) => app.handleInput(k));
	app.render(140);
	await until(() => {
		app.render(140);
		return /gpt-5\.5/.test(plain(app.render(140).join("\n")));
	}, 5000, "the lists");
	app.handleInput("\t");
	assert.match(plain(app.render(140).at(-1)), /:model gpt-5\.5/, "Tab puts in the exact id");
	app.handleInput("\x1b");
	[":", ..."effort x"].forEach((k) => app.handleInput(k));
	app.handleInput("\t");
	assert.match(plain(app.render(140).at(-1)), /:effort xhigh/);
	app.handleInput("\x1b");
	[":", ..."model claude-"].forEach((k) => app.handleInput(k));
	assert.match(plain(app.render(140).join("\n")), /claude-opus-5-5 {2}Claude Opus 5\.5/, "the wildmenu shows each name");
	s.dispose();
});
