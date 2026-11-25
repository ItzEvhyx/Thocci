//! Terminal color palette and capability detection (modeled after ezCMD).
//!
//! Colors automatically disable when `--plain` is passed, stdout is piped,
//! `NO_COLOR` is set, or `TERM=dumb`.

use std::io::IsTerminal;

pub const BOLD: &str = "\x1b[1m";
pub const DIM: &str = "\x1b[90m";
pub const CYAN_BOLD: &str = "\x1b[1;36m";
pub const GREEN_BOLD: &str = "\x1b[1;32m";
pub const YELLOW: &str = "\x1b[33m";
pub const AMBER_BOLD: &str = "\x1b[1;38;2;251;191;36m";
pub const MAGENTA_BOLD: &str = "\x1b[1;38;2;215;135;255m";
pub const WARNING_RED: &str = "\x1b[1;38;2;255;78;78m";
pub const CMD_COLOR: &str = "\x1b[1;38;2;150;208;251m";
pub const RESET: &str = "\x1b[0m";

/// Signature ice-blue to deep-cobalt vertical gradient for the ASCII banner.
pub const BANNER_GRADIENT: [&str; 6] = [
    "\x1b[1;38;2;165;220;253m",
    "\x1b[1;38;2;135;196;249m",
    "\x1b[1;38;2;105;172;244m",
    "\x1b[1;38;2;75;146;236m",
    "\x1b[1;38;2;45;118;222m",
    "\x1b[1;38;2;20;92;204m",
];

#[cfg(windows)]
fn init_windows_vt() -> bool {
    enable_ansi_support::enable_ansi_support().is_ok()
}

#[cfg(not(windows))]
fn init_windows_vt() -> bool {
    true
}

/// Enable ANSI color output only on interactive terminals unless disabled.
pub fn color_enabled(plain: bool) -> bool {
    if plain {
        return false;
    }
    if !std::io::stdout().is_terminal() {
        return false;
    }
    if std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty()) {
        return false;
    }
    if std::env::var("TERM").is_ok_and(|v| v == "dumb") {
        return false;
    }
    init_windows_vt()
}

/// Render a command in bold `#96d0fb` with `<placeholders>` highlighted in yellow.
pub fn colorize_command(cmd_text: &str, color: bool) -> String {
    if !color {
        return cmd_text.to_owned();
    }

    let mut out = String::with_capacity(cmd_text.len() + 24);
    let mut in_placeholder = false;
    let mut in_cmd = false;

    for ch in cmd_text.chars() {
        if ch == '<' {
            if in_cmd {
                out.push_str(RESET);
                in_cmd = false;
            }
            out.push_str(YELLOW);
            out.push('<');
            in_placeholder = true;
        } else if ch == '>' {
            out.push('>');
            out.push_str(RESET);
            in_placeholder = false;
        } else if in_placeholder {
            out.push(ch);
        } else {
            if !in_cmd {
                out.push_str(CMD_COLOR);
                in_cmd = true;
            }
            out.push(ch);
        }
    }

    if in_cmd || in_placeholder {
        out.push_str(RESET);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_disables_color() {
        assert!(!color_enabled(true));
        assert_eq!(
            colorize_command("thocci volume <0-100>", false),
            "thocci volume <0-100>"
        );
    }

    #[test]
    fn colorize_command_highlights_placeholders() {
        let styled = colorize_command("thocci volume <0-100>", true);
        assert!(styled.contains(CMD_COLOR));
        assert!(styled.contains(YELLOW));
        assert!(styled.contains("<0-100>"));
        assert!(styled.ends_with(RESET));
    }
}
