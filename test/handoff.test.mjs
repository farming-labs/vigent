import assert from "node:assert/strict";
import { test } from "node:test";
import { HANDOFF_CAP, HANDOFF_CUT, handoffText, modelCompletions, modelPhrase, resolveModel } from "../dist/handoff.js";
import { resolveRaceModels } from "../dist/race.js";

const items = [
	{ kind: "user", text: "fix the badge" },
	{ kind: "info", text: "continuing the Claude Code conversation…" },
	{ kind: "agent", text: "Looking at it." },
	{ kind: "tool", text: "", tool: { name: "Bash", summary: "npm test", output: "ok", folded: true } },
	{ kind: "agent", text: "Fixed: it counts quantities now." },
];

test("the handoff: the conversation as text, between what to do with it", () => {
	assert.equal(
		handoffText(items, "Claude Code", "claude-opus-5-5"),
		"You're continuing a conversation that ran on Claude Code (claude-opus-5-5) in this same worktree. Here it is so far:\n\n" +
			"<user>fix the badge</user>\n<assistant>Looking at it.</assistant>\nBash: npm test\n<assistant>Fixed: it counts quantities now.</assistant>\n\n" +
			"Continue from here. The files in this worktree are already as that conversation left them.\n\n",
	);
	assert.match(handoffText(items, "Codex"), /ran on Codex \(the default model\) in this same worktree/);
	assert.equal(handoffText([{ kind: "info", text: "x" }], "Codex"), "", "nothing to carry");
});

test("the handoff keeps the most recent 40,000 characters, and says when it cut", () => {
	const long = [];
	for (let i = 0; i < 100; i++) long.push({ kind: "user", text: `${i} ${"é".repeat(995)}` });
	const text = handoffText(long, "pi", "anthropic/claude-opus-5-5");
	const body = text.split("\n\n")[1];
	assert.ok(body.startsWith(`${HANDOFF_CUT}\n<user>`), "a line says what's left out, then whole messages");
	const kept = body.split("\n").slice(1);
	assert.ok(kept.at(-1).startsWith("<user>99 "), "the most recent last");
	assert.ok(Array.from(kept.join("\n")).length <= HANDOFF_CAP);
	assert.ok(Array.from(kept.join("\n")).length > HANDOFF_CAP - 1100, "about the cap, counted in characters");
	// one message longer than the cap: its end
	const one = handoffText([{ kind: "agent", text: `start ${"x".repeat(50_000)} end` }], "Codex");
	const b = one.split("\n\n")[1];
	assert.equal(b.split("\n")[0], HANDOFF_CUT);
	assert.equal(Array.from(b.split("\n")[1]).length, HANDOFF_CAP);
	assert.ok(b.endsWith(" end</assistant>"));
	// just under: nothing is left out
	assert.ok(!handoffText([{ kind: "user", text: "y".repeat(HANDOFF_CAP - 13) }], "Codex").includes(HANDOFF_CUT));
});

test("models in messages: the exact id, its name and version, the engine", () => {
	assert.equal(modelPhrase("Codex", "gpt-5.5", "GPT-5.5"), "gpt-5.5 (GPT-5.5, Codex)");
	assert.equal(modelPhrase("Claude Code", "claude-opus-5-5", "Claude Opus 5.5"), "claude-opus-5-5 (Claude Opus 5.5, Claude Code)");
	assert.equal(modelPhrase("Gemini CLI", "my-model", "my-model"), "my-model (Gemini CLI)");
	assert.equal(modelPhrase("Codex"), "the default model (Codex)");
});

const index = new Map([
	["claude", [{ value: "claude-opus-5-5", label: "Claude Opus 5.5" }, { value: "claude-sonnet-5-5", label: "Claude Sonnet 5.5" }]],
	["codex", [{ value: "gpt-5.5", label: "GPT-5.5" }, { value: "gpt-5.5-codex", label: "GPT-5.5 Codex" }]],
	["opencode", [{ value: "anthropic/claude-opus-5-5", label: "Claude Opus 5.5" }]],
	["pi", [{ value: "anthropic/claude-opus-5-5", label: "Claude Opus 5.5" }, { value: "openai/gpt-5.5", label: "GPT-5.5" }]],
]);

test(":model <id>: which engine runs it, from the engines' own lists", () => {
	assert.deepEqual(resolveModel("gpt-5.5", "claude", index), { engine: "codex", model: "gpt-5.5", label: "GPT-5.5" }, "an exact id another engine lists");
	assert.deepEqual(resolveModel("claude-sonnet-5-5", "claude", index), { engine: "claude", model: "claude-sonnet-5-5", label: "Claude Sonnet 5.5" }, "the agent's own engine");
	assert.deepEqual(resolveModel("anthropic/claude-opus-5-5", "pi", index), { engine: "pi", model: "anthropic/claude-opus-5-5", label: "Claude Opus 5.5" }, "two list it: the agent's own");
	assert.equal(resolveModel("anthropic/claude-opus-5-5", "codex", index).engine, "opencode", "else the first in engine order");
	assert.equal(resolveModel("anthropic/claude-opus-5-5", "codex", index, (e) => e === "pi").engine, "pi", "a ready one first");
	assert.equal(resolveModel("opus", "codex", index), undefined, "no aliases, no partial words");
	assert.equal(resolveModel("my-own-model", "claude", index), undefined, "no list has it: a name for the agent's own engine");
	// :race resolves ids the same way
	assert.deepEqual(resolveRaceModels(["gpt-5.5", "claude-opus-5-5"], new Map([...index].map(([e, l]) => [e, l.map((c) => c.value)])), () => undefined).racers, [
		{ engine: "codex", model: "gpt-5.5" },
		{ engine: "claude", model: "claude-opus-5-5" },
	]);
});

test(":model completion: exact ids with their names, the agent's engine first", () => {
	assert.deepEqual(modelCompletions("opus", "pi", index), [
		{ id: "anthropic/claude-opus-5-5", hint: "Claude Opus 5.5" },
		{ id: "claude-opus-5-5", hint: "Claude Opus 5.5" },
	]);
	assert.deepEqual(modelCompletions("gpt-", "claude", index).map((c) => c.id), ["gpt-5.5", "gpt-5.5-codex", "openai/gpt-5.5"]);
	assert.deepEqual(modelCompletions("", undefined, index).length, 6);
});
