//! Which projects' .vigent.toml may run commands (src/trust.ts).
//!
//! A project config can set the theme, templates, keys and worktree layout
//! freely. The parts that run programs (hooks, engine arguments, custom
//! engine commands, Claude Code permission rules, and commands and mappings
//! that can run a shell command, define commands or mappings, write a file
//! they name or start :serve) only count once you trust that file, like
//! direnv's `allow`: `:trust` in vigent, or `vigent trust` in the project.
//! Trust is for the file as it is now (its sha256, in
//! $XDG_STATE_HOME/vigent/trusted.json); if it changes, vigent asks again.
//!
//! The same `:trust` trusts the repository itself (its absolute path, in the
//! same store), which lets agent CLIs run there (src/workspace.ts): vigent runs
//! each agent CLI headless, and headless CLIs skip their own "trust this
//! folder?" question while they load the repository's .claude/, .pi/ and the
//! like. That lasts until `vigent trust --revoke`.

use std::path::{Path, PathBuf};

use crate::app::App;
use crate::config::{self, Config, LoadOptions, PROJECT_CONFIG};
use crate::keys::{KEY_MODES, KeyMode, Keymap, RhsValue};

#[cfg(test)]
thread_local! {
    /// a test's own trust store (tests run in parallel, so not $XDG_STATE_HOME)
    static TEST_STORE: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

fn state_file() -> PathBuf {
    #[cfg(test)]
    if let Some(f) = TEST_STORE.with(|s| s.borrow().clone()) {
        return f;
    }
    let base = std::env::var_os("XDG_STATE_HOME")
        .filter(|x| !x.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default().join(".local").join("state"));
    base.join("vigent").join("trusted.json")
}

/// sha256 of the text, as lowercase hex (node's createHash("sha256").digest("hex")).
pub fn sha256_hex(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc,
        0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
        0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut h: [u32; 8] = [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let mut msg = data.to_vec();
    let bits = (data.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bits.to_be_bytes());
    for chunk in msg.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([chunk[4 * i], chunk[4 * i + 1], chunk[4 * i + 2], chunk[4 * i + 3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *x = x.wrapping_add(y);
        }
    }
    h.iter().map(|x| format!("{x:08x}")).collect()
}

fn read() -> serde_json::Map<String, serde_json::Value> {
    std::fs::read_to_string(state_file()).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()).and_then(|v| v.as_object().cloned()).unwrap_or_default()
}

fn key(file: &Path) -> String {
    std::path::absolute(file).unwrap_or_else(|_| file.to_path_buf()).to_string_lossy().to_string()
}

/// Whether this file, with exactly this content, is trusted.
pub fn is_trusted(file: &Path, content: &str) -> bool {
    read().get(&key(file)).and_then(|v| v.as_str()) == Some(sha256_hex(content.as_bytes()).as_str())
}

fn write(all: serde_json::Map<String, serde_json::Value>) -> std::io::Result<()> {
    let f = state_file();
    if let Some(dir) = f.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = serde_json::to_string_pretty(&serde_json::Value::Object(all)).unwrap_or_default();
    std::fs::write(f, format!("{text}\n"))
}

/// Trust this file as it is now.
pub fn trust_file(file: &Path, content: &str) -> std::io::Result<()> {
    let mut all = read();
    all.insert(key(file), serde_json::Value::String(sha256_hex(content.as_bytes())));
    write(all)
}

// ---------------------------------------------------------------------
// the repository (src/workspace.ts, and the repository half of src/trust.ts)
// ---------------------------------------------------------------------

/// A trusted repository's entry in the store (a file's is its sha256).
const REPOSITORY: &str = "repository";

/// What a reopened agent waits for, shown in the sidebar instead of "paused".
pub const TRUST_HOLD: &str = "waiting for :trust";

/// The project files of agent CLIs that run code or change what an agent may do, as the message names them.
pub const AGENT_CONFIGS: [&str; 9] = [".claude/", ".mcp.json", ".pi/", "opencode.json", "opencode.jsonc", ".opencode/", ".gemini/", ".cursor/", ".codex/"];

/// Where you stand: the folder agents run in, whether it's a git repository, and whether it's trusted.
#[derive(Clone, Debug, PartialEq)]
pub struct Workspace {
    pub dir: PathBuf,
    pub repo: bool,
    pub trusted: bool,
}

impl Workspace {
    fn kind(&self) -> &'static str {
        if self.repo { "repository" } else { "folder" }
    }
}

/// Which of AGENT_CONFIGS this folder has.
pub fn agent_configs(dir: &Path) -> Vec<&'static str> {
    AGENT_CONFIGS
        .iter()
        .copied()
        .filter(|name| match std::fs::metadata(dir.join(name.trim_end_matches('/'))) {
            Ok(m) => if name.ends_with('/') { m.is_dir() } else { m.is_file() },
            Err(_) => false,
        })
        .collect()
}

/// What starting an agent says while the repository isn't trusted.
pub fn untrusted_message(w: &Workspace) -> String {
    let found = agent_configs(&w.dir);
    let list = if found.is_empty() { ".claude/, .pi/, …".to_string() } else { found.join(", ") };
    format!("this {} isn't trusted yet: its agent CLIs would load its own settings ({list}). :trust to allow it", w.kind())
}

/// The start screen's line while the repository isn't trusted.
pub fn untrusted_note(w: &Workspace) -> String {
    format!("this {} isn't trusted yet: :trust lets its agent CLIs run here", w.kind())
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default()
}

/// A folder no repository's settings come from: your home folder, else the temp folder.
pub fn neutral_dir() -> PathBuf {
    let h = home();
    if !h.as_os_str().is_empty() && h.is_dir() { h } else { std::env::temp_dir() }
}

fn same(a: &Path, b: &Path) -> bool {
    if key(a) == key(b) {
        return true;
    }
    matches!((std::fs::canonicalize(a), std::fs::canonicalize(b)), (Ok(x), Ok(y)) if x == y)
}

/// Your own folders are never a repository to trust: your home folder and your config folders.
fn yours(dir: &Path) -> bool {
    let h = home();
    let config = std::env::var_os("XDG_CONFIG_HOME").filter(|x| !x.is_empty()).map(PathBuf::from).unwrap_or_else(|| h.join(".config"));
    [h, config, config::config_dir()].iter().any(|d| !d.as_os_str().is_empty() && same(dir, d))
}

/// Whether agent CLIs may run in this repository (or folder, outside one).
pub fn is_repo_trusted(dir: &Path) -> bool {
    yours(dir) || read().get(&key(dir)).and_then(|v| v.as_str()) == Some(REPOSITORY)
}

/// Trust this repository: its agent CLIs may run, until revoked. Whether it wasn't trusted before.
pub fn trust_repo(dir: &Path) -> std::io::Result<bool> {
    if is_repo_trusted(dir) {
        return Ok(false);
    }
    let mut all = read();
    all.insert(key(dir), serde_json::Value::String(REPOSITORY.into()));
    write(all).map(|_| true)
}

/// Forget a repository's trust, and its .vigent.toml's. Whether either was trusted.
pub fn revoke_repo(dir: &Path) -> std::io::Result<bool> {
    let mut all = read();
    let keys = [key(dir), key(&std::path::absolute(dir).unwrap_or_else(|_| dir.to_path_buf()).join(PROJECT_CONFIG))];
    let mut any = false;
    for k in keys {
        any |= all.remove(&k).is_some();
    }
    if any {
        write(all)?;
    }
    Ok(any)
}

/// Everything trusted, in order: (path, whether it's a repository; else a .vigent.toml as it was when trusted).
pub fn trusted_entries() -> Vec<(String, bool)> {
    let mut v: Vec<(String, bool)> = read().into_iter().map(|(k, v)| (k, v.as_str() == Some(REPOSITORY))).collect();
    v.sort();
    v
}

/// Where you stand when vigent starts in `cwd`: the repository (or this folder, outside one), and whether it's trusted.
pub fn workspace_at(cwd: &Path, root: Option<&Path>) -> Workspace {
    let dir = std::path::absolute(root.unwrap_or(cwd)).unwrap_or_else(|_| root.unwrap_or(cwd).to_path_buf());
    let trusted = is_repo_trusted(&dir);
    Workspace { dir, repo: root.is_some(), trusted }
}

/// `:trust`: trust the repository, so its agent CLIs may run (agents that waited
/// for it start), and its .vigent.toml as it is now, if it has one. With nothing
/// left to trust, it says the repository is trusted (src/trust.ts executeTrust).
pub fn execute_trust(app: &mut App) -> String {
    let Some(w) = app.agents.workspace.clone() else {
        return execute_project_trust(app);
    };
    let newly = match trust_repo(&w.dir) {
        Ok(n) => n,
        Err(e) => return format!("can't trust this {}: {e}", w.kind()),
    };
    if let Some(x) = app.agents.workspace.as_mut() {
        x.trusted = true;
    }
    // the engines' own commands may run here now
    crate::engines::detect::set_probe_dir(None);
    let file = project_file(app.root.as_deref()).filter(|f| f.exists()).map(|_| execute_project_trust(app));
    // the agents that waited go once the file's arguments and hooks are in place
    app.release_trust_holds();
    match (newly, file) {
        (false, Some(f)) => f,
        (false, None) => format!("this {} is trusted", w.kind()),
        (true, Some(f)) => format!("trusted this {} · {f}", w.kind()),
        (true, None) => format!("trusted this {}", w.kind()),
    }
}

/// The project's .vigent.toml (at the repository root), if there's a repository.
pub fn project_file(root: Option<&Path>) -> Option<PathBuf> {
    root.map(|r| r.join(PROJECT_CONFIG))
}

/// What a project file holds back until it's trusted, read over `mine`: a mapping or command that
/// waits only because it calls one of your commands is named too.
fn waiting(content: &str, mine: Config) -> Vec<String> {
    config::load_str(content, PROJECT_CONFIG, LoadOptions { base: Some(mine), trusted: false, label: PROJECT_CONFIG }).gated
}

/// A copy of the config in use, with the commands you have now (:command and :delcommand included).
fn in_use(app: &App) -> Config {
    let mut c = app.config.clone();
    c.commands = crate::commands::windows::commands_of(app).clone();
    c
}

/// `:trust`: trust the project's .vigent.toml as it is now and use what it runs from now on.
/// Returns the message to show (src/trust.ts executeProjectTrust).
pub fn execute_project_trust(app: &mut App) -> String {
    let Some(file) = project_file(app.root.as_deref()).filter(|f| f.exists()) else {
        return format!("no {PROJECT_CONFIG} in this project");
    };
    let content = match std::fs::read_to_string(&file) {
        Ok(c) => c,
        Err(e) => return format!("can't read {PROJECT_CONFIG}: {e}"),
    };
    // only a file that doesn't parse is refused; other warnings never stopped a file from being trusted
    if let Err(e) = config::parse_toml(&content) {
        return format!("can't trust {PROJECT_CONFIG}: {e}");
    }
    // what waits, read over a copy of the config in use
    let gated = waiting(&content, in_use(app));
    if let Err(e) = trust_file(&file, &content) {
        return format!("can't trust {PROJECT_CONFIG}: {e}");
    }
    let base = std::mem::take(&mut app.config);
    let keys_before = base.keys.clone();
    app.config = config::load_str(&content, PROJECT_CONFIG, LoadOptions { base: Some(base), trusted: true, label: PROJECT_CONFIG }).config;
    // the mappings that waited for :trust work now
    apply_new_mappings(&mut app.keymap, &keys_before, &app.config.keys);
    // the commands that waited for :trust run now (alongside any you defined with :command)
    if app.windows.commands_loaded {
        for g in gated.iter().filter_map(|g| g.strip_prefix("commands.")) {
            if let Some((_, steps)) = app.config.commands.iter().find(|(n, _)| n == g).cloned() {
                match app.windows.commands.iter_mut().find(|(n, _)| n == g) {
                    Some(e) => e.1 = steps,
                    None => app.windows.commands.push((g.to_string(), steps)),
                }
            }
        }
    }
    crate::permissions::reload(app);
    if gated.is_empty() { format!("trusted {PROJECT_CONFIG}") } else { format!("trusted {PROJECT_CONFIG}: {} now run", gated.join(", ")) }
}

/// Map what changed between two configs' [keys] (`after` is `before` with a file's mappings added).
fn apply_new_mappings(keymap: &mut Keymap, before: &[(KeyMode, String, RhsValue)], after: &[(KeyMode, String, RhsValue)]) {
    let last = |keys: &[(KeyMode, String, RhsValue)], mode: KeyMode, lhs: &str| keys.iter().rev().find(|(m, l, _)| *m == mode && l == lhs).map(|(_, _, r)| r.clone());
    for mode in KEY_MODES {
        let mut seen: Vec<&str> = vec![];
        for (m, lhs, _) in after.get(before.len()..).unwrap_or_default() {
            if *m != mode || seen.contains(&lhs.as_str()) {
                continue;
            }
            seen.push(lhs);
            let was = last(before, mode, lhs);
            if let Some(now) = last(after, mode, lhs).filter(|now| was.as_ref() != Some(now)) {
                // a bad mapping: vigent warned about it at startup
                let _ = keymap.set(mode, lhs, &now);
            }
        }
    }
}

/// `vigent trust`: trust this repository (outside one, this folder), so its agent
/// CLIs may run, and its .vigent.toml as it is now. `--list` shows what's
/// trusted, `--revoke [folder]` forgets a repository's trust (src/cli.ts trustCommand).
pub fn command(args: &[String]) -> i32 {
    let cwd = std::env::current_dir().unwrap_or_default();
    match args.first().map(String::as_str) {
        Some("--list") => {
            let all = trusted_entries();
            if all.is_empty() {
                println!("nothing is trusted yet");
            }
            for (p, repo) in all {
                println!("{}  {p}", if repo { "repository" } else { "file      " });
            }
            return 0;
        }
        Some("--revoke") => {
            let from = args.get(1).map(|a| std::path::absolute(a).unwrap_or_else(|_| PathBuf::from(a))).unwrap_or_else(|| cwd.clone());
            let dir = if from.exists() { crate::worktree::repo_root(&from) } else { None }.unwrap_or(from);
            match revoke_repo(&dir) {
                Ok(true) => println!("revoked trust in {}", dir.display()),
                Ok(false) => println!("{} isn't trusted", dir.display()),
                Err(e) => {
                    eprintln!("vigent: {e}");
                    return 1;
                }
            }
            return 0;
        }
        Some(_) => {
            eprintln!("usage: vigent trust [--list | --revoke [folder]]");
            return 1;
        }
        None => {}
    }
    let root = crate::worktree::repo_root(&cwd);
    let dir = std::path::absolute(root.as_deref().unwrap_or(&cwd)).unwrap_or_else(|_| cwd.clone());
    if let Err(e) = trust_repo(&dir) {
        eprintln!("vigent: {e}");
        return 1;
    }
    println!("trusted {}", dir.display());
    let file = dir.join(PROJECT_CONFIG);
    if root.is_none() || !file.exists() {
        return 0;
    }
    let content = std::fs::read_to_string(&file).unwrap_or_default();
    // read over your config, as vigent starts
    let gated = waiting(&content, config::load(&config::config_path(), LoadOptions::default()).config);
    if let Err(e) = trust_file(&file, &content) {
        eprintln!("vigent: {e}");
        return 1;
    }
    if gated.is_empty() {
        println!("trusted {}", file.display());
    } else {
        println!("trusted {}: {} will run", file.display(), gated.join(", "));
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trust_maps_what_waited() {
        let project = "[keys.normal]\nj = \":!touch pwned<CR>\"\nK = \":colorscheme nord<CR>\"\n";
        let mine = config::load_str("[keys.normal]\nQ = \":!touch mine<CR>\"\n", "config.toml", LoadOptions::default()).config;
        let untrusted = config::load_str(project, PROJECT_CONFIG, LoadOptions { base: Some(mine), trusted: false, label: PROJECT_CONFIG });
        assert_eq!(untrusted.gated, ["keys.normal.j"]);
        let (mut keymap, warnings) = crate::keys::build_keymap(None, &untrusted.config.keys);
        assert!(warnings.is_empty());
        let j = |km: &Keymap| km.lookup(KeyMode::Normal, &["j".into()], false).exact.map(|b| b.rhs_text);
        assert_eq!(j(&keymap).as_deref(), Some("scroll.down"), "j scrolls, as built in");
        assert!(keymap.lookup(KeyMode::Normal, &["K".into()], false).exact.is_some_and(|b| b.user), "the safe one is mapped");
        let before = untrusted.config.keys.clone();
        let trusted = config::load_str(project, PROJECT_CONFIG, LoadOptions { base: Some(untrusted.config), trusted: true, label: PROJECT_CONFIG });
        assert!(trusted.gated.is_empty());
        apply_new_mappings(&mut keymap, &before, &trusted.config.keys);
        assert_eq!(j(&keymap).as_deref(), Some(":!touch pwned<CR>"), ":trust maps it");
    }

    #[test]
    fn the_message_names_what_waits_for_your_commands() {
        let project = "[commands]\nCheck = \"Test\"\n[keys.normal]\n\"<leader>t\" = \":Test<CR>\"\n\"<leader>h\" = \":Hi<CR>\"\n";
        let mut app = crate::app::tests::app();
        app.config = config::load_str("[commands]\nTest = \"!npm test\"\nHi = \"queue hi\"\n", "config.toml", LoadOptions::default()).config;
        assert_eq!(waiting(project, in_use(&app)), ["commands.Check", "keys.normal.<leader>t"]);
        assert!(waiting(project, Config::default()).is_empty(), "without your commands, nothing would be named");
        // what :command and :delcommand changed counts, as :Test runs it now
        crate::commands::exec(&mut app, "delcommand Test");
        crate::commands::exec(&mut app, "command! Hi !echo hi");
        assert_eq!(waiting(project, in_use(&app)), ["keys.normal.<leader>h"]);
    }

    /// A temp folder of its own, with this test's own trust store; git-initialized, with `files`.
    fn repo_with(name: &str, files: &[&str]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vigent-trust-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for f in files {
            let p = dir.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, "{}").unwrap();
        }
        TEST_STORE.with(|s| *s.borrow_mut() = Some(dir.join("state").join("trusted.json")));
        dir
    }

    #[test]
    fn the_message_names_the_agent_configs_the_repository_has() {
        let dir = repo_with("configs", &[".claude/settings.json", ".pi/extensions/x.ts", "opencode.json", ".codex/config.toml"]);
        assert_eq!(agent_configs(&dir), [".claude/", ".pi/", "opencode.json", ".codex/"]);
        let w = Workspace { dir: dir.clone(), repo: true, trusted: false };
        assert_eq!(untrusted_message(&w), "this repository isn't trusted yet: its agent CLIs would load its own settings (.claude/, .pi/, opencode.json, .codex/). :trust to allow it");
        let bare = repo_with("bare", &[]);
        assert_eq!(untrusted_message(&Workspace { dir: bare.clone(), repo: true, trusted: false }), "this repository isn't trusted yet: its agent CLIs would load its own settings (.claude/, .pi/, …). :trust to allow it");
        assert_eq!(untrusted_message(&Workspace { dir: bare.clone(), repo: false, trusted: false }), "this folder isn't trusted yet: its agent CLIs would load its own settings (.claude/, .pi/, …). :trust to allow it");
        assert_eq!(untrusted_note(&w), "this repository isn't trusted yet: :trust lets its agent CLIs run here");
        // a file named like a folder doesn't count, nor a folder named like a file
        let odd = repo_with("odd", &["opencode.json/x"]);
        std::fs::write(odd.join(".claude"), "").unwrap();
        assert!(agent_configs(&odd).is_empty());
        for d in [dir, bare, odd] {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    #[test]
    fn a_repositorys_trust_is_its_path_in_the_same_store() {
        let dir = repo_with("store", &[]);
        assert!(!is_repo_trusted(&dir));
        assert!(trust_repo(&dir).unwrap());
        assert!(!trust_repo(&dir).unwrap(), "already trusted");
        assert!(is_repo_trusted(&dir));
        let store: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join("state").join("trusted.json")).unwrap()).unwrap();
        assert_eq!(store[dir.to_string_lossy().as_ref()], "repository");
        trust_file(&dir.join(PROJECT_CONFIG), "x").unwrap();
        assert_eq!(trusted_entries(), [(dir.to_string_lossy().to_string(), true), (dir.join(PROJECT_CONFIG).to_string_lossy().to_string(), false)]);
        assert!(revoke_repo(&dir).unwrap());
        assert!(!is_repo_trusted(&dir) && !is_trusted(&dir.join(PROJECT_CONFIG), "x"), "the repository and its .vigent.toml");
        assert!(!revoke_repo(&dir).unwrap());
        assert!(is_repo_trusted(&home()), "your home folder is never a repository to trust");
        assert_eq!(workspace_at(&dir.join("sub"), None), Workspace { dir: dir.join("sub"), repo: false, trusted: false }, "outside a repository, the folder itself");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// An App in `dir`, a repository that isn't trusted.
    fn untrusted_app(dir: &Path) -> App {
        let mut app = crate::app::tests::app();
        app.cwd = dir.to_path_buf();
        app.root = Some(dir.to_path_buf());
        app.config.worktree.enabled = false;
        app.agents.workspace = Some(workspace_at(dir, Some(dir)));
        app
    }

    #[test]
    fn untrusted_starting_an_agent_runs_nothing_and_says_why_then_trust_lets_it() {
        let dir = repo_with("start", &[".claude/settings.json"]);
        let mut app = untrusted_app(&dir);
        let expected = "this repository isn't trusted yet: its agent CLIs would load its own settings (.claude/). :trust to allow it";
        assert!(app.start_agent("fix it", Default::default()).is_none());
        assert_eq!(app.message, (expected.to_string(), "ErrorMsg"));
        app.input = "fix the bug".into();
        app.submit();
        crate::commands::exec(&mut app, "new other task");
        crate::commands::exec(&mut app, "race the task");
        assert_eq!(app.message.0, expected);
        assert!(app.sessions.is_empty());
        assert_eq!(crate::engines::select::launch(&app, "claude").blocked.as_deref(), Some(expected));
        crate::commands::exec(&mut app, "trust");
        assert_eq!(app.message.0, "trusted this repository");
        assert_eq!(app.untrusted(), None);
        assert_eq!(crate::engines::select::launch(&app, "claude").blocked, None);
        assert!(app.start_agent("fix it", Default::default()).is_some());
        crate::commands::exec(&mut app, "trust");
        assert_eq!(app.message.0, "this repository is trusted", ":trust with nothing left to trust says so");
        // demo mode never waits
        let mut demo = untrusted_app(&repo_with("demo", &[]));
        demo.serve.demo = true;
        assert_eq!(demo.untrusted(), None);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_reopened_agent_waits_for_trust_and_its_cli_never_runs_before() {
        let dir = repo_with("restore", &[".pi/extensions/x.ts"]);
        let mut app = untrusted_app(&dir);
        let marker = dir.join("ran");
        let bin = dir.join("fake-claude");
        std::fs::write(&bin, format!("#!/bin/sh
pwd >> {}
", marker.display())).unwrap();
        std::process::Command::new("chmod").arg("755").arg(&bin).status().unwrap();
        let i = app.add_session(crate::session::Session::new(0, "from last time", "claude", dir.clone()), true);
        assert!(app.hold_until_trusted(i));
        assert_eq!(app.sessions[i].hold_reason.as_deref(), Some(TRUST_HOLD));
        app.input = "carry on".into();
        app.submit();
        assert_eq!(app.message.0, "this repository isn't trusted yet: its agent CLIs would load its own settings (.pi/). :trust to allow it");
        assert_eq!(app.sessions[i].queue, ["carry on"], "the prompt waits");
        // a turn sent straight to the session runs nothing either
        let (tx, _rx) = std::sync::mpsc::channel();
        let mut launch = crate::engines::select::launch(&app, "claude");
        launch.bin = bin.to_string_lossy().into_owned();
        app.sessions[i].run("sneaked in".into(), &launch, tx.clone());
        assert!(app.sessions[i].child.is_none());
        assert!(matches!(app.sessions[i].items.last(), Some(crate::session::Item::Error(e)) if e.starts_with("error: this repository isn't trusted yet")));
        // nor pi
        let p = app.add_session(crate::session::Session::new(0, "pi agent", "pi", dir.clone()), false);
        crate::engines::pi::warm(&mut app, p);
        assert!(app.sessions[p].pi.is_none());
        assert!(!marker.exists(), "the agent CLI never ran");
        // trusted: the hold goes (the queued prompt is sent), and a turn runs
        app.sessions[i].queue.clear();
        crate::commands::exec(&mut app, "trust");
        assert_eq!(app.message.0, "trusted this repository");
        assert_eq!(app.sessions[i].hold_reason, None);
        let mut launch = crate::engines::select::launch(&app, "claude");
        launch.bin = bin.to_string_lossy().into_owned();
        app.sessions[i].state = crate::session::State::Idle;
        app.sessions[i].run("go".into(), &launch, tx);
        let _ = app.sessions[i].child.take().expect("it runs now").wait();
        assert_eq!(std::fs::read_to_string(&marker).unwrap().trim(), dir.to_string_lossy());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn model_lists_run_outside_a_repository_you_havent_trusted() {
        let dir = repo_with("models", &[".pi/extensions/x.ts"]);
        let log = dir.join("pi-cwd");
        let bin = dir.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("pi"), format!("#!/bin/sh
pwd > {}
echo 'provider  model' >&2
echo 'anthropic  claude-x' >&2
", log.display())).unwrap();
        std::process::Command::new("chmod").arg("755").arg(bin.join("pi")).status().unwrap();
        let neutral = dir.join("neutral");
        std::fs::create_dir_all(&neutral).unwrap();
        let env: crate::engines::detect::Env = [("PATH".to_string(), format!("{}:/usr/bin:/bin", bin.display()))].into_iter().collect();
        // the probe folder is the process's: only this test sets it, and puts it back
        crate::engines::detect::set_probe_dir(Some(neutral.clone()));
        let models = crate::engines::choices::pi_models(&env);
        let ran_in = std::fs::read_to_string(&log).unwrap_or_default();
        crate::engines::detect::set_probe_dir(None);
        assert_eq!(models, [("anthropic".to_string(), "claude-x".to_string())]);
        assert_eq!(ran_in.trim(), neutral.to_string_lossy());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn sha256_matches_node() {
        assert_eq!(sha256_hex(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        let long = "a".repeat(200);
        assert_eq!(sha256_hex(long.as_bytes()), "c2a908d98f5df987ade41b5fce213067efbcc21ef2240212a41e54b5e7c28ae5");
    }
}
