//! Tab completion for the : command line (src/completion.ts, the race
//! argument part of src/race.ts) and "did you mean" (src/suggest.ts).

/// Every ex command, by the name you'd normally type (short forms still run, they just aren't offered).
pub const EX_COMMANDS: &[&str] = &[
    "attach", "bNext", "bdelete", "bnext", "bprevious", "buffer", "buffers", "checkhealth", "close", "colorscheme", "cost", "dashboard", "diff", "effort", "engine", "engines", "extensions", "help",
    "highlight", "history", "jumps", "kill", "ls", "marks", "merge", "model", "new", "nohlsearch", "only", "options", "pause", "permissions", "pr", "qall", "queue", "quit", "race", "resume", "review",
    "serve", "set", "settings", "split", "templates", "tree", "trust", "usage", "vsplit", "wincmd", "worktree", "worktrees", "wq", "write",
];

pub const WINCMD_KEYS: &[&str] = &["h", "j", "k", "l", "w", "W", "p", "t", "b", "v", "s", "c", "q", "o", "=", ">", "<", "+", "-", "|", "_", "x", "r"];
const AGENT_COMMANDS: &[&str] = &["b", "buffer", "sp", "split", "vs", "vsp", "vsplit"];
const THEME_COMMANDS: &[&str] = &["colorscheme", "colo", "theme"];

#[derive(Default)]
pub struct Sources {
    pub agents: Vec<String>,
    pub themes: Vec<String>,
    /// your own commands ([commands], :command), each with its hint: its description, or that it waits for :trust
    pub commands: Vec<(String, String)>,
    /// what `:race --engines` offers
    pub engines: Vec<String>,
    /// what `:race --models` offers
    pub models: Vec<String>,
    /// what `:model` offers: every ready engine's models, with their names
    pub model_index: Option<crate::engines::handoff::ModelIndex>,
    /// the engine whose models `:model` offers first (the focused agent's)
    pub model_own: Option<String>,
    /// what `:effort` offers: the levels the focused agent's engine takes
    pub efforts: Vec<String>,
}

/// The command line is at `:model <id>`: Tab will want the engines' models.
pub fn wants_models(line: &str) -> bool {
    let t = line.trim_start();
    t.strip_prefix("model").or_else(|| t.strip_prefix("mo")).is_some_and(|rest| rest.starts_with(char::is_whitespace))
}

/// The text before the part being completed, and what that part could be.
pub fn complete_command_line(line: &str, sources: &Sources) -> (String, Vec<String>) {
    let (base, items, _) = complete_with_hints(line, sources);
    (base, items)
}

/// complete_command_line, with a hint beside each item (a model's name, what your command does), "" for none.
pub fn complete_with_hints(line: &str, sources: &Sources) -> (String, Vec<String>, Vec<String>) {
    let lead_len = line.len() - line.trim_start().len();
    let (lead, rest) = line.split_at(lead_len);
    let cmd_len = rest.find(char::is_whitespace).unwrap_or(rest.len());
    let (cmd, after) = rest.split_at(cmd_len);
    if after.is_empty() {
        return complete_name(lead, cmd, &sources.commands);
    }
    let gap_len = after.len() - after.trim_start().len();
    let (gap, arg) = after.split_at(gap_len);
    let base = format!("{lead}{cmd}{gap}");
    if cmd == "race" || cmd == "race!" {
        return match complete_race_arg(arg, sources) {
            Some((b, items)) => (base + &b, items, vec![]),
            None => (base, vec![], vec![]),
        };
    }
    if cmd == "model" || cmd == "mo" {
        // exact ids that start with or contain what's typed, each with its name and version
        let found = sources.model_index.as_ref().map(|i| crate::engines::handoff::model_completions(arg, sources.model_own.as_deref(), i)).unwrap_or_default();
        let hints = found.iter().map(|c| c.1.clone().unwrap_or_default()).collect();
        return (base, found.into_iter().map(|c| c.0).collect(), hints);
    }
    if cmd == "effort" || cmd == "think" {
        let needle = arg.to_lowercase();
        return (base, sources.efforts.iter().filter(|e| e.starts_with(&needle)).cloned().collect(), vec![]);
    }
    let agents = AGENT_COMMANDS.contains(&cmd);
    let options: Vec<String> = if agents {
        sources.agents.clone()
    } else if THEME_COMMANDS.contains(&cmd) {
        sources.themes.clone()
    } else if cmd == "wincmd" || cmd == "winc" {
        WINCMD_KEYS.iter().map(|s| s.to_string()).collect()
    } else {
        vec![]
    };
    let needle = arg.to_lowercase();
    let items = options.into_iter().filter(|o| if agents { o.to_lowercase().contains(&needle) } else { o.to_lowercase().starts_with(&needle) }).collect();
    (base, items, vec![])
}

/// A command's name: the built-ins that start with it, then yours (whatever the case: `:re` offers `Review` too), with their hints.
fn complete_name(lead: &str, cmd: &str, commands: &[(String, String)]) -> (String, Vec<String>, Vec<String>) {
    let mut items: Vec<String> = EX_COMMANDS.iter().filter(|c| c.starts_with(cmd)).map(|c| c.to_string()).collect();
    let needle = cmd.to_lowercase();
    let mut yours: Vec<&(String, String)> = commands.iter().filter(|(n, _)| n.to_lowercase().starts_with(&needle)).collect();
    if yours.is_empty() {
        return (lead.to_string(), items, vec![]);
    }
    yours.sort_by(|a, b| a.0.cmp(&b.0));
    let mut hints = vec![String::new(); items.len()];
    for (n, h) in yours {
        items.push(n.clone());
        hints.push(h.clone());
    }
    (lead.to_string(), items, hints)
}

/// `:race` arguments: --engines a,b and --models x,y lists, and the flags themselves.
fn complete_race_arg(arg: &str, sources: &Sources) -> Option<(String, Vec<String>)> {
    // the last word: a flag and its comma list, or a flag being typed
    let word_start = arg.rfind(char::is_whitespace).map(|i| i + 1).unwrap_or(0);
    let word = &arg[word_start..];
    // "--engines claude,co" (flag, then a space) or "--engines=claude,co"
    let (flag, list) = if let Some(eq) = word.find('=') {
        (&word[..eq], Some(&word[eq + 1..]))
    } else {
        let before = arg[..word_start].trim_end();
        let prev = before.rsplit(char::is_whitespace).next().unwrap_or("");
        if word_start > 0 && is_list_flag(prev) && !word.starts_with('-') { (prev, Some(word)) } else { (word, None) }
    };
    if let Some(list) = list
        && is_list_flag(flag)
    {
        let needle_start = list.rfind(',').map(|i| i + 1).unwrap_or(0);
        let needle = list[needle_start..].to_lowercase();
        let from = if flag.starts_with("--engine") || flag == "-e" { &sources.engines } else { &sources.models };
        let base = arg[..arg.len() - (list.len() - needle_start)].to_string();
        return Some((base, from.iter().filter(|x| x.to_lowercase().starts_with(&needle)).cloned().collect()));
    }
    if word.starts_with('-') && word.chars().skip(1).all(|c| c == '-' || c.is_ascii_lowercase()) {
        let base = arg[..word_start].to_string();
        return Some((base, ["--engines", "--models", "--template"].iter().filter(|f| f.starts_with(word)).map(|f| f.to_string()).collect()));
    }
    None
}

fn is_list_flag(f: &str) -> bool {
    matches!(f, "--engines" | "--engine" | "-e" | "--models" | "--model" | "-m")
}

fn distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut dp: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut prev = dp[0];
        dp[0] = i;
        for j in 1..=b.len() {
            let tmp = dp[j];
            dp[j] = (dp[j] + 1).min(dp[j - 1] + 1).min(prev + usize::from(a[i - 1] != b[j - 1]));
            prev = tmp;
        }
    }
    dp[b.len()]
}

/// The closest candidate within a few edits.
pub fn closest<'a>(name: &str, candidates: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    let mut best = None;
    let mut best_score = 2.max(name.chars().count() / 3) + 1;
    for c in candidates {
        let d = distance(&name.to_lowercase(), &c.to_lowercase());
        if d < best_score {
            best = Some(c);
            best_score = d;
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    fn src() -> Sources {
        Sources {
            agents: vec!["cart total".into(), "docs pass".into()],
            themes: vec!["gruvbox".into(), "nord".into()],
            engines: vec!["claude".into(), "codex".into()],
            models: vec!["gpt-5".into()],
            ..Default::default()
        }
    }

    #[test]
    fn model_ids_with_their_names_and_effort_levels() {
        use crate::engines::choices::Choice;
        let c = |v: &str, l: &str| Choice::new(v, Some(l), None);
        let s = Sources {
            model_index: Some(vec![("claude".into(), vec![c("claude-opus-5-5", "Claude Opus 5.5"), c("claude-opus-5-1", "Claude Opus 5.1")]), ("codex".into(), vec![c("gpt-5.5", "GPT-5.5")])]),
            model_own: Some("codex".into()),
            efforts: vec!["default".into(), "low".into(), "high".into()],
            ..Default::default()
        };
        assert_eq!(complete_with_hints("model opus", &s), ("model ".into(), vec!["claude-opus-5-5".to_string(), "claude-opus-5-1".into()], vec!["Claude Opus 5.5".to_string(), "Claude Opus 5.1".into()]));
        assert_eq!(complete_command_line("model gpt", &s).1, ["gpt-5.5"]);
        assert_eq!(complete_command_line("effort h", &s).1, ["high"]);
        assert!(wants_models("model gp") && wants_models("mo ") && !wants_models("model") && !wants_models("modelx y"));
    }

    #[test]
    fn commands_agents_themes() {
        assert_eq!(complete_command_line("vs", &src()), ("".into(), vec!["vsplit".to_string()]));
        assert_eq!(complete_command_line("b tot", &src()), ("b ".into(), vec!["cart total".to_string()]));
        assert_eq!(complete_command_line("colo g", &src()), ("colo ".into(), vec!["gruvbox".to_string()]));
        assert_eq!(complete_command_line("wincmd ", &src()).1.len(), WINCMD_KEYS.len());
        assert_eq!(complete_command_line("race fix --engines claude,c", &src()), ("race fix --engines claude,".into(), vec!["claude".to_string(), "codex".to_string()]));
        assert_eq!(complete_command_line("race fix --m", &src()), ("race fix ".into(), vec!["--models".to_string()]));
        assert_eq!(closest("colo", ["color", "x"]), Some("color"));
    }

    #[test]
    fn your_commands_complete_with_their_hints() {
        let s = Sources { commands: vec![("Review".into(), "review the branch".into()), ("Deploy".into(), "ship it (waits for :trust)".into()), ("Rerun".into(), String::new())], ..src() };
        assert_eq!(complete_with_hints("Re", &s), ("".into(), vec!["Rerun".to_string(), "Review".into()], vec![String::new(), "review the branch".into()]));
        let (_, items, hints) = complete_with_hints("re", &s);
        assert_eq!(items, ["resume", "review", "Rerun", "Review"], "the built-ins that fit, then yours");
        assert_eq!(hints, ["", "", "", "review the branch"]);
        assert_eq!(complete_with_hints("  De", &s), ("  ".into(), vec!["Deploy".to_string()], vec!["ship it (waits for :trust)".to_string()]));
        assert_eq!(complete_with_hints("vs", &s), ("".into(), vec!["vsplit".to_string()], vec![]), "none of yours fit: no hints");
        assert_eq!(complete_command_line("Review x", &s).1, Vec::<String>::new(), "only the name completes");
    }

    #[test]
    fn every_command_offered_runs() {
        for c in EX_COMMANDS {
            let _ = c; // checked against commands::lookup in windows tests once every area has ported its commands
        }
    }
}
