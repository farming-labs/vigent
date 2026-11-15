/**
 * Moving an open agent to a model on another engine (`:model`). One engine
 * can't read another's session, so the conversation goes along as text, at
 * the front of the first prompt the new engine gets. The worktree stays, so
 * the files are already where the conversation left them.
 */
import { modelOwners } from "./race.js";
import type { TranscriptItem } from "./session.js";

/** how much of the conversation is carried over, in characters: the most recent part */
export const HANDOFF_CAP = 40_000;
/** the line before a carried conversation that was cut */
export const HANDOFF_CUT = "(earlier messages left out)";

/** A model in messages: "gpt-5.5 (GPT-5.5, Codex)", "my-model (Codex)", "the default model (Codex)". */
export function modelPhrase(engineName: string, model?: string, label?: string): string {
	if (!model) return `the default model (${engineName})`;
	return label && label !== model ? `${model} (${label}, ${engineName})` : `${model} (${engineName})`;
}

/** characters, not UTF-16 units: the Rust port counts the same way */
const chars = (s: string) => Array.from(s);

/**
 * The text that goes before the new engine's first prompt: the conversation
 * so far from the transcript (prompts, replies, a line per tool call), its
 * most recent HANDOFF_CAP characters. Empty when there's nothing to carry.
 */
export function handoffText(items: TranscriptItem[], engineName: string, model?: string): string {
	const lines = items.flatMap((i) =>
		i.kind === "user" ? [`<user>${i.text}</user>`] : i.kind === "agent" ? [`<assistant>${i.text}</assistant>`] : i.kind === "tool" && i.tool ? [`${i.tool.name}: ${i.tool.summary}`] : [],
	);
	if (!lines.length) return "";
	const kept: string[] = [];
	let size = 0;
	let cut = false;
	for (let k = lines.length - 1; k >= 0; k--) {
		const n = chars(lines[k]).length + (kept.length ? 1 : 0);
		if (size + n > HANDOFF_CAP) {
			// one message longer than all of it: its end
			if (!kept.length) kept.push(chars(lines[k]).slice(-HANDOFF_CAP).join(""));
			cut = true;
			break;
		}
		kept.unshift(lines[k]);
		size += n;
	}
	const body = `${cut ? `${HANDOFF_CUT}\n` : ""}${kept.join("\n")}`;
	return `You're continuing a conversation that ran on ${engineName} (${model ?? "the default model"}) in this same worktree. Here it is so far:\n\n${body}\n\nContinue from here. The files in this worktree are already as that conversation left them.\n\n`;
}

/** an engine's models, as its list names them */
export type ModelIndex = Map<string, { value: string; label?: string }[]>;

/** a model on an engine, with its readable name when the engine's list has one */
export interface ModelPick {
	engine: string;
	model: string;
	label?: string;
}

/**
 * Which engine runs `:model <id>`: the agent's own when its list has the id,
 * else the engine whose list has it (as `:race --models` resolves ids), a
 * ready one first. Undefined when no list has it: a name for the agent's own
 * engine, as before.
 */
export function resolveModel(id: string, own: string, index: ModelIndex, ready: (engine: string) => boolean = () => true): ModelPick | undefined {
	const label = (engine: string) => index.get(engine)?.find((c) => c.value === id)?.label;
	if (index.get(own)?.some((c) => c.value === id)) return { engine: own, model: id, label: label(own) };
	const ids = new Map([own, ...[...index.keys()].filter((e) => e !== own)].map((e) => [e, (index.get(e) ?? []).map((c) => c.value)]));
	const owners = modelOwners(id, ids);
	const engine = owners.includes(own) ? own : (owners.find(ready) ?? owners[0]);
	return engine ? { engine, model: id, label: label(engine) } : undefined;
}

/**
 * What Tab offers after `:model`: every exact id that starts with or contains
 * what's typed (any case), the agent's own engine's first, each with its name.
 */
export function modelCompletions(typed: string, own: string | undefined, index: ModelIndex): { id: string; hint?: string }[] {
	const needle = typed.toLowerCase();
	const engines = own && index.has(own) ? [own, ...[...index.keys()].filter((e) => e !== own)] : [...index.keys()];
	const out: { id: string; hint?: string }[] = [];
	const seen = new Set<string>();
	for (const e of engines) {
		for (const c of index.get(e) ?? []) {
			if (seen.has(c.value) || !(c.value.toLowerCase().includes(needle) || (c.label ?? "").toLowerCase().includes(needle))) continue;
			seen.add(c.value);
			out.push({ id: c.value, ...(c.label && c.label !== c.value ? { hint: c.label } : {}) });
		}
	}
	return out;
}
