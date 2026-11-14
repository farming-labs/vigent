//! The registry: themes, templates, keymaps, engines and setups people share,
//! from the index the website publishes (src/registry.ts, word for word).
//!
//!   vigent add <name>            show the file (and what it runs), ask, then add it
//!   vigent add <kind>/<name>     when two kinds share a name
//!   vigent add --list [kind]     every entry
//!   vigent search <words>        entries whose name, description or tags have them
//!
//! VIGENT_REGISTRY points at another index: a URL, or a local index.json (or
//! the folder it's in). There's no HTTP client in the binary: the index is
//! fetched with curl (Windows 10 and later ship curl.exe), or wget.

use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::config_edit::{Merge, Unit, merge_toml, toml_key};
use crate::toml::Val;

pub const REGISTRY_URL: &str = "https://vigent.farming-labs.dev/registry/index.json";
pub const KINDS: [&str; 5] = ["theme", "template", "keymap", "engine", "setup"];
const PLURAL: [&str; 5] = ["themes", "templates", "keymaps", "engines", "setups"];
const KEY_MODES: [&str; 4] = ["normal", "insert", "visual", "command"];

/// One thing an entry runs: a command, or a rule that lets Claude Code run one without asking.
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub from: String,
    pub command: Option<String>,
    pub allow: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub name: String,
    pub kind: String,
    pub description: String,
    pub author: String,
    pub license: Option<String>,
    pub tags: Vec<String>,
    /// the file it adds, exactly
    pub file: String,
    pub runs: Vec<Run>,
}

/// Where the index comes from: VIGENT_REGISTRY, else the website.
pub fn source() -> String {
    std::env::var("VIGENT_REGISTRY").ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).unwrap_or_else(|| REGISTRY_URL.into())
}

/// Fetch a URL with curl, else wget: the binary carries no HTTP or TLS code of its own.
fn fetch(url: &str) -> Result<String, String> {
    let fail = |why: String| format!("couldn't fetch the registry from {url}: {why}");
    let tools: [(&str, &[&str]); 2] = [("curl", &["-fsSL", "--max-time", "30"]), ("wget", &["-qO-", "--timeout=30"])];
    for (tool, args) in tools {
        let out = match Command::new(tool).args(args).arg(url).stdin(Stdio::null()).output() {
            Ok(o) => o,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(fail(e.to_string())),
        };
        if out.status.success() {
            return String::from_utf8(out.stdout).map_err(|_| fail("it isn't text".into()));
        }
        let err = String::from_utf8_lossy(&out.stderr);
        let why = err.lines().map(str::trim).rfind(|l| !l.is_empty()).map(str::to_string).unwrap_or_else(|| format!("{tool} failed ({})", out.status));
        return Err(fail(why));
    }
    Err(fail("vigent fetches it with curl or wget, and neither is installed (or set VIGENT_REGISTRY to a local index.json)".into()))
}

/// Read the index from a URL or a local file (or the folder holding index.json).
pub fn load(source: &str) -> Result<Vec<Entry>, String> {
    let lower = source.to_ascii_lowercase();
    let text = if lower.starts_with("http://") || lower.starts_with("https://") {
        fetch(source)?
    } else {
        let p = PathBuf::from(source.strip_prefix("file://").unwrap_or(source));
        let mut file = std::path::absolute(&p).unwrap_or(p);
        if file.is_dir() {
            file = file.join("index.json");
        }
        std::fs::read_to_string(&file).map_err(|e| {
            let why = if e.kind() == std::io::ErrorKind::NotFound { "no such file".to_string() } else { e.to_string() };
            format!("couldn't read the registry at {}: {why}", file.display())
        })?
    };
    parse_index(&text, source)
}

/// The index's JSON, checked: entries it can't use are left out.
pub fn parse_index(text: &str, source: &str) -> Result<Vec<Entry>, String> {
    let bad = || format!("the registry at {source} isn't an index vigent can read");
    let raw: serde_json::Value = serde_json::from_str(text).map_err(|_| bad())?;
    let (Some(version), Some(entries)) = (raw.get("version").and_then(|v| v.as_f64()), raw.get("entries").and_then(|e| e.as_array())) else {
        return Err(bad());
    };
    if version > 1.0 {
        return Err(format!("the registry at {source} is newer than this vigent (format {version}): update vigent"));
    }
    let s = |v: &serde_json::Value, k: &str| v.get(k).and_then(|x| x.as_str()).map(str::to_string);
    let mut out = vec![];
    for e in entries {
        let (Some(name), Some(kind), Some(description), Some(author), Some(file)) = (s(e, "name"), s(e, "kind"), s(e, "description"), s(e, "author"), s(e, "file")) else {
            continue;
        };
        let (Some(tags), Some(runs)) = (e.get("tags").and_then(|t| t.as_array()), e.get("runs").and_then(|r| r.as_array())) else {
            continue;
        };
        if !KINDS.contains(&kind.as_str()) {
            continue;
        }
        let runs: Option<Vec<Run>> = runs
            .iter()
            .map(|r| {
                let run = Run { from: s(r, "from")?, command: s(r, "command"), allow: s(r, "allow") };
                (run.command.is_some() || run.allow.is_some()).then_some(run)
            })
            .collect();
        let Some(runs) = runs else { continue };
        let tags = tags.iter().filter_map(|t| t.as_str().map(str::to_string)).collect();
        out.push(Entry { name, kind, description, author, license: s(e, "license"), tags, file, runs });
    }
    Ok(out)
}

/// Up to three names close to the one asked for: a few edits away, or one inside the other.
pub fn close_names(query: &str, names: &[&str]) -> Vec<String> {
    let q = query.to_lowercase();
    let qc: Vec<char> = q.chars().collect();
    let limit = 2.max(qc.len() / 3);
    let mut seen: Vec<&str> = vec![];
    for n in names {
        if !seen.contains(n) {
            seen.push(n);
        }
    }
    let mut found: Vec<(usize, &str)> = seen
        .into_iter()
        .map(|n| (crate::suggest::distance(&qc, &n.to_lowercase().chars().collect::<Vec<_>>()), n))
        .filter(|&(d, n)| d <= limit || (qc.len() >= 3 && n.contains(q.as_str())) || (n.chars().count() >= 3 && q.contains(n)))
        .collect();
    found.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(b.1)));
    found.into_iter().take(3).map(|(_, n)| n.to_string()).collect()
}

/// theme or themes → theme
fn kind_of(word: &str) -> Option<&'static str> {
    KINDS.iter().zip(PLURAL).find(|(k, p)| **k == word || *p == word).map(|(k, _)| *k)
}

/// `kind/name` or `name`: (kind, name), or why the kind isn't one.
pub fn parse_query(query: &str) -> Result<(Option<&'static str>, &str), String> {
    let Some((kind, name)) = query.split_once('/') else { return Ok((None, query)) };
    kind_of(kind).map(|k| (Some(k), name)).ok_or_else(|| format!("unknown kind '{kind}' ({})", KINDS.join(", ")))
}

/// A built-in theme needs nothing added.
pub fn builtin_theme_message(query: &str) -> Option<String> {
    let (kind, name) = parse_query(query).ok()?;
    (kind.is_none_or(|k| k == "theme") && crate::theme::is_builtin(name)).then(|| format!("'{name}' is a built-in theme: nothing to add, use :colorscheme {name}"))
}

/// The entry a query names, or why there isn't one.
pub fn find<'a>(entries: &'a [Entry], query: &str) -> Result<&'a Entry, String> {
    let (kind, name) = parse_query(query)?;
    let found: Vec<&Entry> = entries.iter().filter(|e| e.name == name && kind.is_none_or(|k| e.kind == k)).collect();
    match found.len() {
        1 => Ok(found[0]),
        0 => {
            let names: Vec<&str> = entries.iter().filter(|e| kind.is_none_or(|k| e.kind == k)).map(|e| e.name.as_str()).collect();
            let close = close_names(name, &names);
            let kind = kind.map(|k| format!("{k} ")).unwrap_or_default();
            let hint = if close.is_empty() { "(vigent add --list shows them all)".to_string() } else { format!("(did you mean {}?)", close.join(", ")) };
            Err(format!("no {kind}'{name}' in the registry {hint}"))
        }
        _ => {
            let kinds: Vec<&str> = found.iter().map(|e| e.kind.as_str()).collect();
            let (last, rest) = kinds.split_last().expect("two or more");
            let ways: Vec<String> = found.iter().map(|e| format!("vigent add {}/{}", e.kind, e.name)).collect();
            Err(format!("'{name}' is a {} and a {last}: {}", rest.join(", a "), ways.join(" or ")))
        }
    }
}

/// What an entry owns in config.toml: [templates.x], [engines.x], each [keys.<mode>] mapping.
pub fn entry_units(raw: &[(String, Val)]) -> Vec<Unit> {
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    let mut units = vec![];
    for (top, value) in raw {
        let Some(table) = value.as_table() else {
            units.push(Unit { name: toml_key(top), paths: vec![vec![top.clone()]] });
            continue;
        };
        for (k, v) in table {
            if top == "keys" && KEY_MODES.contains(&k.as_str()) && v.as_table().is_some() {
                // keys right under [keys] are normal-mode mappings
                for (lhs, _) in v.as_table().unwrap_or_default() {
                    let mut paths = vec![s(&["keys", k, lhs])];
                    if k == "normal" {
                        paths.push(s(&["keys", lhs]));
                    }
                    units.push(Unit { name: format!("[keys.{k}] {}", toml_key(lhs)), paths });
                }
            } else if top == "keys" {
                units.push(Unit { name: format!("[keys] {}", toml_key(k)), paths: vec![s(&["keys", k]), s(&["keys", "normal", k])] });
            } else {
                units.push(Unit { name: format!("[{}.{}]", toml_key(top), toml_key(k)), paths: vec![vec![top.clone(), k.clone()]] });
            }
        }
    }
    units
}

/// Where an entry goes, and the text that goes there.
#[derive(Debug)]
pub struct Plan {
    pub file: PathBuf,
    pub text: String,
    /// "It writes" or "It adds to"
    pub verb: &'static str,
    pub replaced: Vec<String>,
    pub done: Vec<String>,
}

pub fn plan(entry: &Entry, force: bool, cwd: &Path) -> Result<Plan, String> {
    let exists = |file: &Path| format!("{} already exists (--force replaces it)", file.display());
    let name = &entry.name;
    match entry.kind.as_str() {
        "theme" | "setup" => {
            let file = if entry.kind == "theme" {
                crate::theme::themes_dir().join(format!("{name}.toml"))
            } else {
                let Some(root) = crate::worktree::repo_root(cwd) else {
                    return Err(format!("not inside a git repository: a setup is the {} at a repository's root", crate::config::PROJECT_CONFIG));
                };
                root.join(crate::config::PROJECT_CONFIG)
            };
            if file.exists() && !force {
                return Err(exists(&file));
            }
            let replaced = if file.exists() { vec![file.display().to_string()] } else { vec![] };
            let done = if entry.kind == "theme" {
                vec![format!("added {}", file.display()), format!("use it with :colorscheme {name} (or theme = \"{name}\" in config.toml)")]
            } else {
                vec![format!("added {}", file.display()), "nothing in it runs until you trust it: :trust in vigent, or vigent trust".into()]
            };
            Ok(Plan { file, text: entry.file.clone(), verb: "It writes", replaced, done })
        }
        _ => {
            let file = crate::config::config_path();
            let text = if file.exists() { std::fs::read_to_string(&file).map_err(|e| format!("couldn't read {}: {e}", file.display()))? } else { String::new() };
            let raw = crate::toml::parse(&entry.file).map_err(|e| format!("the entry's file doesn't parse: {e}"))?;
            let units = entry_units(&raw);
            let (text, replaced) = match merge_toml(&text, &entry.file, &units, force) {
                Merge::Text { text, replaced } => (text, replaced),
                Merge::Conflicts(c) => {
                    let which = if c.len() == 1 { "it" } else { "them" };
                    return Err(format!("{} already has {} (--force replaces {which})", file.display(), c.join(", ")));
                }
                Merge::Error(e) => return Err(format!("couldn't add it to {} ({e}); add it by hand, or fix the file first (vigent config check)", file.display())),
            };
            let names: Vec<String> = units.iter().map(|u| u.name.clone()).collect();
            let what = if entry.kind == "keymap" { format!("{} mapping{}", names.len(), if names.len() == 1 { "" } else { "s" }) } else { names.join(", ") };
            let mut done = vec![format!("added {what} to {}", file.display())];
            done.push(match entry.kind.as_str() {
                "template" => format!("use it with :new -t {name} <task> (vigent reads it when it starts)"),
                "engine" => format!("use it with :new <task> --engine {name}, or engine = \"{name}\" in config.toml (vigent reads it when it starts)"),
                _ => "vigent reads them when it starts".into(),
            });
            Ok(Plan { file, text, verb: "It adds to", replaced, done })
        }
    }
}

/// What `vigent add` shows before it asks: the entry, the file, and everything it runs.
pub fn describe(entry: &Entry, plan: &Plan) -> String {
    let license = entry.license.as_ref().map(|l| format!(" ({l})")).unwrap_or_default();
    let mut out = vec![format!("{}/{} by {}{license}", entry.kind, entry.name, entry.author), entry.description.clone(), String::new(), format!("{} {}:", plan.verb, plan.file.display()), String::new()];
    for l in entry.file.strip_suffix('\n').unwrap_or(&entry.file).split('\n') {
        out.push(if l.trim().is_empty() { String::new() } else { format!("    {l}") });
    }
    out.push(String::new());
    let commands: Vec<&Run> = entry.runs.iter().filter(|r| r.command.is_some()).collect();
    let rules: Vec<&Run> = entry.runs.iter().filter(|r| r.allow.is_some()).collect();
    if !commands.is_empty() {
        out.push("It runs:".into());
        out.extend(commands.iter().map(|r| format!("    $ {}    # {}", r.command.as_deref().unwrap_or_default(), r.from)));
    }
    if !rules.is_empty() {
        out.push("It lets Claude Code run, without asking:".into());
        out.extend(rules.iter().map(|r| format!("    {}    # {}", r.allow.as_deref().unwrap_or_default(), r.from)));
    }
    if !commands.is_empty() || !rules.is_empty() {
        out.push(String::new());
    }
    if !plan.replaced.is_empty() {
        out.push(format!("It replaces {}", plan.replaced.join(", ")));
        out.push(String::new());
    }
    out.join("\n")
}

/// Ask on the terminal; no answer (the end of the input) is no.
fn ask(question: &str) -> bool {
    print!("{question}");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    match std::io::stdin().lock().read_line(&mut line) {
        Ok(n) if n > 0 => {
            // a terminal echoed the answer and its Enter; piped in, it wasn't
            if !std::io::stdin().is_terminal() {
                println!();
            }
            let a = line.trim().to_ascii_lowercase();
            a == "y" || a == "yes"
        }
        _ => {
            println!();
            false
        }
    }
}

const ADD_USAGE: &str = "usage: vigent add <name> | <kind>/<name> [--yes] [--force]   vigent add --list [kind]";

fn load_or_say() -> Option<Vec<Entry>> {
    load(&source()).map_err(|e| eprintln!("vigent: {e}")).ok()
}

/// `vigent add`.
pub fn add_command(args: &[String]) -> i32 {
    let has = |f: &[&str]| args.iter().any(|a| f.contains(&a.as_str()));
    let (list, yes, force) = (has(&["--list", "-l"]), has(&["--yes", "-y"]), has(&["--force"]));
    if let Some(bad) = args.iter().find(|a| a.starts_with('-') && !["--list", "-l", "--yes", "-y", "--force"].contains(&a.as_str())) {
        eprintln!("vigent: unknown option '{bad}'\n{ADD_USAGE}");
        return 1;
    }
    let words: Vec<&str> = args.iter().filter(|a| !a.starts_with('-')).map(String::as_str).collect();
    if list {
        let kind = words.first().map(|w| kind_of(w));
        if let Some(None) = kind {
            eprintln!("vigent: unknown kind '{}' ({})", words[0], KINDS.join(", "));
            return 1;
        }
        let Some(entries) = load_or_say() else { return 1 };
        let width = entries.iter().map(|e| e.name.chars().count()).max().unwrap_or(0) + 2;
        for (k, plural) in KINDS.iter().zip(PLURAL) {
            let shown: Vec<&Entry> = entries.iter().filter(|e| e.kind == *k).collect();
            if kind.is_some_and(|x| x != Some(*k)) || shown.is_empty() {
                continue;
            }
            println!("{plural}");
            for e in shown {
                println!("  {:<width$}{}", e.name, e.description);
            }
        }
        return 0;
    }
    let [query] = words[..] else {
        eprintln!("{ADD_USAGE}");
        return 1;
    };
    if let Some(m) = builtin_theme_message(query) {
        eprintln!("vigent: {m}");
        return 1;
    }
    if let Err(e) = parse_query(query) {
        eprintln!("vigent: {e}");
        return 1;
    }
    let Some(entries) = load_or_say() else { return 1 };
    let entry = match find(&entries, query) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("vigent: {e}");
            return 1;
        }
    };
    let cwd = std::env::current_dir().unwrap_or_default();
    let plan = match plan(entry, force, &cwd) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("vigent: {e}");
            return 1;
        }
    };
    println!("{}", describe(entry, &plan));
    if !yes && !ask("Add it? [y/N] ") {
        println!("nothing added");
        return 1;
    }
    let written = plan.file.parent().map(std::fs::create_dir_all).unwrap_or(Ok(())).and_then(|_| std::fs::write(&plan.file, &plan.text));
    if let Err(e) = written {
        eprintln!("vigent: couldn't write {}: {e}", plan.file.display());
        return 1;
    }
    for l in &plan.done {
        println!("{l}");
    }
    0
}

/// `vigent search <words>`: entries with every word in their kind, name, description, tags or author.
pub fn search_command(args: &[String]) -> i32 {
    let words: Vec<String> = args.iter().filter(|a| !a.starts_with('-')).map(|w| w.to_lowercase()).collect();
    if words.is_empty() {
        eprintln!("usage: vigent search <words>");
        return 1;
    }
    let Some(entries) = load_or_say() else { return 1 };
    let found: Vec<&Entry> = entries
        .iter()
        .filter(|e| {
            let text = format!("{} {} {} {} {}", e.kind, e.name, e.description, e.tags.join(" "), e.author).to_lowercase();
            words.iter().all(|w| text.contains(w.as_str()))
        })
        .collect();
    if found.is_empty() {
        eprintln!("vigent: nothing in the registry matches '{}' (vigent add --list shows them all)", words.join(" "));
        return 1;
    }
    let width = found.iter().map(|e| e.kind.len() + 1 + e.name.chars().count()).max().unwrap_or(0) + 2;
    for e in found {
        println!("  {:<width$}{}", format!("{}/{}", e.kind, e.name), e.description);
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config_edit::{StmtKind, toml_statements};

    fn entry(kind: &str, name: &str, file: &str) -> Entry {
        Entry { name: name.into(), kind: kind.into(), description: "d".into(), author: "a".into(), license: None, tags: vec![], file: file.into(), runs: vec![] }
    }

    #[test]
    fn reads_the_index() {
        let json = r#"{"version":1,"entries":[
            {"name":"x","kind":"engine","description":"d","author":"a","license":"MIT","tags":["t"],"file":"[engines.x]\ncommand = [\"x\"]\n","runs":[{"from":"engines.x.command","command":"x {prompt}"}]},
            {"name":"bad","kind":"plugin","description":"d","author":"a","tags":[],"file":"","runs":[]},
            {"name":"norun","kind":"setup","description":"d","author":"a","tags":[],"file":"","runs":[{"from":"hooks.x"}]}
        ]}"#;
        let e = parse_index(json, "here").unwrap();
        assert_eq!(e.len(), 1, "unknown kinds and broken runs are left out");
        assert_eq!(e[0].runs[0].command.as_deref(), Some("x {prompt}"));
        assert_eq!(e[0].license.as_deref(), Some("MIT"));
        assert!(parse_index("[]", "here").unwrap_err().contains("isn't an index vigent can read"));
        assert!(parse_index(r#"{"version":2,"entries":[]}"#, "here").unwrap_err().contains("newer than this vigent (format 2)"));
    }

    #[test]
    fn finds_entries_and_says_why_not() {
        let all = vec![entry("theme", "harbor", ""), entry("template", "fix-ci", ""), entry("theme", "both", ""), entry("template", "both", "")];
        assert_eq!(find(&all, "harbor").unwrap().name, "harbor");
        assert_eq!(find(&all, "template/both").unwrap().kind, "template");
        assert_eq!(find(&all, "harbr").unwrap_err(), "no 'harbr' in the registry (did you mean harbor?)");
        assert_eq!(find(&all, "zzzzzzzz").unwrap_err(), "no 'zzzzzzzz' in the registry (vigent add --list shows them all)");
        assert_eq!(find(&all, "both").unwrap_err(), "'both' is a theme and a template: vigent add theme/both or vigent add template/both");
        assert_eq!(find(&all, "plugin/x").unwrap_err(), "unknown kind 'plugin' (theme, template, keymap, engine, setup)");
        assert_eq!(find(&all, "engines/harbor").unwrap_err(), "no engine 'harbor' in the registry (vigent add --list shows them all)");
        assert_eq!(close_names("pnpm", &["pnpm-monorepo", "cargo-workspace"]), ["pnpm-monorepo"]);
        assert_eq!(builtin_theme_message("theme/nord").as_deref(), Some("'nord' is a built-in theme: nothing to add, use :colorscheme nord"));
        assert_eq!(builtin_theme_message("template/nord"), None);
    }

    #[test]
    fn units_name_what_an_entry_owns() {
        let raw = crate::toml::parse("[keys.normal]\n\"<leader>an\" = \":new \"\n[templates.x]\nprompt = \"p\"\n").unwrap();
        let u = entry_units(&raw);
        assert_eq!(u.iter().map(|u| u.name.as_str()).collect::<Vec<_>>(), ["[keys.normal] \"<leader>an\"", "[templates.x]"]);
        assert_eq!(u[0].paths.len(), 2, "[keys] \"x\" is a normal-mode mapping too");
    }

    #[test]
    fn statements_over_several_lines() {
        let text = "theme = \"nord\"\nlist = [\n  \"a\", # [not.a.table]\n  \"b\",\n]\nnote = \"\"\"\n[also.not]\n\"\"\"\n\n[ui]\nnumber = false\n";
        let got: Vec<(StmtKind, usize, usize, String)> = toml_statements(text).into_iter().map(|s| (s.kind, s.start, s.end, s.path.join("."))).collect();
        use StmtKind::*;
        assert_eq!(
            got,
            [(Kv, 0, 1, "theme".into()), (Kv, 1, 5, "list".into()), (Kv, 5, 8, "note".into()), (Other, 8, 9, String::new()), (Header, 9, 10, "ui".into()), (Kv, 10, 11, "number".into())]
        );
    }

    #[test]
    fn merges_beside_whats_there() {
        let unit = |name: &str, paths: &[&[&str]]| Unit { name: name.into(), paths: paths.iter().map(|p| p.iter().map(|s| s.to_string()).collect()).collect() };
        let t = unit("[templates.x]", &[&["templates", "x"]]);
        let entry = "[templates.x]\nprompt = \"\"\"\nhi \\\n  there\n\"\"\"\n";
        let text = "# mine\ntheme = \"nord\"\n\n[ui]\nnumber = false\n";
        assert_eq!(merge_toml(text, entry, std::slice::from_ref(&t), false), Merge::Text { text: format!("{text}\n{entry}"), replaced: vec![] });
        assert_eq!(merge_toml("", entry, std::slice::from_ref(&t), false), Merge::Text { text: entry.into(), replaced: vec![] });

        let dotted = "templates.x.prompt = \"mine\"\n\n[ui]\nnumber = false\n";
        assert_eq!(merge_toml(dotted, entry, std::slice::from_ref(&t), false), Merge::Conflicts(vec!["[templates.x]".into()]));
        assert_eq!(merge_toml(dotted, entry, std::slice::from_ref(&t), true), Merge::Text { text: format!("[ui]\nnumber = false\n\n{entry}"), replaced: vec!["[templates.x]".into()] });

        // into the table that's there, comment and all; the comment isn't repeated when it's added again
        let keys = unit("[keys.normal] \"y\"", &[&["keys", "normal", "y"], &["keys", "y"]]);
        let km = "# why\n[keys.normal]\n\"y\" = \"G\"\n";
        let text = "[keys.normal]\n\"x\" = \"gg\"\n\n[keys.insert]\n";
        let Merge::Text { text: once, .. } = merge_toml(text, km, std::slice::from_ref(&keys), false) else { panic!() };
        assert_eq!(once, "[keys.normal]\n\"x\" = \"gg\"\n# why\n\"y\" = \"G\"\n\n[keys.insert]\n");
        let Merge::Text { text: twice, .. } = merge_toml(&once, km, std::slice::from_ref(&keys), true) else { panic!() };
        assert_eq!(twice, once);
        assert_eq!(merge_toml("[keys]\n\"y\" = \"x\"\n", km, std::slice::from_ref(&keys), false), Merge::Conflicts(vec!["[keys.normal] \"y\"".into()]));

        // a table written inline can't get a header: nothing changes
        assert!(matches!(merge_toml("keys = { normal = { \"x\" = \"gg\" } }\n", km, &[keys], false), Merge::Error(_)));
        assert!(matches!(merge_toml("theme = \n", entry, &[t], false), Merge::Error(_)));
    }
}
