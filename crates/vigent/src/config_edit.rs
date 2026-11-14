//! Edit config.toml in place for `:settings` and `:permissions`: change one
//! value and keep everything else (comments, order, other keys) as the user
//! wrote it (src/config-edit.ts).

/// A TOML string value (JSON's quoting is TOML's basic string).
pub fn toml_str(v: &str) -> String {
    serde_json::to_string(v).unwrap_or_default()
}

/// A TOML key: bare when it can be, quoted otherwise (`"<leader>n"`).
pub fn toml_key(k: &str) -> String {
    if !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') { k.to_string() } else { toml_str(k) }
}

/// `[section]` on this line (with an optional comment after it): the section's name.
fn header(line: &str) -> Option<&str> {
    let t = line.trim_start();
    let rest = t.strip_prefix('[')?;
    let close = rest.find(']')?;
    let name = &rest[..close];
    if name.is_empty() || name.contains('[') {
        return None;
    }
    let after = rest[close + 1..].trim_start();
    if after.is_empty() || after.starts_with('#') { Some(name) } else { None }
}

/// `key =` (or `# key =`) starts this line.
fn is_key_line(line: &str, key: &str) -> bool {
    let mut t = line.trim_start();
    if let Some(r) = t.strip_prefix('#') {
        t = r.trim_start();
    }
    t.strip_prefix(key).is_some_and(|r| r.trim_start().starts_with('='))
}

fn commented(line: &str) -> bool {
    line.trim_start().starts_with('#')
}

/// Set `key = value` in `[section]` (or at the top level when section is
/// None). A commented-out line for the same key (`# key = …`) is replaced in
/// place, so the setting stays where the template documents it.
pub fn set_toml_value(text: &str, section: Option<&str>, key: &str, value: &str) -> String {
    let mut lines: Vec<String> = if text.is_empty() { vec![] } else { text.strip_suffix('\n').unwrap_or(text).split('\n').map(str::to_string).collect() };
    let k = toml_key(key);
    let line = format!("{k} = {value}");
    let mut start = 0;
    let mut end = lines.iter().position(|l| header(l).is_some()).unwrap_or(lines.len());
    if let Some(section) = section {
        let Some(at) = lines.iter().position(|l| header(l).map(str::trim) == Some(section)) else {
            // a new section at the end
            if lines.last().is_some_and(|l| !l.trim().is_empty()) {
                lines.push(String::new());
            }
            lines.push(format!("[{section}]"));
            lines.push(line);
            return format!("{}\n", lines.join("\n"));
        };
        start = at + 1;
        end = (start..lines.len()).find(|&i| header(&lines[i]).is_some()).unwrap_or(lines.len());
    }
    let live = (start..end).find(|&i| is_key_line(&lines[i], &k) && !commented(&lines[i]));
    let off = (start..end).find(|&i| is_key_line(&lines[i], &k) && commented(&lines[i]));
    match live.or(off) {
        Some(at) => lines[at] = line,
        None => {
            // after the last non-blank line of the section
            let mut insert = end;
            while insert > start && lines[insert - 1].trim().is_empty() {
                insert -= 1;
            }
            lines.insert(insert, line);
        }
    }
    format!("{}\n", lines.join("\n"))
}

// ---------------------------------------------------------------------------
// Adding tables (vigent add): a registry entry's [templates.x], [keys.normal] ...
// ---------------------------------------------------------------------------

use crate::toml::Val;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum StmtKind {
    Header,
    Kv,
    Other,
}

/// One statement of a TOML file, by line: a [table] header, a key = value (as many lines as its value takes), or a comment or blank line.
#[derive(Clone, Debug)]
pub struct Stmt {
    pub kind: StmtKind,
    /// its first line, and the line after its last
    pub start: usize,
    pub end: usize,
    /// a header's table, or a key's path within its table
    pub path: Vec<String>,
    /// [[an array of tables]]
    pub array: bool,
}

fn line_list(text: &str) -> Vec<String> {
    if text.is_empty() { vec![] } else { text.strip_suffix('\n').unwrap_or(text).split('\n').map(str::to_string).collect() }
}

/// The line after the one a value starting at lines[line][col] ends on: strings, arrays and inline tables can go on for lines.
fn value_end(lines: &[String], line: usize, col: usize) -> usize {
    let mut depth = 0i32;
    // the quote that opened the string it's in, and how many of them
    let mut quote: Option<(u8, usize)> = None;
    for (l, text) in lines.iter().enumerate().skip(line) {
        let s = text.as_bytes();
        let mut i = if l == line { col } else { 0 };
        while i < s.len() {
            let c = s[i];
            if let Some((q, n)) = quote {
                if q == b'"' && c == b'\\' {
                    i += 1;
                } else if s[i..].starts_with(&vec![q; n]) {
                    // """a""""" ends with the last three quotes
                    if n == 3 {
                        while s.get(i + 3) == Some(&q) {
                            i += 1;
                        }
                    }
                    i += n - 1;
                    quote = None;
                }
            } else if c == b'#' {
                break;
            } else if c == b'"' || c == b'\'' {
                let n = if s[i..].starts_with(&[c; 3]) { 3 } else { 1 };
                quote = Some((c, n));
                i += n - 1;
            } else if c == b'[' || c == b'{' {
                depth += 1;
            } else if c == b']' || c == b'}' {
                depth -= 1;
            }
            i += 1;
        }
        // a one-line string ends with its line, closed or not
        if matches!(quote, Some((_, 1))) {
            quote = None;
        }
        if quote.is_none() && depth <= 0 {
            return l + 1;
        }
    }
    lines.len()
}

/// Split TOML text into statements, by line (src/config-edit.ts tomlStatements).
pub fn toml_statements(text: &str) -> Vec<Stmt> {
    let lines = line_list(text);
    let mut out = vec![];
    let mut i = 0;
    while i < lines.len() {
        let t = lines[i].trim();
        if t.starts_with('[') {
            let array = t.starts_with("[[");
            let path = crate::toml::read_key(t, if array { 2 } else { 1 }).map(|(p, _)| p).unwrap_or_default();
            out.push(Stmt { kind: StmtKind::Header, start: i, end: i + 1, path, array });
            i += 1;
            continue;
        }
        let key = if t.is_empty() || t.starts_with('#') { None } else { crate::toml::read_key(&lines[i], 0) };
        match key {
            Some((path, at)) if lines[i].as_bytes().get(at) == Some(&b'=') => {
                let end = value_end(&lines, i, at + 1);
                out.push(Stmt { kind: StmtKind::Kv, start: i, end, path, array: false });
                i = end;
            }
            _ => {
                out.push(Stmt { kind: StmtKind::Other, start: i, end: i + 1, path: vec![], array: false });
                i += 1;
            }
        }
    }
    out
}

/// What an entry adds and owns: one name for messages, and every path that means it ([keys.normal] "x" is also [keys] "x").
#[derive(Clone, Debug)]
pub struct Unit {
    pub name: String,
    pub paths: Vec<Vec<String>>,
}

#[derive(Debug, PartialEq)]
pub enum Merge {
    Text { text: String, replaced: Vec<String> },
    Conflicts(Vec<String>),
    Error(String),
}

/// The value at a path of tables.
pub fn value_at<'a>(t: &'a [(String, Val)], path: &[String]) -> Option<&'a Val> {
    let (first, rest) = path.split_first()?;
    let v = t.iter().find(|(k, _)| k == first).map(|(_, v)| v)?;
    if rest.is_empty() { Some(v) } else { value_at(v.as_table()?, rest) }
}

fn remove_at(t: &mut Vec<(String, Val)>, path: &[String]) {
    let Some((first, rest)) = path.split_first() else { return };
    if rest.is_empty() {
        t.retain(|(k, _)| k != first);
    } else if let Some((_, Val::Table(inner))) = t.iter_mut().find(|(k, _)| k == first) {
        remove_at(inner, rest);
    }
}

fn merge_into(into: &mut Vec<(String, Val)>, from: &[(String, Val)]) {
    for (k, v) in from {
        match (into.iter_mut().find(|(k2, _)| k2 == k), v) {
            (Some((_, Val::Table(inner))), Val::Table(add)) => merge_into(inner, add),
            (Some((_, old)), _) => *old = v.clone(),
            (None, _) => into.push((k.clone(), v.clone())),
        }
    }
}

fn starts_with(p: &[String], prefix: &[String]) -> bool {
    prefix.len() <= p.len() && prefix.iter().zip(p).all(|(a, b)| a == b)
}

fn joined(lines: &[String]) -> String {
    if lines.is_empty() { String::new() } else { format!("{}\n", lines.join("\n")) }
}

const CANT_ADD: &str = "its tables are written in a way this edit can't add to";

/// Add an entry's TOML to config.toml's text, keeping everything else as it's
/// written (src/config-edit.ts mergeToml): a table the file already has gets
/// the entry's keys at its end, a new one goes at the end of the file. A unit
/// the file already has is a conflict, unless `force`, which takes the file's
/// own out first. The result is read back, and must be the file's settings
/// plus the entry's, or nothing changes.
pub fn merge_toml(text: &str, entry: &str, units: &[Unit], force: bool) -> Merge {
    let first_line = |e: String| e.lines().next().unwrap_or_default().to_string();
    let mine = match crate::toml::parse(text) {
        Ok(t) => t,
        Err(e) => return Merge::Error(first_line(e)),
    };
    let adds = match crate::toml::parse(entry) {
        Ok(t) => t,
        Err(e) => return Merge::Error(format!("the entry: {}", first_line(e))),
    };
    let taken: Vec<&Unit> = units.iter().filter(|u| u.paths.iter().any(|p| value_at(&mine, p).is_some())).collect();
    if !taken.is_empty() && !force {
        return Merge::Conflicts(taken.iter().map(|u| u.name.clone()).collect());
    }
    let owned: Vec<&Vec<String>> = taken.iter().flat_map(|u| u.paths.iter()).collect();
    let under = |p: &[String]| owned.iter().any(|u| starts_with(p, u));

    // the file's own, out
    let mut lines = line_list(text);
    if !owned.is_empty() {
        let mut keep = vec![true; lines.len()];
        let stmts = toml_statements(text);
        let mut section: Vec<String> = vec![];
        for (i, s) in stmts.iter().enumerate() {
            if s.kind == StmtKind::Header {
                section = s.path.clone();
                if s.array || !under(&section) {
                    continue;
                }
                // the whole table, but not the comments after its last key: they're about the next one
                let mut last = i;
                for (j, t) in stmts.iter().enumerate().skip(i + 1) {
                    if t.kind == StmtKind::Header {
                        break;
                    }
                    if t.kind == StmtKind::Kv {
                        last = j;
                    }
                }
                keep[s.start..stmts[last].end].fill(false);
            } else if s.kind == StmtKind::Kv && under(&[section.clone(), s.path.clone()].concat()) {
                keep[s.start..s.end].fill(false);
            }
        }
        // no two blank lines where something came out
        let mut i = 0;
        while i < lines.len() {
            if keep[i] {
                i += 1;
                continue;
            }
            let mut j = i;
            while j < lines.len() && !keep[j] {
                j += 1;
            }
            if (i == 0 || lines[i - 1].trim().is_empty()) && j < lines.len() && lines[j].trim().is_empty() {
                keep[j] = false;
            }
            i = j + 1;
        }
        lines = lines.into_iter().zip(keep).filter(|(_, k)| *k).map(|(l, _)| l).collect();
    }

    // the entry, in blocks: what comes before its first header, then each table with the comments just above it
    struct Block {
        header: Option<usize>,
        path: Vec<String>,
        lead: Vec<String>,
        body: Vec<String>,
    }
    let elines = line_list(entry);
    let mut blocks = vec![Block { header: None, path: vec![], lead: vec![], body: vec![] }];
    let mut pending: Vec<usize> = vec![];
    for s in toml_statements(entry) {
        match s.kind {
            StmtKind::Other => pending.extend(s.start..s.end),
            StmtKind::Header => {
                blocks.push(Block { header: Some(s.start), path: s.path.clone(), lead: pending.iter().map(|&i| elines[i].clone()).collect(), body: vec![] });
                pending.clear();
            }
            StmtKind::Kv => {
                let b = blocks.last_mut().expect("a block");
                b.body.extend(pending.iter().map(|&i| elines[i].clone()));
                b.body.extend((s.start..s.end).map(|i| elines[i].clone()));
                pending.clear();
            }
        }
    }
    blocks.last_mut().expect("a block").body.extend(pending.iter().map(|&i| elines[i].clone()));
    let trim = |l: &[String]| {
        let a = l.iter().position(|x| !x.trim().is_empty()).unwrap_or(l.len());
        let b = l.iter().rposition(|x| !x.trim().is_empty()).map_or(a, |b| b + 1);
        l[a..b.max(a)].to_vec()
    };

    for b in &blocks {
        let lead = trim(&b.lead);
        let body = trim(&b.body);
        if b.header.is_none() && body.is_empty() {
            continue;
        }
        let stmts = toml_statements(&joined(&lines));
        let first_header = stmts.iter().find(|s| s.kind == StmtKind::Header).map(|s| s.start);
        let own = if b.header.is_some() { stmts.iter().find(|s| s.kind == StmtKind::Header && !s.array && s.path == b.path) } else { None };
        if b.header.is_none() || own.is_some() {
            // at the end of that table (or of the top, before the first table), after its last line that isn't blank
            let start = own.map_or(0, |s| s.end);
            let next = stmts.iter().find(|s| s.kind == StmtKind::Header && s.start >= start).map(|s| s.start);
            let mut insert = next.unwrap_or(lines.len());
            while insert > start && lines[insert - 1].trim().is_empty() {
                insert -= 1;
            }
            // a comment the table already has (from adding this entry before) isn't repeated
            let there: Vec<&str> = lines[start..insert].iter().map(|l| l.trim()).collect();
            let mut add: Vec<String> = lead.iter().filter(|l| !there.contains(&l.trim())).cloned().collect();
            add.extend(body);
            // a blank line between the top's settings and the first table
            if own.is_none() && first_header == Some(insert) {
                add.push(String::new());
            }
            lines.splice(insert..insert, add);
        } else {
            // a table some key of the file already defines can't be given a header of its own
            let mut section: Vec<String> = vec![];
            for s in &stmts {
                if s.kind == StmtKind::Header {
                    section = s.path.clone();
                } else if s.kind == StmtKind::Kv && starts_with(&b.path, &[section.clone(), s.path.clone()].concat()) {
                    return Merge::Error(CANT_ADD.into());
                }
            }
            if lines.last().is_some_and(|l| !l.trim().is_empty()) {
                lines.push(String::new());
            }
            lines.extend(lead);
            lines.push(elines[b.header.expect("a header")].clone());
            lines.extend(body);
        }
    }
    let out = joined(&lines);

    // read back: the file's settings (without what was replaced) and the entry's, nothing else
    let got = match crate::toml::parse(&out) {
        Ok(t) => t,
        Err(e) => return Merge::Error(first_line(e)),
    };
    let mut want = mine;
    for p in &owned {
        remove_at(&mut want, p);
    }
    merge_into(&mut want, &adds);
    if !crate::toml::same_table(&got, &want) {
        return Merge::Error(CANT_ADD.into());
    }
    Merge::Text { text: out, replaced: taken.iter().map(|u| u.name.clone()).collect() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_keep_the_rest() {
        let text = "# mine\ntheme = \"nord\"\n\n[ui]\n# number = true\nsidebar = true\n\n[keys.normal]\nx = \"y\"\n";
        let t = set_toml_value(text, Some("ui"), "number", "false");
        assert_eq!(t, "# mine\ntheme = \"nord\"\n\n[ui]\nnumber = false\nsidebar = true\n\n[keys.normal]\nx = \"y\"\n");
        let t = set_toml_value(&t, None, "theme", "\"gruvbox\"");
        assert!(t.contains("theme = \"gruvbox\"\n"));
        let t = set_toml_value(&t, Some("ui"), "icons", "\"nerd\"");
        assert!(t.contains("sidebar = true\nicons = \"nerd\"\n\n[keys.normal]"), "{t}");
        let t = set_toml_value(&t, Some("engines.claude"), "allow", "[\"Bash(gh:*)\"]");
        assert!(t.ends_with("x = \"y\"\n\n[engines.claude]\nallow = [\"Bash(gh:*)\"]\n"), "{t}");
        let t = set_toml_value(&t, Some("keys.normal"), "<leader>n", "\":new \"");
        assert!(t.contains("\"<leader>n\" = \":new \""), "{t}");
        assert_eq!(set_toml_value("", None, "theme", "\"x\""), "theme = \"x\"\n");
    }
}
