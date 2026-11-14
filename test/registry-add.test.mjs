/**
 * `vigent add`, `vigent add --list` and `vigent search`, with VIGENT_REGISTRY
 * pointing at the repo's own index (website/public/registry/index.json) or at
 * one made for a test. They run Node and dist/, or the standalone binary when
 * VIGENT_BINARY is set, so the Rust port is held to the same behavior:
 *
 *   VIGENT_BINARY=dist-bin/vigent-linux-x64/vigent node --test test/registry-add.test.mjs
 */
import assert from "node:assert/strict";
import { execFileSync, spawn, spawnSync } from "node:child_process";
import * as fs from "node:fs";
import * as http from "node:http";
import * as os from "node:os";
import * as path from "node:path";
import { after, test } from "node:test";

const ROOT = path.join(import.meta.dirname, "..");
const INDEX = path.join(ROOT, "website", "public", "registry", "index.json");
const COMMAND = process.env.VIGENT_BINARY ? [path.resolve(process.env.VIGENT_BINARY)] : [process.execPath, path.join(ROOT, "dist", "cli.js")];
const tmp = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), "vigent-add-")));
after(() => fs.rmSync(tmp, { recursive: true, force: true }));

let n = 0;
/** A fresh home (and config folder) for each test. */
function home() {
	const dir = path.join(tmp, `h${n++}`);
	fs.mkdirSync(dir, { recursive: true });
	return { dir, config: path.join(dir, "xdg", "vigent"), env: { HOME: dir, XDG_CONFIG_HOME: path.join(dir, "xdg") } };
}

/** Run vigent: its output, error output and exit code. */
function vigent(args, { env = {}, input = "", cwd = tmp } = {}) {
	const r = spawnSync(COMMAND[0], [...COMMAND.slice(1), ...args], {
		cwd,
		input,
		encoding: "utf8",
		// no proxy: the test's own server is on this machine
		env: { PATH: process.env.PATH, LANG: "en_US.UTF-8", VIGENT_REGISTRY: INDEX, ...env },
	});
	return { out: r.stdout, err: r.stderr, code: r.status };
}

const index = JSON.parse(fs.readFileSync(INDEX, "utf8"));

test("--list shows every entry by kind, or one kind", () => {
	const r = vigent(["add", "--list"]);
	assert.equal(r.code, 0, r.err);
	for (const k of ["themes", "templates", "keymaps", "engines", "setups"]) assert.match(r.out, new RegExp(`^${k}$`, "m"));
	for (const e of index.entries) assert.match(r.out, new RegExp(`^  ${e.name} +${e.description.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}$`, "m"));
	const engines = vigent(["add", "--list", "engines"]);
	assert.match(engines.out, /^engines\n {2}aider +/);
	assert.doesNotMatch(engines.out, /themes|harbor/);
	assert.match(vigent(["add", "--list", "plugins"]).err, /unknown kind 'plugins' \(theme, template, keymap, engine, setup\)/);
});

test("search finds entries with every word", () => {
	const r = vigent(["search", "pnpm"]);
	assert.equal(r.code, 0, r.err);
	assert.match(r.out, /^ {2}setup\/pnpm-monorepo +installs each new worktree/m);
	assert.match(vigent(["search", "GRUVBOX", "orange"]).out, /theme\/ember/);
	const none = vigent(["search", "gruvbox", "rust"]);
	assert.equal(none.code, 1);
	assert.match(none.err, /nothing in the registry matches 'gruvbox rust'/);
	assert.match(vigent(["search"]).err, /usage: vigent search <words>/);
});

test("a theme: shows the file, writes it to the themes folder, and says :colorscheme", () => {
	const h = home();
	const r = vigent(["add", "harbor", "--yes"], { env: h.env });
	assert.equal(r.code, 0, r.err);
	const file = path.join(h.config, "themes", "harbor.toml");
	assert.match(r.out, /^theme\/harbor by vigent \(MIT\)\nnord with a card per agent/);
	assert.ok(r.out.includes(`It writes ${file}:\n\n    # nord, with a card per agent`));
	assert.match(r.out, /use it with :colorscheme harbor/);
	assert.doesNotMatch(r.out, /Add it\?/, "--yes doesn't ask");
	assert.equal(fs.readFileSync(file, "utf8"), index.entries.find((e) => e.name === "harbor").file);
	assert.match(vigent(["theme", "list"], { env: h.env }).out, /^harbor$/m, "vigent finds it");

	const again = vigent(["add", "harbor", "--yes"], { env: h.env });
	assert.equal(again.code, 1);
	assert.ok(again.err.includes(`${file} already exists (--force replaces it)`));
	fs.writeFileSync(file, "# mine\n");
	assert.equal(vigent(["add", "harbor", "--yes", "--force"], { env: h.env }).code, 0);
	assert.match(fs.readFileSync(file, "utf8"), /extends = "nord"/);
});

test("without XDG_CONFIG_HOME, the config folder is ~/.config/vigent", () => {
	const h = home();
	const r = vigent(["add", "ember", "-y"], { env: { HOME: h.dir } });
	assert.equal(r.code, 0, r.err);
	assert.ok(fs.existsSync(path.join(h.dir, ".config", "vigent", "themes", "ember.toml")));
});

test("a built-in theme, an unknown name, an unknown kind", () => {
	const builtin = vigent(["add", "nord"]);
	assert.equal(builtin.code, 1);
	assert.match(builtin.err, /'nord' is a built-in theme: nothing to add, use :colorscheme nord/);
	assert.match(vigent(["add", "theme/gruvbox"]).err, /use :colorscheme gruvbox/);
	const typo = vigent(["add", "harbr"]);
	assert.equal(typo.code, 1);
	assert.match(typo.err, /no 'harbr' in the registry \(did you mean harbor\?\)/);
	assert.match(vigent(["add", "pnpm"]).err, /did you mean pnpm-monorepo\?/);
	assert.match(vigent(["add", "zzzzzzzz"]).err, /no 'zzzzzzzz' in the registry \(vigent add --list shows them all\)/);
	assert.match(vigent(["add", "engine/harbor"]).err, /no engine 'harbor' in the registry/);
	assert.match(vigent(["add", "plugin/x"]).err, /unknown kind 'plugin'/);
	assert.match(vigent(["add"]).err, /usage: vigent add <name>/);
	assert.match(vigent(["add", "x", "--bogus"]).err, /unknown option '--bogus'/);
});

test("a name two kinds share needs its kind", () => {
	const h = home();
	const file = path.join(h.dir, "index.json");
	const theme = index.entries.find((e) => e.name === "ember");
	const template = index.entries.find((e) => e.name === "fix-ci");
	fs.writeFileSync(file, JSON.stringify({ version: 1, entries: [{ ...theme, name: "both" }, { ...template, name: "both", file: template.file.replace("fix-ci", "both") }] }));
	const r = vigent(["add", "both"], { env: { ...h.env, VIGENT_REGISTRY: file } });
	assert.equal(r.code, 1);
	assert.match(r.err, /'both' is a theme and a template: vigent add theme\/both or vigent add template\/both/);
	const t = vigent(["add", "template/both", "-y"], { env: { ...h.env, VIGENT_REGISTRY: h.dir } });
	assert.equal(t.code, 0, t.err);
	assert.match(fs.readFileSync(path.join(h.config, "config.toml"), "utf8"), /^\[templates\.both\]$/m);
});

test("a template goes into config.toml beside what's there; a second time it says what's already there", () => {
	const h = home();
	assert.equal(vigent(["config", "init"], { env: h.env }).code, 0);
	const file = path.join(h.config, "config.toml");
	const before = fs.readFileSync(file, "utf8");
	const r = vigent(["add", "fix-ci", "-y"], { env: h.env });
	assert.equal(r.code, 0, r.err);
	assert.ok(r.out.includes(`It adds to ${file}:`));
	assert.ok(r.out.includes(`added [templates.fix-ci] to ${file}\nuse it with :new -t fix-ci <task>`));
	const after = fs.readFileSync(file, "utf8");
	assert.ok(after.startsWith(before), "everything that was there stays, comments and order too");
	assert.ok(after.endsWith(`\n\n${index.entries.find((e) => e.name === "fix-ci").file}`));
	assert.match(vigent(["config", "check"], { env: h.env }).out, /: ok ·/);

	const again = vigent(["add", "fix-ci", "-y"], { env: h.env });
	assert.equal(again.code, 1);
	assert.ok(again.err.includes(`${file} already has [templates.fix-ci] (--force replaces it)`));
	const forced = vigent(["add", "fix-ci", "-y", "--force"], { env: h.env });
	assert.equal(forced.code, 0, forced.err);
	assert.match(forced.out, /It replaces \[templates\.fix-ci\]/);
	assert.equal(fs.readFileSync(file, "utf8"), after, "replaced by the same: no second copy");
});

test("a keymap's mappings go into the [keys.normal] that's there; one you mapped yourself stops it", () => {
	const h = home();
	assert.equal(vigent(["config", "init"], { env: h.env }).code, 0);
	const file = path.join(h.config, "config.toml");
	const r = vigent(["add", "leader-land", "-y"], { env: h.env });
	assert.equal(r.code, 0, r.err);
	assert.match(r.out, /added 7 mappings to /);
	const text = fs.readFileSync(file, "utf8");
	assert.equal(text.match(/^\[keys\.normal\]$/gm).length, 1, "the table that was there");
	const normal = text.slice(text.indexOf("[keys.normal]"), text.indexOf("[keys.insert]"));
	assert.match(normal, /"<S-Tab>" = "session\.prev"\n[^]*# <leader>a, then one key[^\n]*\n"<leader>an" = ":new "\n[^]*"<leader>ak" = ":kill<CR>"\n\n$/, "at the end of that table");
	const check = vigent(["config", "check"], { env: h.env });
	assert.match(check.out, /: ok · .* 13 mapping\(s\)/, check.out);

	const mine = home();
	fs.mkdirSync(mine.config, { recursive: true });
	fs.writeFileSync(path.join(mine.config, "config.toml"), '# my keys\n[keys]\n"<leader>an" = ":new auth "\n');
	const refused = vigent(["add", "leader-land", "-y"], { env: mine.env });
	assert.equal(refused.code, 1);
	assert.match(refused.err, /already has \[keys\.normal\] "<leader>an" \(--force replaces it\)/);
	assert.equal(vigent(["add", "leader-land", "-y", "--force"], { env: mine.env }).code, 0);
	const forced = fs.readFileSync(path.join(mine.config, "config.toml"), "utf8");
	assert.match(forced, /^# my keys\n\[keys\]\n\n/);
	assert.doesNotMatch(forced, /:new auth/);
	assert.match(forced, /"<leader>an" = ":new "/);
});

test("an engine: lists what it runs and asks; no is no", () => {
	const h = home();
	const no = vigent(["add", "aider"], { env: h.env, input: "n\n" });
	assert.equal(no.code, 1);
	assert.match(no.out, /It runs:\n {4}\$ aider --yes-always --no-pretty --message \{prompt\} {4}# engines\.aider\.command\n/);
	assert.match(no.out, /Add it\? \[y\/N\] \nnothing added\n$/);
	assert.ok(!fs.existsSync(path.join(h.config, "config.toml")), "nothing written");
	const eof = vigent(["add", "aider"], { env: h.env, input: "" });
	assert.equal(eof.code, 1, "no answer is no");

	const yes = vigent(["add", "aider"], { env: h.env, input: "y\n" });
	assert.equal(yes.code, 0, yes.err);
	assert.match(yes.out, /use it with :new <task> --engine aider/);
	assert.equal(fs.readFileSync(path.join(h.config, "config.toml"), "utf8"), index.entries.find((e) => e.name === "aider").file, "a new config.toml is the entry");
	assert.match(vigent(["config", "check"], { env: h.env }).out, /: ok ·/);
});

test("a setup: the repository's .vigent.toml, its commands and Claude Code rules listed, waiting for :trust", () => {
	const h = home();
	const outside = vigent(["add", "pnpm-monorepo", "-y"], { env: h.env, cwd: h.dir });
	assert.equal(outside.code, 1);
	assert.match(outside.err, /not inside a git repository/);

	const repo = path.join(h.dir, "shop");
	fs.mkdirSync(path.join(repo, "src"), { recursive: true });
	execFileSync("git", ["init", "-q"], { cwd: repo });
	const r = vigent(["add", "pnpm-monorepo"], { env: h.env, cwd: path.join(repo, "src"), input: "yes\n" });
	assert.equal(r.code, 0, r.err);
	const file = path.join(repo, ".vigent.toml");
	assert.ok(r.out.includes(`It writes ${file}:`));
	assert.match(r.out, /It runs:\n {4}\$ pnpm install --frozen-lockfile {4}# hooks\.worktree_created\n {4}\$ pnpm test {4}# hooks\.before_merge\n {4}\$ pnpm test \{args\} {4}# commands\.Test\n/);
	assert.match(r.out, /It lets Claude Code run, without asking:\n {4}Bash\(pnpm test:\*\) {4}# engines\.claude\.allow\n {4}Bash\(pnpm lint:\*\) {4}# engines\.claude\.allow\n/);
	assert.match(r.out, /nothing in it runs until you trust it: :trust in vigent, or vigent trust/);
	assert.equal(fs.readFileSync(file, "utf8"), index.entries.find((e) => e.name === "pnpm-monorepo").file);

	const again = vigent(["add", "cargo-workspace", "-y"], { env: h.env, cwd: repo });
	assert.equal(again.code, 1);
	assert.ok(again.err.includes(`${file} already exists (--force replaces it)`));
	assert.equal(vigent(["add", "cargo-workspace", "-y", "--force"], { env: h.env, cwd: repo }).code, 0);
	assert.match(fs.readFileSync(file, "utf8"), /cargo fetch --locked/);
});

test("the index over HTTP, and what happens when it can't be had", async () => {
	const body = fs.readFileSync(INDEX, "utf8");
	const server = http.createServer((req, res) => {
		if (req.url === "/registry/index.json") res.end(body);
		else if (req.url === "/new.json") res.end(JSON.stringify({ version: 9, entries: [] }));
		else {
			res.statusCode = 404;
			res.end("no");
		}
	});
	await new Promise((r) => server.listen(0, "127.0.0.1", r));
	const base = `http://127.0.0.1:${server.address().port}`;
	// spawnSync would block the server, so these run async
	const run = (args, env) =>
		new Promise((resolve) => {
			const child = spawn(COMMAND[0], [...COMMAND.slice(1), ...args], { cwd: tmp, env: { PATH: process.env.PATH, ...env } });
			let out = "";
			let err = "";
			child.stdout.on("data", (d) => (out += d));
			child.stderr.on("data", (d) => (err += d));
			child.on("close", (code) => resolve({ out, err, code }));
		});
	try {
		const h = home();
		const ok = await run(["add", "goose", "-y"], { ...h.env, VIGENT_REGISTRY: `${base}/registry/index.json` });
		assert.equal(ok.code, 0, ok.err);
		assert.match(fs.readFileSync(path.join(h.config, "config.toml"), "utf8"), /\[engines\.goose\]/);
		const missing = await run(["add", "goose"], { ...h.env, VIGENT_REGISTRY: `${base}/nothing.json` });
		assert.equal(missing.code, 1);
		assert.match(missing.err, new RegExp(`couldn't fetch the registry from ${base}/nothing\\.json: `));
		const newer = await run(["add", "goose"], { ...h.env, VIGENT_REGISTRY: `${base}/new.json` });
		assert.match(newer.err, /is newer than this vigent \(format 9\): update vigent/);
	} finally {
		await new Promise((r) => server.close(r));
	}
	// nothing listening there now
	const down = vigent(["search", "x"], { env: { VIGENT_REGISTRY: `${base}/registry/index.json` } });
	assert.equal(down.code, 1);
	assert.match(down.err, new RegExp(`^vigent: couldn't fetch the registry from ${base}/registry/index\\.json: .+`));
	const gone = vigent(["add", "goose"], { env: { VIGENT_REGISTRY: path.join(tmp, "no-such-index.json") } });
	assert.match(gone.err, /couldn't read the registry at .*no-such-index\.json: no such file/);
});

test("mergeToml: statements over several lines, keys written any way, and a file it can't add to", async () => {
	const { mergeToml, tomlStatements } = await import("../dist/config-edit.js");
	const text = 'theme = "nord"\nlist = [\n  "a", # [not.a.table]\n  "b",\n]\nnote = """\n[also.not]\n"""\n\n[ui]\nnumber = false\n';
	assert.deepEqual(
		tomlStatements(text).map((s) => [s.kind, s.start, s.end, s.path.join(".")]),
		[
			["kv", 0, 1, "theme"],
			["kv", 1, 5, "list"],
			["kv", 5, 8, "note"],
			["other", 8, 9, ""],
			["header", 9, 10, "ui"],
			["kv", 10, 11, "number"],
		],
	);
	const unit = { name: "[templates.x]", paths: [["templates", "x"]] };
	const entry = '[templates.x]\nprompt = """\nhi\n"""\n';
	const added = mergeToml(text, entry, [unit]);
	assert.equal(added.text, `${text}\n${entry}`);

	const dotted = 'templates.x.prompt = "mine"\n\n[ui]\nnumber = false\n';
	assert.deepEqual(mergeToml(dotted, entry, [unit]), { conflicts: ["[templates.x]"] });
	assert.equal(mergeToml(dotted, entry, [unit], true).text, `[ui]\nnumber = false\n\n${entry}`);

	const inline = 'keys = { normal = { "x" = "gg" } }\n';
	const r = mergeToml(inline, '[keys.normal]\n"y" = "G"\n', [{ name: '[keys.normal] "y"', paths: [["keys", "normal", "y"]] }]);
	assert.ok(r.error, "it won't write a file that reads differently");
	assert.ok(mergeToml("theme = \n", entry, [unit]).error, "a config that doesn't parse");
});
