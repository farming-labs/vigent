/**
 * Trust a temp repository for a test, as `:trust` / `vigent trust` would:
 * vigent starts no agent CLI in a repository you haven't trusted. The entry
 * goes in the trust store the vigent under test reads, given its environment
 * ($XDG_STATE_HOME, else $HOME/.local/state).
 */
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

/** Where `env`'s vigent keeps trusted.json. */
export function trustStore(env = process.env) {
	const state = env.XDG_STATE_HOME || path.join(env.HOME || os.homedir(), ".local", "state");
	return path.join(state, "vigent", "trusted.json");
}

/** Trust `dir` (a repository's root, or a folder outside one) for the vigent that runs with `env`. */
export function trustRepo(dir, env = process.env) {
	const file = trustStore(env);
	let all = {};
	try {
		all = JSON.parse(fs.readFileSync(file, "utf8"));
	} catch {
		// a fresh store
	}
	all[fs.realpathSync(dir)] = "repository";
	fs.mkdirSync(path.dirname(file), { recursive: true });
	fs.writeFileSync(file, `${JSON.stringify(all, null, 2)}\n`);
}
