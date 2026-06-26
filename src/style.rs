//! Minimal ANSI styling, auto-disabled when stdout isn't a TTY or `NO_COLOR`
//! is set. (So `charm deploy` output piped through the git push stays clean.)

use std::io::IsTerminal;
use std::sync::OnceLock;

fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| {
        std::env::var_os("NO_COLOR").is_none() && std::io::stdout().is_terminal()
    })
}

fn paint(code: &str, s: &str) -> String {
    if enabled() {
        format!("\x1b[{code}m{s}\x1b[0m")
    } else {
        s.to_string()
    }
}

pub fn green(s: &str) -> String {
    paint("32", s)
}
pub fn red(s: &str) -> String {
    paint("31", s)
}
pub fn yellow(s: &str) -> String {
    paint("33", s)
}
pub fn bold(s: &str) -> String {
    paint("1", s)
}
pub fn dim(s: &str) -> String {
    paint("2", s)
}
