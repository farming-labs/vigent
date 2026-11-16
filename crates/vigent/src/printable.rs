//! An agent's text made safe to draw inside a window (src/printable.ts):
//! a terminal acts on control characters instead of showing them, so tabs
//! become two spaces, a lone `\r` keeps what came after it (what a progress
//! bar ends up showing), and every escape sequence, control character (C0,
//! DEL, C1) and bidi control is dropped. (The TypeScript keeps colors in the
//! string; here text is styled by spans, so they go too: this is its
//! plainText.) And `one_row`: a screen row as the terminal must get it
//! (src/app.ts oneRow).

/// Text with no escape sequences or control characters but newlines.
pub fn printable(text: &str) -> String {
    if !text.chars().any(is_control) {
        return text.to_string();
    }
    let text = text.replace("\r\n", "\n");
    let stripped = strip_escapes(&text);
    let mut out = String::with_capacity(stripped.len());
    for (i, line) in stripped.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let line = match line.rfind('\r') {
            Some(at) => &line[at + 1..],
            None => line,
        };
        for c in line.chars() {
            match c {
                '\t' => out.push_str("  "),
                c if is_control(c) => {}
                c => out.push(c),
            }
        }
    }
    out
}

fn is_control(c: char) -> bool {
    let n = c as u32;
    (n <= 0x1f && c != '\n') || (0x7f..=0x9f).contains(&n) || is_bidi(c)
}

/// The bidi controls, which reorder what a row shows.
fn is_bidi(c: char) -> bool {
    matches!(c, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
}

/// A control character a terminal acts on (tabs and line ends aside), or a
/// bidi control: what config values and names may not hold (src/printable.ts CONTROL).
pub fn is_unsafe(c: char) -> bool {
    let n = c as u32;
    (n <= 0x1f && !matches!(c, '\t' | '\n' | '\r')) || (0x7f..=0x9f).contains(&n) || is_bidi(c)
}

/// `printable` on one line: runs of whitespace become one space (src/printable.ts oneLine).
pub fn one_line(text: &str) -> String {
    printable(text).split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Text for a plain terminal with every character it would act on written
/// out instead: `\x1b`, `\x9b`, `\r`, `\u202e`. Tabs and newlines stay
/// (src/printable.ts escapeControls).
pub fn escape_controls(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        let n = c as u32;
        if c == '\r' {
            out.push_str("\\r");
        } else if !is_unsafe(c) {
            out.push(c);
        } else if n < 0x100 {
            out.push_str(&format!("\\x{n:02x}"));
        } else {
            out.push_str(&format!("\\u{n:04x}"));
        }
    }
    out
}

/// CSI, OSC, DCS/SOS/PM/APC strings and two-byte escapes, removed.
pub fn strip_escapes(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '\x1b' {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        i += 1;
        match chars.get(i) {
            Some('[') => {
                i += 1;
                while i < chars.len() && ('0'..='?').contains(&chars[i]) {
                    i += 1;
                }
                while i < chars.len() && (' '..='/').contains(&chars[i]) {
                    i += 1;
                }
                if i < chars.len() && ('@'..='~').contains(&chars[i]) {
                    i += 1;
                }
            }
            Some(']') => {
                i += 1;
                while i < chars.len() && chars[i] != '\x07' && chars[i] != '\x1b' {
                    i += 1;
                }
                if chars.get(i) == Some(&'\x07') {
                    i += 1;
                } else if chars.get(i) == Some(&'\x1b') && chars.get(i + 1) == Some(&'\\') {
                    i += 2;
                }
            }
            Some('P' | 'X' | '^' | '_') => {
                i += 1;
                while i < chars.len() && chars[i] != '\x1b' {
                    i += 1;
                }
                if chars.get(i + 1) == Some(&'\\') {
                    i += 2;
                }
            }
            _ => {
                while i < chars.len() && (' '..='/').contains(&chars[i]) {
                    i += 1;
                }
                if i < chars.len() && ('0'..='~').contains(&chars[i]) {
                    i += 1;
                }
            }
        }
    }
    out
}

/// A line of a file (a diff's) as vigent draws it: what a terminal would act
/// on is written out (`\x1b]52;…`), so you see what the file holds; tabs are
/// two spaces and a CRLF line's `\r` goes (src/printable.ts fileText).
pub fn file_text(line: &str) -> String {
    escape_controls(line.strip_suffix('\r').unwrap_or(line)).replace('\t', "  ")
}

/// Every escape sequence and control character gone, newlines and tabs
/// aside: what's left of a command once nothing in it can be a terminal's
/// (src/printable.ts withoutEscapes).
pub fn without_escapes(text: &str) -> String {
    strip_escapes(text).chars().filter(|&c| matches!(c, '\n' | '\t') || !(is_unsafe(c) || c == '\r')).collect()
}

/// One row: newlines, carriage returns and tabs become spaces; other cursor movers go.
pub fn one_row(line: &str) -> String {
    if !line.chars().any(|c| (c as u32) < 0x20 || c == '\x7f') {
        return line.to_string();
    }
    let line = line.replace("\r\n", " ");
    line.chars()
        .filter_map(|c| match c {
            '\n' | '\r' | '\t' => Some(' '),
            c if ((c as u32) < 0x20 && c != '\x1b' && c != '\x07') || c == '\x7f' => None,
            c => Some(c),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controls_go_text_stays() {
        assert_eq!(printable("plain"), "plain");
        assert_eq!(printable("a\tb"), "a  b");
        assert_eq!(printable("progress 10%\rprogress 100%\nnext"), "progress 100%\nnext");
        assert_eq!(printable("\x1b[31mred\x1b[0m\r\n\x1b]8;;http://x\x07link\x1b]8;;\x07"), "red\nlink");
        assert_eq!(printable("a\x1b[2Kb\x07c"), "abc");
        assert_eq!(one_row("cat <<EOF\nmulti\tx\r"), "cat <<EOF multi x ");
    }

    #[test]
    fn links_c1_and_bidi_go() {
        // a link's text stays, never its target; C1 can't end one early and start an OSC 52
        assert_eq!(printable("\x1b]8;;file:///etc/passwd\x07https://github.com\x1b]8;;\x07"), "https://github.com");
        assert_eq!(printable("\x1b]8;;http://a\u{9c}\u{9d}52;c;aGFja2Vk\x07after"), "after");
        assert_eq!(printable("\u{9d}52;c;aGk=\u{9c}"), "52;c;aGk=");
        assert_eq!(printable("abc\u{202e}dcba\u{2066}x\u{200f}"), "abcdcbax");
        assert_eq!(printable("\x1b[8mhidden\x1b[0m"), "hidden");
        assert_eq!(one_line("m-MODEL\x1b]0;T\x07\u{9d}0;C\u{9c} \n x"), "m-MODEL0;C x");
    }

    #[test]
    fn escaped_for_a_plain_terminal() {
        assert_eq!(escape_controls("ok\tname\n"), "ok\tname\n");
        assert_eq!(escape_controls("a\x1b]52;c;eA==\x07b\u{9b}2J\u{202e}"), "a\\x1b]52;c;eA==\\x07b\\x9b2J\\u202e");
        assert_eq!(file_text("+\tx \x1b]52;c;eA==\x07 a\rb\r"), "+  x \\x1b]52;c;eA==\\x07 a\\rb");
        assert!(is_unsafe('\x1b') && is_unsafe('\u{85}') && is_unsafe('\u{2067}') && !is_unsafe('\n') && !is_unsafe('é'));
    }
}
