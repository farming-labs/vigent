//! Moving an open agent to a model on another engine (`:model`; src/handoff.ts).
//! One engine can't read another's session, so the conversation goes along
//! as text, at the front of the first prompt the new engine gets. The
//! worktree stays, so the files are already where the conversation left them.

use super::choices::Choice;
use crate::session::Item;

/// How much of the conversation is carried over, in characters: the most recent part.
pub const HANDOFF_CAP: usize = 40_000;
/// The line before a carried conversation that was cut.
pub const HANDOFF_CUT: &str = "(earlier messages left out)";

/// Each engine's models, as its list names them.
pub type ModelIndex = Vec<(String, Vec<Choice>)>;

/// A model on an engine, with its readable name when the engine's list has one.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelPick {
    pub engine: String,
    pub model: String,
    pub label: Option<String>,
}

/// A model in messages: "gpt-5.5 (GPT-5.5, Codex)", "my-model (Codex)", "the default model (Codex)".
pub fn model_phrase(engine_name: &str, model: Option<&str>, label: Option<&str>) -> String {
    match (model, label) {
        (None, _) => format!("the default model ({engine_name})"),
        (Some(m), Some(l)) if l != m => format!("{m} ({l}, {engine_name})"),
        (Some(m), _) => format!("{m} ({engine_name})"),
    }
}

/// The text that goes before the new engine's first prompt: the conversation
/// so far from the transcript (prompts, replies, a line per tool call), its
/// most recent HANDOFF_CAP characters. Empty when there's nothing to carry.
pub fn handoff_text(items: &[Item], engine_name: &str, model: Option<&str>) -> String {
    let lines: Vec<String> = items
        .iter()
        .filter_map(|i| match i {
            Item::User(t) => Some(format!("<user>{t}</user>")),
            Item::Text(t) => Some(format!("<assistant>{t}</assistant>")),
            Item::Tool { name, summary, .. } => Some(format!("{name}: {summary}")),
            _ => None,
        })
        .collect();
    if lines.is_empty() {
        return String::new();
    }
    let mut kept: Vec<String> = vec![];
    let (mut size, mut cut) = (0, false);
    for line in lines.iter().rev() {
        let len = line.chars().count();
        let n = len + usize::from(!kept.is_empty());
        if size + n > HANDOFF_CAP {
            // one message longer than all of it: its end
            if kept.is_empty() {
                kept.push(line.chars().skip(len - HANDOFF_CAP).collect());
            }
            cut = true;
            break;
        }
        kept.push(line.clone());
        size += n;
    }
    kept.reverse();
    let body = format!("{}{}", if cut { format!("{HANDOFF_CUT}\n") } else { String::new() }, kept.join("\n"));
    format!(
        "You're continuing a conversation that ran on {engine_name} ({}) in this same worktree. Here it is so far:\n\n{body}\n\nContinue from here. The files in this worktree are already as that conversation left them.\n\n",
        model.unwrap_or("the default model")
    )
}

fn label_of(index: &ModelIndex, engine: &str, id: &str) -> Option<String> {
    index.iter().find(|(e, _)| e == engine)?.1.iter().find(|c| c.value == id)?.label.clone()
}

/// Which engine runs `:model <id>`: the agent's own when its list has the id,
/// else the engine whose list has it (as `:race --models` resolves ids), a
/// ready one first. None when no list has it: a name for the agent's own
/// engine, as before.
pub fn resolve_model(id: &str, own: &str, index: &ModelIndex, ready: &dyn Fn(&str) -> bool) -> Option<ModelPick> {
    let pick = |engine: &str| Some(ModelPick { engine: engine.to_string(), model: id.to_string(), label: label_of(index, engine, id) });
    if index.iter().any(|(e, l)| e == own && l.iter().any(|c| c.value == id)) {
        return pick(own);
    }
    let mut ids: Vec<(String, Vec<String>)> = vec![(own.to_string(), vec![])];
    for (e, l) in index {
        let list = l.iter().map(|c| c.value.clone()).collect();
        if e == own { ids[0].1 = list } else { ids.push((e.clone(), list)) }
    }
    let owners = crate::race::model_owners(id, &ids);
    let engine = if owners.contains(&own) { Some(own) } else { owners.iter().copied().find(|e| ready(e)).or(owners.first().copied()) };
    pick(engine?)
}

/// What Tab offers after `:model`: every exact id that starts with or contains
/// what's typed (any case), the agent's own engine's first, each with its name.
pub fn model_completions(typed: &str, own: Option<&str>, index: &ModelIndex) -> Vec<(String, Option<String>)> {
    let needle = typed.to_lowercase();
    let mut order: Vec<&(String, Vec<Choice>)> = index.iter().filter(|(e, _)| Some(e.as_str()) == own).collect();
    order.extend(index.iter().filter(|(e, _)| Some(e.as_str()) != own));
    let mut out: Vec<(String, Option<String>)> = vec![];
    for (_, list) in order {
        for c in list {
            let name = c.label.clone().unwrap_or_default();
            if out.iter().any(|(id, _)| *id == c.value) || !(c.value.to_lowercase().contains(&needle) || name.to_lowercase().contains(&needle)) {
                continue;
            }
            out.push((c.value.clone(), c.label.clone().filter(|l| *l != c.value)));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items() -> Vec<Item> {
        vec![
            Item::User("fix the badge".into()),
            Item::Info("continuing the Claude Code conversation…".into()),
            Item::Text("Looking at it.".into()),
            Item::Tool { id: "t".into(), name: "Bash".into(), summary: "npm test".into() },
            Item::Text("Fixed: it counts quantities now.".into()),
        ]
    }

    #[test]
    fn the_handoff_is_the_conversation_between_what_to_do_with_it() {
        assert_eq!(
            handoff_text(&items(), "Claude Code", Some("claude-opus-5-5")),
            "You're continuing a conversation that ran on Claude Code (claude-opus-5-5) in this same worktree. Here it is so far:\n\n<user>fix the badge</user>\n<assistant>Looking at it.</assistant>\nBash: npm test\n<assistant>Fixed: it counts quantities now.</assistant>\n\nContinue from here. The files in this worktree are already as that conversation left them.\n\n"
        );
        assert!(handoff_text(&items(), "Codex", None).contains("ran on Codex (the default model) in this same worktree"));
        assert_eq!(handoff_text(&[Item::Info("x".into())], "Codex", None), "", "nothing to carry");
    }

    #[test]
    fn the_handoff_keeps_the_most_recent_40000_characters() {
        let long: Vec<Item> = (0..100).map(|i| Item::User(format!("{i} {}", "é".repeat(995)))).collect();
        let text = handoff_text(&long, "pi", Some("anthropic/claude-opus-5-5"));
        let body = text.split("\n\n").nth(1).unwrap();
        assert!(body.starts_with(&format!("{HANDOFF_CUT}\n<user>")));
        let kept: Vec<&str> = body.split('\n').skip(1).collect();
        assert!(kept.last().unwrap().starts_with("<user>99 "));
        let n = kept.join("\n").chars().count();
        assert!(n <= HANDOFF_CAP && n > HANDOFF_CAP - 1100, "about the cap, in characters: {n}");
        let one = handoff_text(&[Item::Text(format!("start {} end", "x".repeat(50_000)))], "Codex", None);
        let b = one.split("\n\n").nth(1).unwrap();
        assert_eq!(b.split('\n').next(), Some(HANDOFF_CUT));
        assert_eq!(b.split('\n').nth(1).unwrap().chars().count(), HANDOFF_CAP);
        assert!(b.ends_with(" end</assistant>"));
        assert!(!handoff_text(&[Item::User("y".repeat(HANDOFF_CAP - 13))], "Codex", None).contains(HANDOFF_CUT));
    }

    #[test]
    fn models_in_messages() {
        assert_eq!(model_phrase("Codex", Some("gpt-5.5"), Some("GPT-5.5")), "gpt-5.5 (GPT-5.5, Codex)");
        assert_eq!(model_phrase("Gemini CLI", Some("my-model"), Some("my-model")), "my-model (Gemini CLI)");
        assert_eq!(model_phrase("Codex", None, None), "the default model (Codex)");
    }

    fn index() -> ModelIndex {
        let c = |v: &str, l: &str| Choice::new(v, Some(l), None);
        vec![
            ("claude".into(), vec![c("claude-opus-5-5", "Claude Opus 5.5"), c("claude-sonnet-5-5", "Claude Sonnet 5.5")]),
            ("codex".into(), vec![c("gpt-5.5", "GPT-5.5"), c("gpt-5.5-codex", "GPT-5.5 Codex")]),
            ("opencode".into(), vec![c("anthropic/claude-opus-5-5", "Claude Opus 5.5")]),
            ("pi".into(), vec![c("anthropic/claude-opus-5-5", "Claude Opus 5.5"), c("openai/gpt-5.5", "GPT-5.5")]),
        ]
    }

    #[test]
    fn which_engine_runs_an_exact_id() {
        let all = |_: &str| true;
        let p = |e: &str, m: &str, l: &str| Some(ModelPick { engine: e.into(), model: m.into(), label: Some(l.into()) });
        assert_eq!(resolve_model("gpt-5.5", "claude", &index(), &all), p("codex", "gpt-5.5", "GPT-5.5"));
        assert_eq!(resolve_model("claude-sonnet-5-5", "claude", &index(), &all), p("claude", "claude-sonnet-5-5", "Claude Sonnet 5.5"));
        assert_eq!(resolve_model("anthropic/claude-opus-5-5", "pi", &index(), &all), p("pi", "anthropic/claude-opus-5-5", "Claude Opus 5.5"), "the agent's own");
        assert_eq!(resolve_model("anthropic/claude-opus-5-5", "codex", &index(), &all).unwrap().engine, "opencode", "else the first in engine order");
        assert_eq!(resolve_model("anthropic/claude-opus-5-5", "codex", &index(), &|e| e == "pi").unwrap().engine, "pi", "a ready one first");
        assert_eq!(resolve_model("opus", "codex", &index(), &all), None, "no aliases, no partial words");
        assert_eq!(resolve_model("my-own-model", "claude", &index(), &all), None);
    }

    #[test]
    fn completions_are_exact_ids_with_their_names() {
        let ids = |v: Vec<(String, Option<String>)>| v.into_iter().map(|c| c.0).collect::<Vec<_>>();
        assert_eq!(
            model_completions("opus", Some("pi"), &index()),
            vec![("anthropic/claude-opus-5-5".to_string(), Some("Claude Opus 5.5".to_string())), ("claude-opus-5-5".to_string(), Some("Claude Opus 5.5".to_string()))]
        );
        assert_eq!(ids(model_completions("gpt-", Some("claude"), &index())), ["gpt-5.5", "gpt-5.5-codex", "openai/gpt-5.5"]);
        assert_eq!(model_completions("", None, &index()).len(), 6);
    }
}
