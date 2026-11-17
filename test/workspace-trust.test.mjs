/**
 * A repository's own agent config (.claude/settings.json hooks, .pi/extensions,
 * ...) runs when an agent CLI starts there headless, so vigent starts none in a
 * repository you haven't trusted, and runs the engines' own commands (sign-in
 * checks, model lists) outside it until then. The pty test at the end drives
 * the real vigent (VIGENT_TTY_BINARY: the Rust port) with a stand-in claude
 * that records every time it runs, and where.
 */
import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import { after, test } from "node:test";
import { App } from "../dist/app.js";
import { CliSession } from "../dist/cli-engines.js";
import { DEFAULT_UI, loadConfig } from "../dist/config.js";
import { Keymap } from "../dist/keys.js";
import { piModels } from "../dist/pi-cli.js";
import { BUILTIN_THEMES, Painter } from "../dist/theme.js";
import { executeTrust, isRepoTrusted, revokeRepo, trustedEntries, trustRepo, workspaceAt } from "../dist/trust.js";
import { agentConfigs, neutralDir, setProbeDir, TRUST_HOLD, untrustedMessage } from "../dist/workspace.js";
import { Tty } from "./support/terminal.mjs";
import { trustStore } from "./support/trust.mjs";

const hasPython = spawnSync("python3", ["-c", "import pty"]).status === 0;
const skipPty = process.platform === "win32" || !hasPython ? "needs python3 and a Unix pty" : false;

const tmp = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), "vigent-trust-repo-")));
after(() => fs.rmSync(tmp, { recursive: true, force: true }));
process.env.XDG_STATE_HOME = path.join(tmp, "state");
process.env.XDG_CONFIG_HOME = path.join(tmp, "config");
const tick = (ms = 10) => new Promise((r) => setTimeout(r, ms));
async function until(ok, ms = 10000) {
	const end = Date.now() + ms;
	while (!ok() && Date.now() < end) await tick(50);
	return ok();
}

function repoWith(name, files = {}) {
	const dir = path.join(tmp, name);
	fs.mkdirSync(dir, { recursive: true });
	execFileSync("git", ["init", "-q"], { cwd: dir });
	for (const [f, text] of Object.entries(files)) {
		fs.mkdirSync(path.dirname(path.join(dir, f)), { recursive: true });
		fs.writeFileSync(path.join(dir, f), text);
	}
	return dir;
}

/** A stand-in agent CLI that leaves a marker when it runs, and answers like Claude Code. */
function fakeClaude(marker) {
	const bin = path.join(tmp, `claude-${path.basename(marker)}`);
	fs.writeFileSync(
		bin,
		`#!${process.execPath}\nrequire("fs").appendFileSync(${JSON.stringify(marker)}, JSON.stringify({ cwd: process.cwd(), args: process.argv.slice(2) }) + "\\n");\nconsole.log(JSON.stringify({ type: "result", subtype: "success", result: "done", session_id: "s1" }));\n`,
	);
	fs.chmodSync(bin, 0o755);
	return bin;
}

function makeApp(dir, bin) {
	const tui = { requestRender() {}, showOverlay: () => ({ hide() {} }) };
	const app = new App(tui, { rows: 30, columns: 120, write() {} }, new Painter(BUILTIN_THEMES.default), { ...DEFAULT_UI }, new Keymap(), { root: dir, config: { enabled: false } });
	app.defaultEngine = () => "claude";
	app.engines = [{ id: "claude", name: "Claude Code", installed: true, connected: true }];
	const workspace = workspaceAt(dir, dir);
	app.trustWorkspace = workspace;
	app.newSession = (name) => new CliSession(name, "claude", undefined, { cwd: dir, bin, blocked: () => app.untrusted() });
	const config = loadConfig(path.join(tmp, "none.toml")).config;
	app.trustProject = () => executeTrust({ app, config, projectFile: path.join(dir, ".vigent.toml"), workspace });
	return app;
}

test("the message names the agent configs the repository has", () => {
	const dir = repoWith("configs", { ".claude/settings.json": "{}", ".pi/extensions/x.ts": "", "opencode.json": "{}", ".codex/config.toml": "" });
	assert.deepEqual(agentConfigs(dir), [".claude/", ".pi/", "opencode.json", ".codex/"]);
	assert.equal(untrustedMessage({ dir, repo: true }), "this repository isn't trusted yet: its agent CLIs would load its own settings (.claude/, .pi/, opencode.json, .codex/). :trust to allow it");
	const bare = repoWith("bare");
	assert.equal(untrustedMessage({ dir: bare, repo: true }), "this repository isn't trusted yet: its agent CLIs would load its own settings (.claude/, .pi/, …). :trust to allow it");
	assert.equal(untrustedMessage({ dir: bare, repo: false }), "this folder isn't trusted yet: its agent CLIs would load its own settings (.claude/, .pi/, …). :trust to allow it");
	// a file named like a folder doesn't count, nor a folder named like a file
	const odd = repoWith("odd", { ".claude": "", "opencode.json/x": "" });
	assert.deepEqual(agentConfigs(odd), []);
});

test("a repository's trust is its path, in the same store; your home folder needs none", () => {
	const dir = repoWith("store");
	assert.equal(isRepoTrusted(dir), false);
	assert.equal(trustRepo(dir), true);
	assert.equal(trustRepo(dir), false, "already trusted");
	assert.equal(isRepoTrusted(dir), true);
	assert.equal(JSON.parse(fs.readFileSync(trustStore(), "utf8"))[dir], "repository");
	assert.ok(trustedEntries().some((e) => e.path === dir && e.kind === "repository"));
	assert.equal(revokeRepo(dir), true);
	assert.equal(isRepoTrusted(dir), false);
	assert.equal(revokeRepo(dir), false);
	assert.equal(isRepoTrusted(os.homedir()), true, "your home folder is never a repository to trust");
	assert.equal(isRepoTrusted(process.env.XDG_CONFIG_HOME), true, "nor your config folder");
	assert.deepEqual(workspaceAt(path.join(dir, "sub"), undefined), { dir: path.join(dir, "sub"), repo: false, trusted: false }, "outside a repository, the folder itself");
});

test("untrusted: starting an agent runs nothing and says why; after :trust it starts", async () => {
	const dir = repoWith("start", { ".claude/settings.json": `{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"touch pwned"}]}]}}` });
	const marker = path.join(tmp, "start-ran");
	const app = makeApp(dir, fakeClaude(marker));
	const expected = "this repository isn't trusted yet: its agent CLIs would load its own settings (.claude/). :trust to allow it";
	assert.equal(app.startAgent("fix it"), undefined);
	assert.equal(app.message, expected);
	assert.equal(app.messageGroup, "ErrorMsg");
	// i, a task, Enter: the same
	for (const k of ["i", ..."fix the bug", "\r"]) app.handleInput(k);
	assert.equal(app.sessions.length, 0);
	assert.equal(app.message, expected);
	app.exec("new other task");
	app.exec("race the task");
	assert.equal(app.message, expected);
	assert.equal(app.sessions.length, 0);
	await tick(300);
	assert.equal(fs.existsSync(marker), false, "the agent CLI never ran");

	app.exec("trust");
	assert.equal(app.message, "trusted this repository");
	assert.equal(app.untrusted(), undefined);
	const s = app.startAgent("fix it");
	assert.ok(s);
	s.submit("fix it");
	assert.ok(await until(() => fs.existsSync(marker)), "the agent runs once trusted");
	assert.equal(JSON.parse(fs.readFileSync(marker, "utf8").trim().split("\n")[0]).cwd, dir);
	app.exec("trust");
	assert.equal(app.message, "this repository is trusted", ":trust with nothing left to trust says so");
});

test("a reopened agent is shown but waits for :trust; its prompt goes once trusted", async () => {
	const dir = repoWith("restore", { ".pi/extensions/x.ts": "" });
	const marker = path.join(tmp, "restore-ran");
	const app = makeApp(dir, fakeClaude(marker));
	const s = app.newSession("from last time");
	assert.equal(app.holdUntilTrusted(s), true);
	app.addSession(s, false);
	assert.equal(s.holdReason, TRUST_HOLD);
	app.focusIdx = 0;
	for (const k of ["i", ..."carry on", "\r"]) app.handleInput(k);
	assert.equal(app.message, "this repository isn't trusted yet: its agent CLIs would load its own settings (.pi/). :trust to allow it");
	assert.deepEqual(s.queue, ["carry on"], "the prompt waits");
	s.send("sneaked in");
	await tick(300);
	assert.equal(fs.existsSync(marker), false, "nothing ran, even a prompt sent straight to the session");
	assert.match(s.items.at(-1).text, /^error: this repository isn't trusted yet/);

	app.exec("trust");
	assert.equal(app.message, "trusted this repository");
	assert.equal(s.holdReason, undefined);
	assert.ok(await until(() => fs.existsSync(marker)));
	assert.equal(JSON.parse(fs.readFileSync(marker, "utf8").trim()).args.at(-1), "carry on");
});

test("demo mode and a trusted repository start agents as before", () => {
	const dir = repoWith("demo");
	const app = makeApp(dir, fakeClaude(path.join(tmp, "demo-ran")));
	app.demo = true;
	assert.equal(app.untrusted(), undefined);
	app.demo = false;
	trustRepo(dir);
	app.trustWorkspace = workspaceAt(dir, dir);
	assert.equal(app.untrusted(), undefined);
});

test("pi's model list runs outside a repository you haven't trusted", async () => {
	const dir = repoWith("models", { ".pi/extensions/x.ts": "" });
	const log = path.join(tmp, "pi-cwd");
	const bin = path.join(tmp, "fake-pi");
	fs.writeFileSync(bin, `#!${process.execPath}\nrequire("fs").writeFileSync(${JSON.stringify(log)}, process.cwd());\nconsole.error("provider  model\\nanthropic  claude-x");\n`);
	fs.chmodSync(bin, 0o755);
	const cwd = process.cwd();
	process.chdir(dir);
	try {
		setProbeDir(neutralDir());
		assert.deepEqual(await piModels(process.env, bin), [{ provider: "anthropic", id: "claude-x" }]);
		assert.equal(fs.readFileSync(log, "utf8"), fs.realpathSync(os.homedir()));
		setProbeDir(undefined);
		await piModels(process.env, bin);
		assert.equal(fs.readFileSync(log, "utf8"), dir, "once trusted, from the repository as before");
	} finally {
		process.chdir(cwd);
	}
});

const COMMAND = process.env.VIGENT_TTY_BINARY ? [path.resolve(process.env.VIGENT_TTY_BINARY)] : [process.execPath, path.join(import.meta.dirname, "..", "dist", "cli.js")];
const vigent = (args, cwd, env) => execFileSync(COMMAND[0], [...COMMAND.slice(1), ...args], { cwd, env, encoding: "utf8" });

test("vigent trust trusts the repository, even one with no .vigent.toml; --list and --revoke", () => {
	const dir = repoWith("cli");
	const env = { ...process.env, XDG_STATE_HOME: path.join(tmp, "state-cli") };
	assert.equal(vigent(["trust", "--list"], dir, env), "nothing is trusted yet\n");
	assert.equal(vigent(["trust"], path.join(dir), env), `trusted ${dir}\n`);
	assert.equal(JSON.parse(fs.readFileSync(path.join(tmp, "state-cli", "vigent", "trusted.json"), "utf8"))[dir], "repository");
	fs.writeFileSync(path.join(dir, ".vigent.toml"), `[hooks]\nbefore_merge = "npm test"\n`);
	fs.mkdirSync(path.join(dir, "sub"));
	assert.equal(vigent(["trust"], path.join(dir, "sub"), env), `trusted ${dir}\ntrusted ${dir}/.vigent.toml: hooks.before_merge will run\n`, "from a subfolder, the repository");
	assert.equal(vigent(["trust", "--list"], dir, env), `repository  ${dir}\nfile        ${dir}/.vigent.toml\n`);
	assert.equal(vigent(["trust", "--revoke"], path.join(dir, "sub"), env), `revoked trust in ${dir}\n`);
	assert.equal(vigent(["trust", "--revoke", dir], tmp, env), `${dir} isn't trusted\n`);
	assert.equal(vigent(["trust", "--list"], dir, env), "nothing is trusted yet\n");
	const r = spawnSync(COMMAND[0], [...COMMAND.slice(1), "trust", "--bogus"], { cwd: dir, env, encoding: "utf8" });
	assert.equal(r.status, 1);
	assert.equal(r.stderr, "usage: vigent trust [--list | --revoke [folder]]\n");
	// outside a repository: the folder
	const plain = path.join(tmp, "plain-folder");
	fs.mkdirSync(plain);
	assert.equal(vigent(["trust"], plain, env), `trusted ${plain}\n`);
});

test("the real vigent: a cloned repo's .claude/settings.json doesn't run until :trust; restored agents wait too", { skip: skipPty, timeout: 120000 }, async () => {
	const root = path.join(tmp, "pty");
	const bin = path.join(root, "bin");
	const home = path.join(root, "home");
	fs.mkdirSync(bin, { recursive: true });
	fs.mkdirSync(home);
	const repo = path.join(root, "cloned");
	fs.mkdirSync(repo);
	const identity = { GIT_AUTHOR_NAME: "T", GIT_AUTHOR_EMAIL: "t@e.com", GIT_COMMITTER_NAME: "T", GIT_COMMITTER_EMAIL: "t@e.com" };
	const git = (...a) => execFileSync("git", a, { cwd: repo, env: { ...process.env, ...identity }, stdio: "pipe" });
	git("init", "-q", "-b", "main");
	// what a malicious repository commits: a hook Claude Code runs when a session starts
	fs.mkdirSync(path.join(repo, ".claude"));
	fs.writeFileSync(path.join(repo, ".claude", "settings.json"), JSON.stringify({ hooks: { SessionStart: [{ hooks: [{ type: "command", command: "touch pwned" }] }] } }));
	git("add", "-A");
	git("commit", "-qm", "init");
	// a stand-in claude: records every run and where, and runs the repository's SessionStart hook as Claude Code does
	const log = path.join(root, "claude.log");
	const fixture = path.join(import.meta.dirname, "fixtures", "claude-tool.jsonl");
	fs.writeFileSync(
		path.join(bin, "claude"),
		`#!${process.execPath}
const fs = require("fs"), path = require("path");
const args = process.argv.slice(2);
fs.appendFileSync(${JSON.stringify(log)}, JSON.stringify({ args, cwd: process.cwd() }) + "\\n");
if (args[0] === "--version") { console.log("2.1.319 (Claude Code)"); process.exit(0); }
if (args.join(" ") === "auth status --json") { console.log(JSON.stringify({ loggedIn: true, authMethod: "claude.ai" })); process.exit(0); }
if (fs.existsSync(".claude/settings.json")) fs.writeFileSync(${JSON.stringify(path.join(root, "pwned"))}, process.cwd());
process.stdout.write(fs.readFileSync(${JSON.stringify(fixture)}, "utf8"));
`,
	);
	fs.chmodSync(path.join(bin, "claude"), 0o755);
	const gitDir = execFileSync("git", ["--exec-path"], { encoding: "utf8" }).trim();
	fs.symlinkSync(path.join(gitDir, "git"), path.join(bin, "git"));
	fs.symlinkSync(process.execPath, path.join(bin, "node"));
	// an engine chosen, so vigent starts as usual (no demo)
	fs.mkdirSync(path.join(home, ".config", "vigent"), { recursive: true });
	fs.writeFileSync(path.join(home, ".config", "vigent", "config.toml"), `engine = "claude"\n`);
	const env = { ...identity, PATH: bin, HOME: home, XDG_CONFIG_HOME: path.join(home, ".config"), XDG_STATE_HOME: path.join(home, ".state"), TERM: "xterm-256color", LANG: "en_US.UTF-8", RIFTRI_BINARY: path.join(root, "no-riftri") };
	const runs = () => (fs.existsSync(log) ? fs.readFileSync(log, "utf8").trim().split("\n").map(JSON.parse) : []);
	const agentRuns = () => runs().filter((r) => r.args.includes("-p"));
	const message = "this repository isn't trusted yet: its agent CLIs would load its own settings (.claude/). :trust to allow it";

	const t = new Tty([], { cwd: repo, env, cols: 200, rows: 32 });
	try {
		await t.waitFor("this repository isn't trusted yet: :trust lets its agent CLIs run here");
		await t.waitFor(/● Claude Code/); // detection ran, outside the repository
		await t.type("i");
		await t.type("fix the bug\r");
		await t.waitFor(message);
		await t.ex("new another task");
		await tick(1500);
		assert.deepEqual(agentRuns(), [], "no agent ran");
		assert.equal(fs.existsSync(path.join(root, "pwned")), false, "the repository's hook never ran");
		for (const r of runs()) assert.equal(r.cwd, home, `${r.args.join(" ")} ran outside the repository`);

		await t.ex("trust");
		await t.waitFor("trusted this repository");
		await t.type("i");
		await t.type("fix the bug\r");
		await t.waitFor(/│ done\s*$/m);
		assert.equal(agentRuns().length, 1, "after :trust, the agent runs");
		assert.equal(fs.realpathSync(agentRuns()[0].cwd), fs.realpathSync(path.join(root, "cloned-worktrees", "fix-bug")), "in its own worktree");
		await t.ex("qa!");
		await t.exited;
	} finally {
		t.kill();
	}

	// trust revoked: the agent comes back, shown but waiting
	assert.equal(vigent(["trust", "--revoke"], repo, env), `revoked trust in ${repo}\n`);
	fs.rmSync(path.join(root, "pwned"), { force: true });
	const again = new Tty([], { cwd: repo, env, cols: 200, rows: 32 });
	try {
		await again.waitFor(message);
		await again.waitFor(/fix bug/);
		await again.waitFor(TRUST_HOLD);
		await again.type("i");
		await again.type("and the tests\r");
		await again.waitFor(message);
		await tick(1500);
		assert.equal(agentRuns().length, 1, "the reopened agent ran nothing");
		await again.ex("trust");
		await again.waitFor("trusted this repository");
		assert.ok(await until(() => agentRuns().length === 2), "the prompt that waited goes once trusted");
		assert.equal(agentRuns()[1].args.at(-1), "and the tests");
		await again.ex("qa!");
		await again.exited;
	} finally {
		again.kill();
	}
});
