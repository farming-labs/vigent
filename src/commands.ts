/**
 * Your own ex commands, like vim's :command:
 *
 *   [commands]
 *   Review = "new -t review {args}"     # an ex command (the ":" is optional)
 *   Land = ["pr", "kill"]               # several, one after another
 *   Test = "!npm test"                  # a shell command, in the agent's worktree
 *
 *   [commands.Fix]                      # a table, to say what it does
 *   run = "new -t fix {args}"           # (a string or a list, as above)
 *   description = "fix what you describe, from the fix template"
 *
 * Names start with a capital letter, as in vim, so they never shadow a built-in.
 * {args} (or vim's <args>) is what you type after the name.
 */

export type UserCommands = Record<string, string[]>;

/** What else vigent knows about a command: what it does, and that it waits for :trust. */
export interface CommandInfo {
	description?: string;
	/** a project's command with shell steps, before :trust (it isn't in UserCommands yet) */
	waitsForTrust?: boolean;
}

export type CommandInfos = Record<string, CommandInfo>;

export const COMMAND_NAME = /^[A-Z][A-Za-z0-9]*$/;

/** One step of a command: ":new x" → "new x"; "!npm test" stays a shell command. */
export function normalizeStep(step: string): string {
	return step.trim().replace(/^:+\s*/, "");
}

/** Whether running it starts a program on your machine (a shell step): from a project file, that waits for :trust. */
export function runsShell(steps: string[]): boolean {
	return steps.some((s) => normalizeStep(s).startsWith("!"));
}

/** Fill in what was typed after the name. */
export function fillArgs(step: string, args: string): string {
	return step.split("{args}").join(args).split("<args>").join(args);
}

/** Parse [commands]. Mistakes are warnings; shell commands from an untrusted file are gated. */
export function parseCommands(
	value: unknown,
	where: string,
	warnings: string[],
	gated: string[],
	mayRun: boolean,
	info: CommandInfos = {},
	have: UserCommands = {},
): UserCommands {
	const out: UserCommands = {};
	if (!value || typeof value !== "object" || Array.isArray(value)) {
		warnings.push(`${where}: [commands] should be a table, e.g. Test = "!npm test"`);
		return out;
	}
	const isSteps = (v: unknown): v is string | string[] => typeof v === "string" || (Array.isArray(v) && v.length > 0 && v.every((x) => typeof x === "string"));
	const shape = (name: string) => `${where} [commands]: '${name}' should be an ex command ("new -t review {args}"), a shell command ("!npm test") or a list of them`;
	for (const [name, v] of Object.entries(value)) {
		if (!COMMAND_NAME.test(name)) {
			warnings.push(`${where} [commands]: '${name}' should start with a capital letter (letters and digits), like vim's :command`);
			continue;
		}
		let run: unknown = v;
		let description: string | undefined;
		// [commands.Name]: run = ..., description = "..."
		if (v && typeof v === "object" && !Array.isArray(v)) {
			const t = v as Record<string, unknown>;
			run = t.run;
			for (const [k, x] of Object.entries(t)) {
				if (k === "run") continue;
				if (k === "description" && typeof x === "string") description = x.trim() || undefined;
				else if (k === "description") warnings.push(`${where} [commands.${name}]: 'description' should be a string`);
				else warnings.push(`${where} [commands.${name}]: unknown option '${k}' (run, description)`);
			}
			if (run === undefined) {
				warnings.push(`${where} [commands.${name}]: 'run' is missing: an ex command ("new -t review {args}"), a shell command ("!npm test") or a list of them`);
				continue;
			}
		}
		const steps = isSteps(run) ? (typeof run === "string" ? [run] : run) : undefined;
		if (!steps || steps.some((s) => !normalizeStep(s))) {
			warnings.push(shape(name));
			continue;
		}
		if (!mayRun && runsShell(steps)) {
			gated.push(`commands.${name}`);
			// it completes, and says why it doesn't run yet (unless one of yours by that name runs instead)
			if (!have[name]) info[name] = { ...(description ? { description } : {}), waitsForTrust: true };
			continue;
		}
		out[name] = steps.map(normalizeStep);
		if (description) info[name] = { description };
		else delete info[name];
	}
	return out;
}

/** What a command does, in a line: its description, else its steps. */
export function describeCommand(steps: string[], info?: CommandInfo): string {
	return info?.description ?? steps.join(" | ");
}

/** `Name = steps · description`, as :command shows it. */
export function showCommand(name: string, steps: string[], info?: CommandInfo): string {
	return `${name} = ${steps.join(" | ")}${info?.description ? ` · ${info.description}` : ""}`;
}

/** What Tab offers for your commands: each name with its hint (its description; "(waits for :trust)" for a project's shell command before :trust). */
export function commandCompletions(commands: UserCommands, info: CommandInfos): { name: string; hint: string }[] {
	const out = Object.keys(commands).map((name) => ({ name, hint: info[name]?.description ?? "" }));
	for (const [name, i] of Object.entries(info)) {
		if (i.waitsForTrust && !commands[name]) out.push({ name, hint: [i.description, "(waits for :trust)"].filter(Boolean).join(" ") });
	}
	return out;
}
