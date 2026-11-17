/**
 * pi, the coding agent, as vigent runs it: your own install, like Claude Code
 * or Codex. vigent bundles none of it; it starts `pi --mode rpc` per agent.
 */
import { execFile } from "node:child_process";
import { oneLine } from "./printable.js";
import { probeCwd } from "./workspace.js";

export const PI_BIN = "pi";
export const PI_INSTALL = "npm install -g @mariozechner/pi-coding-agent";

/**
 * The models pi can use now (`pi --list-models`): "provider/id". Empty when pi isn't installed.
 * pi loads the folder's .pi/ (its extensions run), so this runs outside a repository you haven't trusted.
 */
export function piModels(env: NodeJS.ProcessEnv = process.env, bin = PI_BIN): Promise<{ provider: string; id: string }[]> {
	return new Promise((resolve) => {
		execFile(bin, ["--list-models"], { cwd: probeCwd(), env: { ...env, PI_OFFLINE: "1" }, timeout: 20000, maxBuffer: 8 * 1024 * 1024 }, (err, stdout, stderr) => {
			if (err) return resolve([]);
			// pi prints the table on stderr (stdout stays clean for piping); take whichever has it
			resolve(parseModelList(`${stdout}\n${stderr}`));
		});
	});
}

/** `provider  model  context ...` rows, under a `provider  model` header; anything before the header is ignored. */
export function parseModelList(text: string): { provider: string; id: string }[] {
	const out: { provider: string; id: string }[] = [];
	let table = false;
	for (const line of text.split("\n")) {
		const [provider, id] = oneLine(line).split(" ");
		if (provider === "provider" && id === "model") {
			table = true;
			continue;
		}
		if (!table || !provider || !id) continue;
		out.push({ provider, id });
	}
	return out;
}
