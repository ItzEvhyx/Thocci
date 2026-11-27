//! Colorized terminal guide, welcome banner, and formatted CLI views.

use std::io::{self, Write};

use thocci_core::client::DaemonStatus;
use thocci_core::paths;
use thocci_core::soundpack::{self, DEFAULT_PACK_ID};

use crate::term::{self, colorize_command};

const BANNER_LINES: [&str; 7] = [
    r"  /$$$$$$$$ /$$                                     /$$",
    r" |__  $$__/| $$                                    |__/",
    r"    | $$   | $$$$$$$   /$$$$$$   /$$$$$$$  /$$$$$$$ /$$",
    r"    | $$   | $$__  $$ /$$__  $$ /$$_____/ /$$_____/| $$",
    r"    | $$   | $$  \ $$| $$  \ $$| $$      | $$      | $$",
    r"    | $$   | $$  | $$|  $$$$$$/|  $$$$$$$|  $$$$$$$| $$",
    r"    |__/   |__/  |__/ \______/  \_______/ \_______/|__/",
];

const BOX_INNER_WIDTH: usize = 78;

/// Print the full color-coded Terminal Guide (`thocci`, `thocci --help`, `thocci guide`).
pub fn print_guide<W: Write>(w: &mut W, color: bool) -> io::Result<()> {
    print_banner(w, color)?;

    let pack_count = soundpack::BUILTIN_PACKS.len();
    if color {
        writeln!(
            w,
            "  {}Thocci v{}{} {}— Mechanical keyboard sounds in Rust.{}",
            term::BOLD,
            thocci_core::VERSION,
            term::RESET,
            term::CMD_COLOR,
            term::RESET
        )?;
        writeln!(
            w,
            "  {}{} built-in soundpacks embedded (v1 & v2 press+release) — zero external deps.{}\n",
            term::DIM,
            pack_count,
            term::RESET
        )?;
    } else {
        writeln!(
            w,
            "  Thocci v{} — Mechanical keyboard sounds in Rust.",
            thocci_core::VERSION
        )?;
        writeln!(
            w,
            "  {pack_count} built-in soundpacks embedded (v1 & v2 press+release) — zero external deps.\n"
        )?;
    }

    print_command_box(
        w,
        "ENGINE CONTROLS",
        term::CYAN_BOLD,
        &[
            ("thocci on", "Start keystroke sounds in the background"),
            (
                "thocci on --volume <0-100>",
                "Start at a specific initial volume",
            ),
            (
                "thocci on --pack <dir>",
                "Start using a custom soundpack folder",
            ),
            ("thocci off", "Stop the background daemon"),
            ("thocci toggle", "Flip on/off (ideal for a hotkey)"),
            ("thocci status", "Show running state, PID, volume & pack"),
        ],
        color,
    )?;

    print_command_box(
        w,
        "VOLUME & SOUNDPACKS",
        term::MAGENTA_BOLD,
        &[
            (
                "thocci volume <0-100>",
                "Adjust volume live (or save while off)",
            ),
            ("thocci packs", "List built-in and imported soundpacks"),
            ("thocci <pack>", "Switch live by id, name, or partial match"),
            ("thocci pack <query>", "Explicit pack switch command"),
            ("thocci cherry-mx", "Switch to Cherry MX (default pack)"),
            ("thocci banana-split", "Switch to Banana Split pack"),
        ],
        color,
    )?;

    print_examples(w, color)?;
    print_setup_box(w, color)?;
    print_footer(w, color)?;
    Ok(())
}

/// Print the post-install welcome screen (`thocci --welcome`).
pub fn print_welcome<W: Write>(w: &mut W, color: bool) -> io::Result<()> {
    print_banner(w, color)?;
    if color {
        writeln!(
            w,
            "  {}Welcome to Thocci!{} {}Mechanical keyboard sounds for Linux, Windows & macOS.{}",
            term::BOLD,
            term::RESET,
            term::CMD_COLOR,
            term::RESET
        )?;
        writeln!(
            w,
            "  {}Run `thocci --help` or `thocci guide` anytime for the full terminal guide.{}\n",
            term::DIM,
            term::RESET
        )?;
    } else {
        writeln!(
            w,
            "  Welcome to Thocci! Mechanical keyboard sounds for Linux, Windows & macOS."
        )?;
        writeln!(
            w,
            "  Run `thocci --help` or `thocci guide` anytime for the full terminal guide.\n"
        )?;
    }

    print_command_box(
        w,
        "QUICK START",
        term::CYAN_BOLD,
        &[
            ("thocci on", "Start keystroke sounds"),
            ("thocci off", "Stop keystroke sounds"),
            ("thocci volume <0-100>", "Set volume live"),
            ("thocci packs", "Browse the soundpack catalog"),
            ("thocci <pack>", "Switch live (e.g. thocci creamy)"),
            ("thocci guide", "Open the full terminal guide"),
        ],
        color,
    )?;

    print_footer(w, color)?;
    Ok(())
}

/// Format `thocci status` with clear visual state indicators.
pub fn print_status<W: Write>(w: &mut W, st: &DaemonStatus, color: bool) -> io::Result<()> {
    if !st.running {
        if color {
            writeln!(
                w,
                "{}○ stopped{}  {}pack={}{}  {}volume={}%{}",
                term::WARNING_RED,
                term::RESET,
                term::DIM,
                st.pack,
                term::RESET,
                term::DIM,
                st.volume,
                term::RESET
            )?;
        } else {
            writeln!(w, "stopped (pack {})", st.pack)?;
        }
        return Ok(());
    }

    let pid = st.pid.unwrap_or(0);
    if color {
        writeln!(
            w,
            "{}● running{}  {}pid={}{}  {}volume={}%{}  {}pack={}{}",
            term::GREEN_BOLD,
            term::RESET,
            term::DIM,
            pid,
            term::RESET,
            term::AMBER_BOLD,
            st.volume,
            term::RESET,
            term::CMD_COLOR,
            st.pack,
            term::RESET
        )?;
    } else {
        writeln!(
            w,
            "running (pid {pid}, volume={} pack={})",
            st.volume, st.pack
        )?;
    }
    Ok(())
}

/// Format `thocci packs` with active indicator, aligned slugs, and usage hint.
pub fn print_packs<W: Write>(w: &mut W, active: &str, color: bool) -> io::Result<()> {
    let imported = soundpack::list_imported_packs();
    let width = soundpack::BUILTIN_PACKS
        .iter()
        .map(|p| p.id.len())
        .chain(imported.iter().map(|p| p.id.len()))
        .max()
        .unwrap_or(14)
        + 2;

    if color {
        writeln!(
            w,
            "\n  {}SOUNDPACKS{} {}({} built-in — switch live with {}thocci <id or name>{}{})\n",
            term::CYAN_BOLD,
            term::RESET,
            term::DIM,
            soundpack::BUILTIN_PACKS.len(),
            term::CMD_COLOR,
            term::RESET,
            term::DIM,
        )?;
        write!(w, "{}", term::RESET)?;
    }

    for pack in soundpack::BUILTIN_PACKS {
        let is_active = pack.id == active;
        let default_tag = if pack.id == DEFAULT_PACK_ID {
            " (default)"
        } else {
            ""
        };
        if color {
            let marker = if is_active {
                format!("{}●{}", term::GREEN_BOLD, term::RESET)
            } else {
                format!("{}·{}", term::DIM, term::RESET)
            };
            let id_color = if is_active {
                term::GREEN_BOLD
            } else {
                term::CMD_COLOR
            };
            let pad = width.saturating_sub(pack.id.len());
            writeln!(
                w,
                "  {marker} {id_color}{}{}{spaces}{}{}{default_tag}{}",
                pack.id,
                term::RESET,
                pack.display_name,
                term::DIM,
                term::RESET,
                spaces = " ".repeat(pad),
            )?;
        } else {
            let marker = if is_active { "*" } else { " " };
            writeln!(
                w,
                "{marker} {:<width$} {}{default_tag}",
                pack.id, pack.display_name
            )?;
        }
    }

    if !imported.is_empty() {
        if color {
            writeln!(
                w,
                "\n  {}IMPORTED{} {}({}){}\n",
                term::MAGENTA_BOLD,
                term::RESET,
                term::DIM,
                imported.len(),
                term::RESET,
            )?;
        }
        for pack in &imported {
            let is_active = pack.id == active;
            if color {
                let marker = if is_active {
                    format!("{}●{}", term::GREEN_BOLD, term::RESET)
                } else {
                    format!("{}·{}", term::DIM, term::RESET)
                };
                let id_color = if is_active {
                    term::GREEN_BOLD
                } else {
                    term::CMD_COLOR
                };
                let pad = width.saturating_sub(pack.id.len());
                writeln!(
                    w,
                    "  {marker} {id_color}{}{}{spaces}{}{}",
                    pack.id,
                    term::RESET,
                    pack.display_name,
                    term::RESET,
                    spaces = " ".repeat(pad),
                )?;
            } else {
                let marker = if is_active { "*" } else { " " };
                writeln!(
                    w,
                    "{marker} {:<width$} {} (imported)",
                    pack.id, pack.display_name
                )?;
            }
        }
    }

    if color {
        writeln!(
            w,
            "\n  {}Tip:{} {}thocci Wooden Keyboard{} {}or{} {}thocci matcha{} {}work directly!{}\n",
            term::AMBER_BOLD,
            term::RESET,
            term::CMD_COLOR,
            term::RESET,
            term::DIM,
            term::RESET,
            term::CMD_COLOR,
            term::RESET,
            term::DIM,
            term::RESET
        )?;
    }
    Ok(())
}

fn print_banner<W: Write>(w: &mut W, color: bool) -> io::Result<()> {
    writeln!(w)?;
    for (idx, line) in BANNER_LINES.iter().enumerate() {
        if color {
            writeln!(w, "{}{}{}", term::BANNER_GRADIENT[idx], line, term::RESET)?;
        } else {
            writeln!(w, "{line}")?;
        }
    }
    writeln!(w)?;
    Ok(())
}

fn print_command_box<W: Write>(
    w: &mut W,
    title: &str,
    accent: &str,
    rows: &[(&str, &str)],
    color: bool,
) -> io::Result<()> {
    let header_prefix = format!("+-- [ {title} ] ");
    let dashes = BOX_INNER_WIDTH
        .saturating_add(2)
        .saturating_sub(header_prefix.len() + 1);
    let top_border = format!("{header_prefix}{}+", "-".repeat(dashes));
    let bottom_border = format!("+{}+", "-".repeat(BOX_INNER_WIDTH));

    if color {
        writeln!(w, "  {accent}{top_border}{}", term::RESET)?;
    } else {
        writeln!(w, "  {top_border}")?;
    }

    const CMD_COL: usize = 27;
    for (cmd, desc) in rows {
        let pad_cmd = CMD_COL.saturating_sub(cmd.len());
        let plain_content_len = 2 + cmd.len() + pad_cmd + 5 + desc.len();
        let pad_right = BOX_INNER_WIDTH.saturating_sub(plain_content_len);

        if color {
            writeln!(
                w,
                "  {accent}|{}  {}{cmd_pad}{}---{} {desc}{right_pad}{accent}|{}",
                term::RESET,
                colorize_command(cmd, true),
                term::DIM,
                term::RESET,
                term::RESET,
                cmd_pad = " ".repeat(pad_cmd),
                right_pad = " ".repeat(pad_right),
            )?;
        } else {
            writeln!(
                w,
                "  |  {cmd}{cmd_pad} --- {desc}{right_pad}|",
                cmd_pad = " ".repeat(pad_cmd),
                right_pad = " ".repeat(pad_right),
            )?;
        }
    }

    if color {
        writeln!(w, "  {accent}{bottom_border}{}\n", term::RESET)?;
    } else {
        writeln!(w, "  {bottom_border}\n")?;
    }
    Ok(())
}

fn print_examples<W: Write>(w: &mut W, color: bool) -> io::Result<()> {
    let examples = [
        ("thocci on", "Start typing with Cherry MX (default)"),
        ("thocci volume 60", "Lower volume to 60% live"),
        ("thocci creamy", "Switch pack by slug"),
        ("thocci Wooden Keyboard", "Switch pack by full display name"),
        ("thocci matcha", "Switch pack by unique partial match"),
        ("thocci toggle", "Toggle on/off (bind to a global shortcut)"),
    ];

    if color {
        writeln!(w, "  {}EXAMPLES TO TRY:{}", term::AMBER_BOLD, term::RESET)?;
    } else {
        writeln!(w, "  EXAMPLES TO TRY:")?;
    }

    for (cmd, comment) in examples {
        let pad = 26usize.saturating_sub(cmd.len());
        if color {
            writeln!(
                w,
                "    {}{spaces}{}# {comment}{}",
                colorize_command(cmd, true),
                term::DIM,
                term::RESET,
                spaces = " ".repeat(pad),
            )?;
        } else {
            writeln!(w, "    {cmd}{spaces}# {comment}", spaces = " ".repeat(pad))?;
        }
    }
    writeln!(w)?;
    Ok(())
}

fn print_setup_box<W: Write>(w: &mut W, color: bool) -> io::Result<()> {
    let state_path = paths::state_dir().display().to_string();
    let header_prefix = "+-- [ SETUP, FLAGS & MAINTENANCE ] ";
    let dashes = BOX_INNER_WIDTH
        .saturating_add(2)
        .saturating_sub(header_prefix.len() + 1);
    let top_border = format!("{header_prefix}{}+", "-".repeat(dashes));
    let bottom_border = format!("+{}+", "-".repeat(BOX_INNER_WIDTH));

    let lines = [
        (
            "Linux perms",
            "sudo usermod -aG input $USER  (then log out & back in)",
        ),
        (
            "Windows",
            "Works out of the box (elevate if capturing admin apps)",
        ),
        (
            "macOS",
            "Grant Input Monitoring in Privacy & Security settings",
        ),
        (
            "Plain mode",
            "Pass --plain (or NO_COLOR=1) to disable ANSI colors",
        ),
        (
            "Uninstall",
            "thocci uninstall   (stops daemon, removes binary & state)",
        ),
        ("State/Logs", state_path.as_str()),
    ];

    if color {
        writeln!(w, "  {}{top_border}{}", term::DIM, term::RESET)?;
    } else {
        writeln!(w, "  {top_border}")?;
    }

    const LABEL_COL: usize = 13;
    for (label, value) in lines {
        let pad_label = LABEL_COL.saturating_sub(label.len());
        let plain_len = 2 + label.len() + pad_label + 2 + value.len();
        let pad_right = BOX_INNER_WIDTH.saturating_sub(plain_len);
        if color {
            writeln!(
                w,
                "  {}|{}  {}{label}{}{lpad}  {value}{rpad}{}|{}",
                term::DIM,
                term::RESET,
                term::YELLOW,
                term::RESET,
                term::DIM,
                term::RESET,
                lpad = " ".repeat(pad_label),
                rpad = " ".repeat(pad_right),
            )?;
        } else {
            writeln!(
                w,
                "  |  {label}{lpad}  {value}{rpad}|",
                lpad = " ".repeat(pad_label),
                rpad = " ".repeat(pad_right),
            )?;
        }
    }

    if color {
        writeln!(w, "  {}{bottom_border}{}\n", term::DIM, term::RESET)?;
    } else {
        writeln!(w, "  {bottom_border}\n")?;
    }
    Ok(())
}

fn print_footer<W: Write>(w: &mut W, color: bool) -> io::Result<()> {
    if color {
        writeln!(
            w,
            "  {}Repo:{} https://github.com/ItzEvhyx/Thocci  {}•  v{}{}\n",
            term::DIM,
            term::RESET,
            term::DIM,
            thocci_core::VERSION,
            term::RESET
        )?;
    } else {
        writeln!(
            w,
            "  Repo: https://github.com/ItzEvhyx/Thocci  (v{})\n",
            thocci_core::VERSION
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guide_renders_both_colored_and_plain() {
        let mut colored = Vec::new();
        print_guide(&mut colored, true).unwrap();
        let colored_str = String::from_utf8(colored).unwrap();
        assert!(colored_str.contains("\x1b["));
        assert!(colored_str.contains("ENGINE CONTROLS"));
        assert!(colored_str.contains("VOLUME & SOUNDPACKS"));
        assert!(colored_str.contains("thocci Wooden Keyboard"));

        let mut plain = Vec::new();
        print_guide(&mut plain, false).unwrap();
        let plain_str = String::from_utf8(plain).unwrap();
        assert!(!plain_str.contains("\x1b["));
        assert!(plain_str.contains("ENGINE CONTROLS"));
    }
}
