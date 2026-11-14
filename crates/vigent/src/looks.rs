//! Colors on screen: :colorscheme (and its picker with a live preview),
//! :theme save, :hi, :set, and the options that change how a theme is drawn
//! (transparent, termguicolors). src/app.ts openThemePicker, saveTheme,
//! execHighlight, applyColorMode; src/config.ts applySetCommand.

use crate::actions::windows::open_picker;
use crate::app::App;
use crate::config::{self, HiSpec, Transparent, UiOptions, Val};
use crate::theme::{self, Attr, ThemeDef};
use crate::ui::picker::{Choice, Live, Look, Picker, Place};
use crate::vimcolors::{HiCommand, parse_highlight_command};

/// The options now, applied to how the theme is drawn (24-bit color, transparency).
pub fn apply_ui(app: &mut App) {
    app.theme.truecolor = app.config.ui.termguicolors;
    app.theme.transparency = app.config.ui.transparent.clone();
}

/// [highlights] from config.toml, as overrides over any theme.
pub fn highlight_overrides(list: &[(String, HiSpec)]) -> Vec<(String, Attr)> {
    let mut out: Vec<(String, Attr)> = vec![];
    for (group, h) in list {
        let a = match h {
            HiSpec::Link(to) => Attr { link: Some(to.clone()), ..Default::default() },
            HiSpec::Command(s) => match parse_highlight_command(&format!("{group} {s}")) {
                Some(HiCommand::Set { attr, .. }) => attr,
                _ => continue,
            },
            HiSpec::Attr { fg, bg, link, bold, italic, underline, reverse } => Attr { fg: fg.clone(), bg: bg.clone(), link: link.clone(), bold: *bold, italic: *italic, underline: *underline, reverse: *reverse },
        };
        out.retain(|(g, _)| g != group);
        out.push((group.clone(), a));
    }
    out
}

/// The theme for a start: overrides from [highlights], drawn with [ui]'s options.
pub fn startup_theme(def: ThemeDef, config: &config::Config) -> theme::Theme {
    let mut t = theme::Theme::new(def);
    t.overrides = highlight_overrides(&config.highlights);
    t.truecolor = config.ui.termguicolors;
    t.transparency = config.ui.transparent.clone();
    t
}

/// Switch to a theme by name; the message says what happened.
pub fn set_theme(app: &mut App, name: &str) {
    match theme::load_def(name) {
        Ok(def) => {
            app.theme.set_def(def);
            let msg = format!("colorscheme {}", app.theme.name);
            app.say(msg, "ModeMsg");
        }
        Err(e) => app.say(e, "ErrorMsg"),
    }
}

/// `:colorscheme [name]`, `:theme save[!] [name]`
pub fn exec_theme(app: &mut App, cmd: &str, arg: &str) {
    if arg.is_empty() {
        return open_theme_picker(app);
    }
    if cmd == "theme"
        && let Some(rest) = arg.strip_prefix("save")
    {
        let (force, rest) = match rest.strip_prefix('!') {
            Some(r) => (true, r),
            None => (false, rest),
        };
        if rest.is_empty() || rest.starts_with(char::is_whitespace) {
            let name = rest.trim();
            if !name.contains(char::is_whitespace) {
                return save_theme(app, name, force);
            }
        }
    }
    set_theme(app, arg);
}

/// `:theme save <name>`: what's on screen, :hi changes included, as a theme file.
pub fn save_theme(app: &mut App, name: &str, force: bool) {
    if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '.' || c == '-') {
        return app.say("usage: :theme save <name>  (letters, digits, - _ .)", "ErrorMsg");
    }
    if theme::is_builtin(name) {
        return app.say(format!("'{name}' is a built-in theme; save under another name"), "ErrorMsg");
    }
    let file = theme::themes_dir().join(format!("{name}.toml"));
    let short = crate::settings::short_path(&file);
    if file.exists() && !force {
        return app.say(format!("E13: {short} exists (add ! to overwrite)"), "ErrorMsg");
    }
    let builtin_base = theme::is_builtin(&app.theme.def.name) && theme::load_def(&app.theme.def.name).map(|d| d.groups == app.theme.def.groups).unwrap_or(false);
    let text = theme::theme_file_for(name, &app.theme, builtin_base);
    let r = std::fs::create_dir_all(theme::themes_dir()).and_then(|_| std::fs::write(&file, text));
    match r.map_err(|e| e.to_string()).and_then(|_| theme::load_def(name)) {
        Ok(def) => {
            app.theme.set_def(def);
            app.say(format!("saved {short} · :colorscheme {name} (or theme = \"{name}\" in config.toml)"), "ModeMsg");
        }
        Err(e) => app.say(format!("couldn't save the theme: {e}"), "ErrorMsg"),
    }
}

fn describe_attr(a: &Attr) -> String {
    let mut parts = vec![];
    if let Some(fg) = &a.fg {
        parts.push(format!("guifg={fg}"));
    }
    if let Some(bg) = &a.bg {
        parts.push(format!("guibg={bg}"));
    }
    let flags: Vec<&str> = [("bold", a.bold), ("italic", a.italic), ("underline", a.underline), ("reverse", a.reverse)].iter().filter(|(_, v)| *v == Some(true)).map(|(k, _)| *k).collect();
    if !flags.is_empty() {
        parts.push(format!("gui={}", flags.join(",")));
    }
    if parts.is_empty() { "cleared".into() } else { parts.join(" ") }
}

/// `:hi Group guifg=… guibg=… gui=…`, `:hi link A B`, `:hi clear [Group]`, `:hi Group`
pub fn exec_highlight(app: &mut App, arg: &str) {
    let Some(cmd) = parse_highlight_command(arg) else {
        return app.say("usage: :hi Group guifg=#rrggbb guibg=#rrggbb gui=bold | :hi link A B | :hi clear", "Comment");
    };
    match cmd {
        HiCommand::Show { group } => {
            let a = app.theme.attr(&group);
            return app.say(format!("{group} xxx {}", describe_attr(&a)), "Normal");
        }
        HiCommand::Set { group, attr, .. } => app.theme.highlight(&group, &attr),
        HiCommand::Link { from, to } => app.theme.link(&from, &to),
        HiCommand::Clear { group } => app.theme.clear(group.as_deref()),
    }
    app.say("", "Normal");
}

// ---------------------------------------------------------------------------
// :set
// ---------------------------------------------------------------------------

fn show(ui: &UiOptions, key: &str) -> String {
    let b = |name: &str, v: bool| if v { format!("  {name}") } else { format!("no{name}") };
    match key {
        "sidebar" => b(key, ui.sidebar),
        "number" => b(key, ui.number),
        "tilde" => b(key, ui.tilde),
        "termguicolors" => b(key, ui.termguicolors),
        "whichkey" => b(key, ui.whichkey),
        "sidebar_width" => format!("  {key}={}", ui.sidebar_width),
        "sidebar_position" => format!("  {key}={}", ui.sidebar_position),
        "icons" => format!("  {key}={}", ui.icons),
        "timeoutlen" => format!("  {key}={}", ui.timeoutlen),
        "notify" => format!("  {key}={}", ui.notify),
        "transparent" => match ui.transparent {
            Transparent::On => b(key, true),
            Transparent::Off => b(key, false),
            Transparent::Auto => format!("  {key}=auto"),
        },
        _ => String::new(),
    }
}

/// The [ui] options in the order :set lists them (src/config.ts UiOptions).
const UI_KEYS: [&str; 11] = ["sidebar", "sidebar_width", "sidebar_position", "number", "tilde", "icons", "timeoutlen", "termguicolors", "transparent", "whichkey", "notify"];

fn is_bool(key: &str) -> bool {
    matches!(key, "sidebar" | "number" | "tilde" | "termguicolors" | "whichkey")
}

fn set_bool(ui: &mut UiOptions, key: &str, v: bool) {
    match key {
        "sidebar" => ui.sidebar = v,
        "number" => ui.number = v,
        "tilde" => ui.tilde = v,
        "termguicolors" => ui.termguicolors = v,
        "whichkey" => ui.whichkey = v,
        "transparent" => ui.transparent = if v { Transparent::On } else { Transparent::Off },
        _ => {}
    }
}

fn get_bool(ui: &UiOptions, key: &str) -> bool {
    match key {
        "sidebar" => ui.sidebar,
        "number" => ui.number,
        "tilde" => ui.tilde,
        "termguicolors" => ui.termguicolors,
        _ => ui.whichkey,
    }
}

fn word(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_alphanumeric() || c == '_')
}

/// vim's :set over [ui]: `opt`, `noopt`, `opt!`, `invopt`, `opt=val`, `opt?`, `opt&`, and none (list them).
/// `now_transparent` is what's on screen, for toggling an option that follows the theme.
/// Returns the message and whether it's an error.
pub fn apply_set(ui: &mut UiOptions, arg: &str, now_transparent: bool) -> (String, bool) {
    if arg.trim().is_empty() {
        return (UI_KEYS.iter().map(|k| show(ui, k)).collect::<Vec<_>>().join("  "), false);
    }
    let mut shown = vec![];
    for part in arg.split_whitespace() {
        if let Some(name) = part.strip_suffix('?').filter(|n| word(n)) {
            let Some(key) = config::ui_option(name) else { return (format!("E518: Unknown option: {name}"), true) };
            shown.push(show(ui, key));
            continue;
        }
        if let Some((name, value)) = part.split_once('=').filter(|(n, _)| word(n)) {
            if let Err(e) = config::set_option(ui, name, &Val::Str(value.to_string())) {
                return (format!("E518: {e}"), true);
            }
            continue;
        }
        if let Some(name) = part.strip_suffix('!').filter(|n| word(n)).or_else(|| part.strip_prefix("inv").filter(|n| word(n))) {
            let key = config::ui_option(name);
            if key == Some("transparent") {
                let now = match ui.transparent {
                    Transparent::Auto => now_transparent,
                    Transparent::On => true,
                    Transparent::Off => false,
                };
                set_bool(ui, "transparent", !now);
                continue;
            }
            match key {
                Some(k) if is_bool(k) => {
                    let v = get_bool(ui, k);
                    set_bool(ui, k, !v);
                }
                _ => return (format!("E518: Unknown option: {part}"), true),
            }
            continue;
        }
        if let Some(name) = part.strip_suffix('&').filter(|n| word(n)) {
            let Some(key) = config::ui_option(name) else { return (format!("E518: Unknown option: {name}"), true) };
            let d = UiOptions::default();
            match key {
                "sidebar_width" => ui.sidebar_width = d.sidebar_width,
                "sidebar_position" => ui.sidebar_position = d.sidebar_position,
                "icons" => ui.icons = d.icons,
                "timeoutlen" => ui.timeoutlen = d.timeoutlen,
                "notify" => ui.notify = d.notify,
                "transparent" => ui.transparent = d.transparent,
                k => set_bool(ui, k, get_bool(&d, k)),
            }
            continue;
        }
        let off = part.strip_prefix("no").filter(|n| config::ui_option(n).is_some());
        let Some(key) = config::ui_option(off.unwrap_or(part)) else { return (format!("E518: Unknown option: {part}"), true) };
        if !is_bool(key) && key != "transparent" {
            shown.push(show(ui, key));
            continue;
        }
        set_bool(ui, key, off.is_none());
    }
    (shown.join("  "), false)
}

/// `:set ...`
pub fn exec_set(app: &mut App, arg: &str) {
    let now = app.theme.transparent();
    let (msg, err) = apply_set(&mut app.config.ui, arg, now);
    apply_ui(app);
    app.say(msg, if err { "ErrorMsg" } else { "Normal" });
}

// ---------------------------------------------------------------------------
// the theme picker
// ---------------------------------------------------------------------------

/// `:colorscheme` with no name: pick a theme with a live preview (src/picker.ts ThemePicker).
pub fn open_theme_picker(app: &mut App) {
    let original = app.theme.def.clone();
    let items = theme::list().iter().map(|t| Choice::new(t, None, None)).collect();
    let mut p = Picker::new(
        "Colorschemes",
        Some(items),
        Some(app.theme.name.clone()),
        false,
        Box::new(move |app, pick| match pick {
            Some(name) => {
                preview(app, &name);
                app.say(format!("colorscheme {name}"), "ModeMsg");
            }
            None => app.theme.set_def(original),
        }),
    );
    p.place = Place::Center { width: 64 };
    p.live = Some(Live { preview, look });
    open_picker(app, p);
}

fn preview(app: &mut App, name: &str) {
    match theme::load_def(name) {
        Ok(d) => {
            app.theme.set_def(d);
            app.say("", "Normal");
        }
        Err(e) => app.say(e, "ErrorMsg"),
    }
}

/// A theme's colors and tags in the picker.
fn look(name: &str) -> Option<Look> {
    let d = theme::load_def(name).ok()?;
    let pal = theme::extract_palette(&d);
    let colors = ["bg", "red", "yellow", "green", "cyan", "blue", "magenta"].iter().map(|k| pal.get(k).unwrap_or("#000000").to_string()).collect();
    let mut tags = vec![if d.background.as_deref() == Some("light") { "light" } else { "dark" }];
    if d.transparent {
        tags.push("transparent");
    }
    Some(Look { colors, tags: tags.join(" ") })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_like_vim() {
        let mut ui = UiOptions::default();
        assert_eq!(apply_set(&mut ui, "nonumber sbw=40", false), (String::new(), false));
        assert!(!ui.number);
        assert_eq!(ui.sidebar_width, 40);
        apply_set(&mut ui, "sbp=right", false);
        assert_eq!(ui.sidebar_position, "right");
        assert!(apply_set(&mut ui, "sidebar_position?", false).0.contains("sidebar_position=right"));
        apply_set(&mut ui, "sidebar_position&", false);
        assert_eq!(ui.sidebar_position, "left");
        apply_set(&mut ui, "number!", false);
        assert!(ui.number);
        apply_set(&mut ui, "invtransparent", true);
        assert_eq!(ui.transparent, Transparent::Off, "following the theme: flip what's on screen");
        assert_eq!(apply_set(&mut ui, "bogus", false), ("E518: Unknown option: bogus".into(), true));
        assert!(apply_set(&mut ui, "sbw=3", false).0.starts_with("E518: 'sbw' should be a number"));
        let all = apply_set(&mut ui, "", false).0;
        assert!(all.starts_with("  sidebar    sidebar_width=40"), "{all}");
    }

    use crate::actions::windows::picker_key;

    #[test]
    fn hi_and_themes() {
        let mut a = crate::app::tests::app();
        exec_highlight(&mut a, "Comment guifg=#ff0000");
        assert_eq!(a.theme.attr("Comment").fg.as_deref(), Some("#ff0000"));
        exec_theme(&mut a, "colorscheme", "nord");
        assert_eq!(a.theme.name, "nord");
        assert_eq!(a.theme.attr("Comment").fg.as_deref(), Some("#ff0000"), ":hi survives a colorscheme change");
        exec_highlight(&mut a, "clear");
        exec_theme(&mut a, "theme", "save default");
        assert!(a.message.0.contains("built-in"));
        exec_theme(&mut a, "colorscheme", "");
        picker_key(&mut a, "j");
        assert_ne!(a.theme.name, "nord", "j previews the next one");
        picker_key(&mut a, "q");
        assert_eq!(a.theme.name, "nord", "q puts the old one back");
        assert!(a.windows.picker.is_none());
    }
}
