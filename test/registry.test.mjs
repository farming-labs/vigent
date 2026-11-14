/**
 * The registry's entries (registry/<kind>/<name>/) and scripts/registry.mjs:
 * every entry loads with vigent's own code, a bad one is rejected with every
 * problem listed, and the index says what each entry runs.
 */
import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import { after, test } from "node:test";
import { buildIndex, checkEntry, INDEX_FILE, indexText, runsOf, shellWords } from "../scripts/registry.mjs";

const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "vigent-registry-test-"));
after(() => fs.rmSync(tmp, { recursive: true, force: true }));
const META = 'description = "a thing"\nauthor = "someone"\nlicense = "MIT"\ntags = ["x"]\n';

test("every entry in the repo loads, and the index is up to date", () => {
	const { problems, index } = buildIndex();
	assert.deepEqual(problems, []);
	assert.equal(fs.readFileSync(INDEX_FILE, "utf8"), indexText(index), "npm run registry writes it");
	const names = index.entries.map((e) => `${e.kind}/${e.name}`);
	for (const n of ["theme/harbor", "theme/ember", "template/fix-ci", "template/test-the-diff", "template/race-review", "keymap/leader-land", "engine/aider", "engine/amp", "engine/goose", "setup/pnpm-monorepo", "setup/cargo-workspace"]) {
		assert.ok(names.includes(n), `${n} is listed`);
	}
	assert.ok(!names.includes("theme/nord"), "built-in themes aren't entries");
	for (const e of index.entries) {
		assert.equal(e.author, "vigent");
		assert.equal(typeof e.file, "string");
		assert.equal(Boolean(e.colors), e.kind === "theme", "themes carry their preview's colors");
	}
	const harbor = index.entries.find((e) => e.name === "harbor");
	assert.equal(harbor.colors.modeBg, "#88c0d0", "the colors vigent paints, with the entry's own highlights");
});

test("npm run registry:check passes on the repo", () => {
	const out = execFileSync(process.execPath, [path.join(import.meta.dirname, "..", "scripts", "registry.mjs"), "--check"], { encoding: "utf8" });
	assert.match(out, /entries ok; website\/public\/registry\/index.json is up to date/);
});

test("what an entry runs comes from the file: hooks, shell commands, engine commands and Claude Code rules", () => {
	const index = buildIndex().index;
	const pnpm = index.entries.find((e) => e.name === "pnpm-monorepo");
	assert.deepEqual(pnpm.runs.filter((r) => r.command).map((r) => r.command), ["pnpm install --frozen-lockfile", "pnpm test", "pnpm test {args}"]);
	assert.deepEqual(pnpm.runs.filter((r) => r.allow).map((r) => r.allow), ["Bash(pnpm test:*)", "Bash(pnpm lint:*)"]);
	const aider = index.entries.find((e) => e.name === "aider");
	assert.deepEqual(aider.runs, [
		{ from: "engines.aider.command", command: "aider --yes-always --no-pretty --message {prompt}" },
		{ from: "engines.aider.model_args", command: "aider --yes-always --no-pretty --message {prompt} --model {model}" },
	]);
	for (const e of index.entries.filter((x) => ["theme", "template", "keymap"].includes(x.kind))) assert.deepEqual(e.runs, [], `${e.name} is data only`);

	const raw = { engines: { claude: { args: ["--permission-mode", "bypassPermissions"] }, mine: { command: ["mine", "-q"] } }, commands: { Land: ["!npm test", "pr"] } };
	assert.deepEqual(runsOf(raw, ["engines.claude.args", "engines.mine.command", "commands.Land"]), [
		{ from: "engines.claude.args", command: "claude --permission-mode bypassPermissions" },
		{ from: "engines.mine.command", command: "mine -q {prompt}" },
		{ from: "commands.Land", command: "npm test" },
	]);
	assert.equal(shellWords(["sh", "-c", "echo 'hi there'"]), `sh -c 'echo '\\''hi there'\\'''`);
});

test("a bad entry is rejected with every problem listed", () => {
	const problems = (kind, name, files) => checkEntry(kind, name, files).problems;
	assert.match(problems("plugin", "x", { "entry.toml": "", "meta.toml": META })[0], /unknown kind 'plugin'/);
	assert.ok(problems("theme", "Bad_Name", { "entry.toml": 'extends = "nord"\n', "meta.toml": META }).some((p) => /isn't a name/.test(p)));
	assert.ok(problems("theme", "nord", { "entry.toml": 'extends = "nord"\n', "meta.toml": META }).some((p) => /built-in theme's name/.test(p)));
	assert.ok(problems("theme", "x", { "entry.toml": 'extends = "nord"\n' }).some((p) => /meta.toml is missing/.test(p)));
	assert.ok(problems("theme", "x", { "entry.toml": 'extends = "nord"\n', "meta.toml": META, "extra.sh": "" }).some((p) => /nothing else/.test(p)));

	const meta = problems("template", "x", { "entry.toml": '[templates.x]\nprompt = "p"\n', "meta.toml": 'description = ""\nlicence = "MIT"\ntags = ["Bad Tag!"]\n' });
	for (const re of [/unknown key 'licence'/, /'author' is missing/, /'license' is missing/, /'description' should be a string/, /'tags' should be a list/]) assert.ok(meta.some((p) => re.test(p)), `${re} in ${meta.join("\n")}`);

	const theme = problems("theme", "x", {
		"entry.toml": 'extends = "nord"\ncolour = 1\n[highlights]\nNormal = { fg = "red", wavy = true }\n[style]\nsidebar = "boxes"\n',
		"meta.toml": META,
	});
	for (const re of [/unknown key 'colour'/, /fg = "red" should be "#rrggbb"/, /unknown attribute 'wavy'/, /'sidebar' should be/]) assert.ok(theme.some((p) => re.test(p)), `${re} in ${theme.join("\n")}`);
	assert.ok(problems("theme", "x", { "entry.toml": 'extends = "mine"\n', "meta.toml": META }).some((p) => /extends a built-in theme/.test(p)));
	assert.ok(problems("theme", "x", { "entry.toml": "extends = \n", "meta.toml": META }).some((p) => /entry.toml: /.test(p)), "a file that isn't TOML");

	const template = problems("template", "x", { "entry.toml": '[templates.x]\nprompt = "p"\nengin = "claude"\n[templates.y]\nprompt = "q"\n', "meta.toml": META });
	assert.ok(template.some((p) => /unknown option 'engin'/.test(p)), "vigent's own warning");
	assert.ok(template.some((p) => /defines one table, \[templates.x\]/.test(p)));

	const keymap = problems("keymap", "x", { "entry.toml": '[keys.normal]\n"<leader>z" = "session.nxt"\n"<leader>y" = ":command! X !rm -rf ~<CR>"\n', "meta.toml": META });
	assert.ok(keymap.some((p) => /unknown action .session.nxt./.test(p)), keymap.join("\n"));
	assert.ok(keymap.some((p) => /can't define commands/.test(p)));

	assert.ok(problems("engine", "x", { "entry.toml": '[engines.x]\nname = "X"\n', "meta.toml": META }).some((p) => /needs a command/.test(p)));
	assert.ok(problems("engine", "claude", { "entry.toml": '[engines.claude]\ncommand = ["claude"]\n', "meta.toml": META }).some((p) => /built-in engine/.test(p)));
	assert.ok(problems("setup", "x", { "entry.toml": '[hooks]\nworktree_made = "npm ci"\n', "meta.toml": META }).some((p) => /unknown hook 'worktree_made'/.test(p)));
	assert.deepEqual(problems("setup", "x", { "entry.toml": '[hooks]\nworktree_created = "npm ci"\n', "meta.toml": META }), []);
});

test("the script lists every problem in a registry folder and fails", () => {
	const dir = path.join(tmp, "registry");
	fs.mkdirSync(path.join(dir, "plugins", "x"), { recursive: true });
	fs.mkdirSync(path.join(dir, "theme", "ok"), { recursive: true });
	fs.writeFileSync(path.join(dir, "theme", "ok", "entry.toml"), 'extends = "nord"\n');
	fs.writeFileSync(path.join(dir, "theme", "ok", "meta.toml"), META);
	fs.mkdirSync(path.join(dir, "engine", "broken"), { recursive: true });
	fs.writeFileSync(path.join(dir, "engine", "broken", "entry.toml"), "[engines.broken]\ncommand = []\n");
	const { problems, index } = buildIndex(dir);
	assert.equal(problems.length, 3, problems.join("\n"));
	assert.match(problems.join("\n"), /unknown kind 'plugins'/);
	assert.match(problems.join("\n"), /engine\/broken\/meta.toml is missing/);
	assert.deepEqual(
		index.entries.map((e) => e.name),
		["ok"],
	);
	const r = spawnSync(process.execPath, [path.join(import.meta.dirname, "..", "scripts", "registry.mjs"), "--check"], { encoding: "utf8" });
	assert.equal(r.status, 0, "the repo's own registry still passes");
});
