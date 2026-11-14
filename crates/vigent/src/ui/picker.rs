//! A list to pick from, vim-style (src/picker.ts ChoicePicker): `j`/`k`
//! (↓/↑, Ctrl-n/p, Tab) move, `g`/`G` jump, Enter picks, `q`/Esc closes.
//! `/` starts a search: type to filter (fuzzy, like pi-tui's), Esc ends it
//! (the filter stays; Esc again drops it, then closes). With `custom`, Enter
//! on a search that matches nothing picks the text itself (a model the list
//! doesn't know). Every picker is one of these: the `@` file picker, `:model`,
//! `:effort`, `:engine`, a refused tool's question, a pi extension's select,
//! and, with [`Live`], `:colorscheme`'s (src/picker.ts ThemePicker). It's open
//! in `App::windows.picker` (`actions::windows::open_picker`).

use ratatui::style::Style;
use ratatui::text::{Line, Span};

use super::text::{fit, width};
use crate::app::App;
use crate::theme::{Attr, Theme};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Choice {
    pub value: String,
    /// what the list shows (default: the value)
    pub label: Option<String>,
    pub note: Option<String>,
    /// a heading shown above each run of choices with the same group
    pub group: Option<String>,
}

impl Choice {
    pub fn new(value: &str, label: Option<&str>, note: Option<&str>) -> Choice {
        Choice { value: value.into(), label: label.map(str::to_string), note: note.map(str::to_string), group: None }
    }
    /// what the list shows
    pub fn shown(&self) -> &str {
        self.label.as_deref().unwrap_or(&self.value)
    }
}

/// Where the picker goes on screen.
#[derive(Clone, Copy, Debug)]
pub enum Place {
    Center { width: u16 },
    /// a completion menu: its bottom-left corner at (col, rows + offset_y)
    BottomLeft { col: u16, offset_y: i16, width: u16 },
}

pub type Done = Box<dyn FnOnce(&mut App, Option<String>)>;

/// What the theme picker shows after a theme's name: its colors (the background first), then its tags (dark, light, transparent).
pub struct Look {
    pub colors: Vec<String>,
    pub tags: String,
}

/// The ThemePicker of src/picker.ts (`:colorscheme`): every move previews the
/// name it lands on, a new filter previews its first match, Enter with nothing
/// matched closes as Esc does, each row shows the theme's colors, and the
/// filter is "the letters in order" rather than pi-tui's fuzzy score.
#[derive(Clone, Copy)]
pub struct Live {
    pub preview: fn(&mut App, &str),
    pub look: fn(&str) -> Option<Look>,
}

pub struct Picker {
    pub title: String,
    /// None while loading
    pub items: Option<Vec<Choice>>,
    pub current: Option<String>,
    pub custom: bool,
    pub query: String,
    pub searching: bool,
    pub index: usize,
    pub scroll: usize,
    pub rows: usize,
    pub place: Place,
    pub done: Option<Done>,
    pub back_out: Option<Box<dyn FnOnce(&mut App)>>,
    /// which picker a list loading in the background is for (`actions::windows::fill_picker`)
    pub id: u64,
    /// the theme picker's live preview
    pub live: Option<Live>,
}

impl Picker {
    pub fn new(title: &str, items: Option<Vec<Choice>>, current: Option<String>, custom: bool, done: Done) -> Picker {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let mut p = Picker {
            title: title.into(),
            items: None,
            current,
            custom,
            query: String::new(),
            searching: false,
            index: 0,
            scroll: 0,
            rows: 14,
            place: Place::Center { width: 72 },
            done: Some(done),
            back_out: None,
            id: NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            live: None,
        };
        if let Some(items) = items {
            p.set_items(items);
        }
        p
    }

    /// The list came (it may load in the background).
    pub fn set_items(&mut self, items: Vec<Choice>) {
        self.index = items.iter().position(|c| Some(&c.value) == self.current.as_ref()).unwrap_or(0);
        self.items = Some(items);
        self.keep_visible();
    }

    pub fn filtered(&self) -> Vec<Choice> {
        let items = self.items.clone().unwrap_or_default();
        if self.live.is_some() {
            if self.query.is_empty() {
                return items;
            }
            // the query's letters in order
            let q = self.query.to_lowercase();
            return items
                .into_iter()
                .filter(|c| {
                    let mut it = c.value.to_lowercase().chars().collect::<Vec<_>>().into_iter();
                    q.chars().all(|x| it.any(|y| y == x))
                })
                .collect();
        }
        if self.query.trim().is_empty() {
            return items;
        }
        fuzzy_filter(items, &self.query, |c| format!("{} {} {} {}", c.label.as_deref().unwrap_or(""), c.value, c.note.as_deref().unwrap_or(""), c.group.as_deref().unwrap_or("")))
    }

    /// The choices with their groups' headings between them: Err(heading) or Ok(index).
    fn rows_of(items: &[Choice]) -> Vec<Result<usize, String>> {
        let mut out = vec![];
        let mut group: Option<&String> = None;
        for (i, c) in items.iter().enumerate() {
            if let Some(g) = &c.group
                && Some(g) != group
            {
                out.push(Err(g.clone()));
            }
            group = c.group.as_ref();
            out.push(Ok(i));
        }
        out
    }

    fn keep_visible(&mut self) {
        let rows = Self::rows_of(&self.filtered());
        let Some(at) = rows.iter().position(|r| *r == Ok(self.index)) else {
            self.scroll = 0;
            return;
        };
        let top = if at > 0 && rows[at - 1].is_err() { at - 1 } else { at };
        if top < self.scroll {
            self.scroll = top;
        }
        if at >= self.scroll + self.rows {
            self.scroll = at + 1 - self.rows;
        }
        self.scroll = self.scroll.min(rows.len().saturating_sub(self.rows));
    }

    /// The picker's height on screen.
    pub fn height(&self) -> usize {
        self.rows + 4
    }

    pub fn lines(&self, w: usize, t: &Theme, icons: &str) -> Vec<Line<'static>> {
        let (tl, tr, bl, br, h, v, lt, rt, cur_mark, prompt) = match icons {
            "ascii" => ("+", "+", "+", "+", "-", "|", "+", "+", "*", ">"),
            "nerd" => ("╭", "╮", "╰", "╯", "─", "│", "├", "┤", "\u{f00c}", "\u{f002}"),
            _ => ("╭", "╮", "╰", "╯", "─", "│", "├", "┤", "●", "❯"),
        };
        let inner = w.max(20) - 2;
        let float = t.hl("NormalFloat");
        let bg = float.bg;
        let on_bg = |g: &str, b| Style { bg: b, ..t.hl(g) };
        let on = |g: &str| {
            let s = t.fg(g);
            match bg {
                Some(b) => s.bg(b),
                None => s,
            }
        };
        let border = on("FloatBorder");
        let row = |mut spans: Vec<Span<'static>>, used: usize, row_bg: Style| {
            let mut out = vec![Span::styled(v.to_string(), border)];
            out.append(&mut spans);
            out.push(Span::styled(" ".repeat(inner.saturating_sub(used)), row_bg));
            out.push(Span::styled(v.to_string(), border));
            Line::from(out)
        };
        let items = self.filtered();
        let title = fit(&format!(" {} ", self.title), inner.saturating_sub(2));
        let mut lines = vec![Line::from(vec![
            Span::styled(format!("{tl}{h}"), border),
            Span::styled(title.clone(), on("FloatTitle")),
            Span::styled(format!("{}{tr}", h.repeat(inner.saturating_sub(1 + width(&title)))), border),
        ])];
        if self.searching {
            let sel = t.hl("PmenuSel");
            lines.push(row(
                vec![Span::styled(format!(" {prompt} "), on("Title")), Span::styled(self.query.clone(), on("NormalFloat")), Span::styled(" ", sel)],
                width(&self.query) + 4,
                on("NormalFloat"),
            ));
        } else {
            let hint = if self.query.is_empty() { format!("/ to search{}", if self.custom { " or type any name" } else { "" }) } else { format!("/{}", self.query) };
            let text = fit(&hint, inner.saturating_sub(4));
            let used = width(&text) + 3;
            lines.push(row(vec![Span::styled(format!(" {prompt} {text}"), on("Comment"))], used, on("NormalFloat")));
        }
        lines.push(Line::from(Span::styled(format!("{lt}{}{rt}", h.repeat(inner)), border)));
        let all = Self::rows_of(&items);
        let name_w = (inner / 3).min(items.iter().map(|x| width(x.shown())).max().unwrap_or(0));
        for i in 0..self.rows {
            match all.get(self.scroll + i) {
                Some(Err(heading)) => {
                    let text = fit(&format!(" {heading}"), inner);
                    let n = width(&text);
                    lines.push(row(vec![Span::styled(text, on("Title"))], n, on("NormalFloat")));
                }
                Some(Ok(ci)) if self.live.is_some() => {
                    // a theme: its name, a strip of its own colors, then its tags
                    let c = &items[*ci];
                    let selected = *ci == self.index;
                    let row_bg = if selected { t.hl("PmenuSel").bg.or(t.hl("Visual").bg) } else { bg };
                    let group = if selected { "PmenuSel" } else { "NormalFloat" };
                    let marker = if Some(&c.value) == self.current.as_ref() { format!("{cur_mark} ") } else { "  ".into() };
                    let look = self.live.and_then(|l| (l.look)(&c.value));
                    let swatch_w = look.as_ref().map_or(0, |l| l.colors.len() * 2 + 1);
                    let tags = look.as_ref().map_or("", |l| l.tags.as_str());
                    let name_w = (inner.saturating_sub(2 + swatch_w + if tags.is_empty() { 0 } else { tags.len() + 1 })).clamp(8, 24);
                    let text = fit(&format!("{marker}{}", c.value), name_w);
                    let mut content = vec![Span::styled(format!(" {text}{} ", " ".repeat(name_w.saturating_sub(width(&text)))), on_bg(group, row_bg))];
                    let mut used = name_w + 2;
                    if let Some(l) = &look
                        && inner.saturating_sub(used) >= swatch_w
                    {
                        for c in &l.colors {
                            content.push(Span::styled("  ", t.style_of(&Attr { bg: Some(c.clone()), ..Default::default() })));
                        }
                        content.push(Span::styled(" ", on_bg(group, row_bg)));
                        used += swatch_w;
                        if !tags.is_empty() && inner.saturating_sub(used) >= tags.len() {
                            used += tags.len();
                            content.push(Span::styled(tags.to_string(), on_bg("Comment", row_bg)));
                        }
                    }
                    lines.push(row(content, used, on_bg("NormalFloat", row_bg)));
                }
                Some(Ok(ci)) => {
                    let c = &items[*ci];
                    let selected = *ci == self.index;
                    let sel = t.hl("PmenuSel");
                    let row_bg = if selected { sel } else { on("NormalFloat") };
                    let group = if selected { sel } else { on("NormalFloat") };
                    let marker = if Some(&c.value) == self.current.as_ref() { format!("{cur_mark} ") } else { "  ".into() };
                    let label = c.shown();
                    let padded = if c.note.is_some() { format!("{label}{}", " ".repeat(name_w.saturating_sub(width(label)))) } else { label.to_string() };
                    let name = fit(&format!("{marker}{padded}"), 10usize.max(inner.saturating_sub(4)));
                    let note = c.note.as_ref().map(|n| fit(&format!("  {n}"), inner.saturating_sub(2 + width(&name)))).unwrap_or_default();
                    let used = 1 + width(&name) + width(&note);
                    let note_style = if selected { sel } else { on("Comment") };
                    lines.push(row(vec![Span::styled(format!(" {name}"), group), Span::styled(note, note_style)], used, row_bg));
                }
                None => {
                    let text = if i == 0 && self.items.is_none() {
                        "  loading…".to_string()
                    } else if i == 0 && items.is_empty() {
                        let q = self.query.trim();
                        if self.custom && !q.is_empty() {
                            format!("  Enter uses '{q}'")
                        } else if self.custom && self.items.as_ref().is_some_and(|x| x.is_empty()) {
                            "  type it, then Enter".into()
                        } else {
                            "  no match".into()
                        }
                    } else {
                        String::new()
                    };
                    let text = fit(&text, inner);
                    let n = width(&text).min(inner);
                    lines.push(row(vec![Span::styled(text, on("Comment"))], n, on("NormalFloat")));
                }
            }
        }
        let keys = if self.searching { "type to filter · Esc done" } else { "j/k move · Enter pick · q close" };
        let count = format!(" {}/{} · {keys} ", if items.is_empty() { 0 } else { self.index + 1 }, items.len());
        lines.push(Line::from(vec![
            Span::styled(format!("{bl}{}", h.repeat(inner.saturating_sub(width(&count) + 1))), border),
            Span::styled(count, on("Comment")),
            Span::styled(format!("{h}{br}"), border),
        ]));
        lines
    }
}

/// What the picker does with a key: nothing more, or it's done.
pub enum Outcome {
    Stay,
    Done(Option<String>),
    BackOut,
    /// a live picker's selection changed: preview it
    Preview(String),
}

/// One key (in vigent's notation) for the picker.
pub fn feed(p: &mut Picker, key: &str) -> Outcome {
    let items = p.filtered();
    let n = items.len();
    let live = p.live.is_some();
    // the selection moved, or the filter changed under it (a live picker previews what it's on)
    let mut moved = false;
    let mv = |p: &mut Picker, by: isize| {
        if n > 0 {
            p.index = ((p.index as isize + by).rem_euclid(n as isize)) as usize;
        }
    };
    if key == "<C-c>" {
        return Outcome::Done(None);
    }
    if key == "<CR>" {
        let q = p.query.trim().to_string();
        let picked = items.get(p.index).map(|c| c.value.clone()).or(if p.custom && !q.is_empty() { Some(q) } else { None });
        return match picked {
            Some(v) => Outcome::Done(Some(v)),
            None if live => Outcome::Done(None),
            None => Outcome::Stay,
        };
    }
    if ["<Down>", "<Tab>", "<C-n>", "<C-j>"].contains(&key) {
        mv(p, 1);
        moved = true;
    } else if ["<Up>", "<S-Tab>", "<C-p>", "<C-k>"].contains(&key) {
        mv(p, -1);
        moved = true;
    } else if p.searching {
        match key {
            "<Esc>" => p.searching = false,
            "<BS>" | "<C-h>" => {
                if !p.query.is_empty() {
                    p.query.pop();
                    p.index = 0;
                    moved = true;
                } else if p.back_out.is_some() {
                    return Outcome::BackOut;
                } else {
                    p.searching = false;
                    if !live {
                        p.index = 0;
                    }
                }
            }
            "<C-u>" => {
                p.query.clear();
                p.index = 0;
                moved = true;
            }
            k => {
                if let Some(text) = crate::keys::token_text(k) {
                    p.query.push_str(&text);
                    p.index = 0;
                    moved = true;
                }
            }
        }
    } else {
        match key {
            "j" => mv(p, 1),
            "k" => mv(p, -1),
            "g" => p.index = 0,
            "G" => p.index = n.saturating_sub(1),
            "/" => p.searching = true,
            "q" => return Outcome::Done(None),
            "<Esc>" => {
                if p.query.is_empty() {
                    return Outcome::Done(None);
                }
                p.query.clear();
                p.index = 0;
            }
            _ => {}
        }
        moved = matches!(key, "j" | "k" | "g" | "G" | "<Esc>");
    }
    p.keep_visible();
    match p.filtered().get(p.index) {
        Some(c) if live && moved => Outcome::Preview(c.value.clone()),
        _ => Outcome::Stay,
    }
}

/// pi-tui's fuzzy match: every query character in order; lower scores are better.
pub fn fuzzy_match(query: &str, text: &str) -> Option<f64> {
    fn run(q: &[char], t: &[char]) -> Option<f64> {
        if q.is_empty() {
            return Some(0.0);
        }
        if q.len() > t.len() {
            return None;
        }
        let (mut qi, mut score, mut last, mut consecutive) = (0usize, 0f64, -1isize, 0f64);
        for (i, &c) in t.iter().enumerate() {
            if qi >= q.len() {
                break;
            }
            if c == q[qi] {
                let boundary = i == 0 || t[i - 1].is_whitespace() || matches!(t[i - 1], '-' | '_' | '.' | '/' | ':');
                if last == i as isize - 1 {
                    consecutive += 1.0;
                    score -= consecutive * 5.0;
                } else {
                    consecutive = 0.0;
                    if last >= 0 {
                        score += (i as isize - last - 1) as f64 * 2.0;
                    }
                }
                if boundary {
                    score -= 10.0;
                }
                score += i as f64 * 0.1;
                last = i as isize;
                qi += 1;
            }
        }
        if qi < q.len() {
            return None;
        }
        if q == t {
            score -= 100.0;
        }
        Some(score)
    }
    let q: Vec<char> = query.to_lowercase().chars().collect();
    let t: Vec<char> = text.to_lowercase().chars().collect();
    if let Some(s) = run(&q, &t) {
        return Some(s);
    }
    // "abc12" also finds "12abc", and the other way
    let letters: String = q.iter().take_while(|c| c.is_ascii_lowercase()).collect();
    let digits: String = q.iter().skip(letters.chars().count()).collect();
    let swapped: Option<String> = if !letters.is_empty() && !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) {
        Some(format!("{digits}{letters}"))
    } else {
        let d: String = q.iter().take_while(|c| c.is_ascii_digit()).collect();
        let l: String = q.iter().skip(d.chars().count()).collect();
        (!d.is_empty() && !l.is_empty() && l.chars().all(|c| c.is_ascii_lowercase())).then(|| format!("{l}{d}"))
    };
    let s: Vec<char> = swapped?.chars().collect();
    run(&s, &t).map(|x| x + 5.0)
}

/// Items whose text matches every space-separated token, best first.
pub fn fuzzy_filter<T>(items: Vec<T>, query: &str, text: impl Fn(&T) -> String) -> Vec<T> {
    let tokens: Vec<&str> = query.split_whitespace().collect();
    if tokens.is_empty() {
        return items;
    }
    let mut scored: Vec<(f64, usize, T)> = vec![];
    for (i, item) in items.into_iter().enumerate() {
        let t = text(&item);
        let mut total = 0.0;
        let mut all = true;
        for tok in &tokens {
            match fuzzy_match(tok, &t) {
                Some(s) => total += s,
                None => {
                    all = false;
                    break;
                }
            }
        }
        if all {
            scored.push((total, i, item));
        }
    }
    scored.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal).then(a.1.cmp(&b.1)));
    scored.into_iter().map(|(_, _, x)| x).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picker(items: &[&str]) -> Picker {
        Picker::new("Attach a file · shop", Some(items.iter().map(|s| Choice::new(s, None, None)).collect()), None, false, Box::new(|_, _| {}))
    }

    #[test]
    fn filter_move_pick() {
        let mut p = picker(&["README.md", "src/cart.js", "src/checkout.js"]);
        p.searching = true;
        for k in ["c", "a", "r", "t"] {
            feed(&mut p, k);
        }
        assert_eq!(p.filtered().iter().map(|c| c.value.as_str()).collect::<Vec<_>>(), vec!["src/cart.js"]);
        let t = crate::theme::load("default").unwrap();
        let text: String = p.lines(64, &t, "unicode").last().unwrap().spans.iter().map(|s| s.content.to_string()).collect();
        assert!(text.contains(" 1/1 · type to filter · Esc done "), "{text}");
        assert!(matches!(feed(&mut p, "<CR>"), Outcome::Done(Some(v)) if v == "src/cart.js"));
        let mut p = picker(&["a", "b"]);
        feed(&mut p, "j");
        assert_eq!(p.index, 1);
        feed(&mut p, "j");
        assert_eq!(p.index, 0);
        assert!(matches!(feed(&mut p, "q"), Outcome::Done(None)));
    }

    /// Keys as a terminal types them: Enter, Esc, else the character.
    fn press(p: &mut Picker, keys: &str) -> Option<Option<String>> {
        let mut last = None;
        for c in keys.chars() {
            let token = match c {
                '\r' => "<CR>".to_string(),
                '\x1b' => "<Esc>".to_string(),
                c => c.to_string(),
            };
            last = match feed(p, &token) {
                Outcome::Done(v) => Some(v),
                _ => None,
            };
        }
        last
    }

    fn plain(p: &Picker) -> String {
        let t = crate::theme::load("default").unwrap();
        p.lines(70, &t, "unicode").iter().map(|l| l.spans.iter().map(|s| s.content.to_string()).collect::<String>()).collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn letters_move_until_slash_searches() {
        let mut p = Picker::new("Model · 'racer'", Some(crate::engines::choices::claude_models()), Some("default".into()), true, Box::new(|_, _| {}));
        let s = plain(&p);
        assert!(s.contains("Model · 'racer' "));
        assert!(s.contains("● Default"));
        assert!(s.contains("Claude Opus 5.5    claude-opus-5-5") || s.contains("Claude Opus 5.5   claude-opus-5-5"), "{s}");
        press(&mut p, "opu");
        assert!(plain(&p).contains("j/k move"));
        press(&mut p, "jj");
        assert!(plain(&p).contains(" 3/5 "));
        press(&mut p, "k");
        assert!(plain(&p).contains(" 2/5 "));
        press(&mut p, "/opu");
        assert!(plain(&p).contains("type to filter · Esc done"));
        assert_eq!(press(&mut p, "\r"), Some(Some("claude-opus-5-5".into())));
    }

    #[test]
    fn esc_ends_the_search_then_drops_the_filter_then_closes() {
        let mut p = Picker::new("Model", Some(crate::engines::choices::claude_models()), Some("default".into()), false, Box::new(|_, _| {}));
        press(&mut p, "/hai");
        assert!(plain(&p).contains("haiku") && !plain(&p).contains("opus"));
        assert_eq!(press(&mut p, "\x1b"), None);
        assert!(plain(&p).contains("/hai") && plain(&p).contains("j/k move"));
        press(&mut p, "\x1b");
        assert!(plain(&p).contains("opus"));
        assert_eq!(press(&mut p, "q"), Some(None));
        let mut c = Picker::new("Model", Some(vec![]), Some(String::new()), true, Box::new(|_, _| {}));
        assert!(plain(&c).contains("/ to search or type any name"));
        press(&mut c, "/my-own-model");
        assert!(plain(&c).contains("Enter uses 'my-own-model'"));
        assert_eq!(press(&mut c, "\r"), Some(Some("my-own-model".into())));
    }

    #[test]
    fn fuzzy_scores() {
        assert!(fuzzy_match("ct", "src/cart.js").is_some());
        assert!(fuzzy_match("xyz", "src/cart.js").is_none());
        let sorted = fuzzy_filter(vec!["src/checkout.js", "cart.js"], "cart", |s| s.to_string());
        assert_eq!(sorted[0], "cart.js");
    }
}
