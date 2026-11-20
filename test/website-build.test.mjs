import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

test("Vercel skips a website build only when its inputs match the previous deployment", (t) => {
	const repo = mkdtempSync(join(tmpdir(), "vigent-website-build-"));
	t.after(() => rmSync(repo, { recursive: true, force: true }));
	const { ignoreCommand } = JSON.parse(readFileSync(new URL("../website/vercel.json", import.meta.url), "utf8"));
	const git = (...args) => execFileSync("git", args, { cwd: repo, encoding: "utf8" }).trim();
	function commit(file, content) {
		writeFileSync(join(repo, file), content);
		git("add", "--", file);
		git(
			"-c",
			"user.name=Test",
			"-c",
			"user.email=test@example.com",
			"-c",
			"commit.gpgsign=false",
			"commit",
			"-qm",
			file,
		);
		return git("rev-parse", "HEAD");
	}
	function skips(previous) {
		// Git provides a POSIX shell for the configured Vercel command on Windows too.
		const result = spawnSync("git", ["-c", `alias.check-build=!cd website && ${ignoreCommand}`, "check-build"], {
			cwd: repo,
			env: { ...process.env, VERCEL_GIT_PREVIOUS_SHA: previous, VERCEL_GIT_COMMIT_SHA: git("rev-parse", "HEAD") },
		});
		assert.ifError(result.error);
		assert.notEqual(result.status, null);
		return result.status === 0;
	}
	git("init", "-q");
	mkdirSync(join(repo, "website"));
	const first = commit("website/page.tsx", "home");
	commit("cli.ts", "cli only");
	assert.equal(skips(first), true, "a CLI-only change skips");
	assert.equal(skips(""), false, "a first deploy builds");
	assert.equal(skips("0123456789abcdef0123456789abcdef01234567"), false, "a missing base builds");
	assert.equal(skips(git("rev-parse", "HEAD")), false, "an explicit redeploy builds");

	for (const input of ["website/page.tsx", "install.sh", "install.ps1"]) {
		const previous = git("rev-parse", "HEAD");
		commit(input, "changed");
		commit("cli.ts", input);
		assert.equal(skips(previous), false, `a change to ${input} earlier in a push still builds`);
	}
});
