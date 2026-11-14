//! vigent as one native binary (the Rust port, #168): a vim-native screen for
//! the coding-agent CLIs you have installed, each agent in its own riftri
//! worktree. PORTING.md maps every module to its TypeScript original and lists
//! what's left to port, by area.

// The port fills these modules in area by area; until then some of what they
// declare (the TypeScript vigent's whole shape) has no caller yet.
#![allow(dead_code)]

mod actions;
mod agents;
mod app;
mod attach;
mod cli;
mod color;
mod commands;
mod completion;
mod config;
mod config_edit;
mod engines;
mod follow;
mod health;
mod help;
mod import;
mod keys;
mod line_edit;
mod looks;
mod names;
mod permissions;
mod printable;
mod race;
mod records;
mod registry;
mod review;
mod search;
mod serve;
mod session;
mod settings;
mod style;
mod suggest;
mod templates;
mod theme;
mod themecli;
mod toml;
mod tree;
mod trust;
mod tui;
mod ui;
mod vimcolors;
mod windows;
mod worktree;

fn main() {
    quiet_on_broken_pipe();
    std::process::exit(cli::main(std::env::args().skip(1).collect()));
}

/// `vigent theme list | head`: when what reads the output stops early, the next
/// print!/println! panics on the closed pipe, and panic = "abort" turns that into
/// an abort with a message. End quietly with 0 instead, like ripgrep, so a
/// pipeline under `set -o pipefail` doesn't fail on it. The screen writes through
/// ratatui, which returns errors instead of panicking, so this only covers the
/// commands that print and exit.
fn quiet_on_broken_pipe() {
    let next = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let payload = info.payload();
        let msg = payload.downcast_ref::<String>().map(String::as_str).or_else(|| payload.downcast_ref::<&str>().copied()).unwrap_or("");
        if msg.starts_with("failed printing to std") && msg.contains("Broken pipe") {
            std::process::exit(0);
        }
        next(info);
    }));
}
