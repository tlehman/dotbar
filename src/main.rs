//! dotbar — a braille-dot progress bar.
//!
//! ```text
//! dotbar [--dense] [<percent>]     render a bar
//! dotbar install-claude [--yes]   wire dotbar into ~/.claude/settings.json
//! dotbar [--dense] <subcommand>    dispatch to `dotbar-<subcommand>`
//! ```
//!
//! With no percent argument it reads Claude Code statusline JSON on stdin and
//! renders `100 - .context_window.remaining_percentage`.

mod install;

use std::io::{Read as _, Write as _};
use std::process::{Command, ExitCode};
use std::time::Duration;

/// Cap on statusline JSON read from stdin. A statusline payload is a few
/// hundred bytes; anything past this is a stuck or hostile writer, and reading
/// it to a String would grow unbounded.
const MAX_STDIN: u64 = 1 << 20;

/// Braille glyphs by dot count, 1..=8: left column bottom-up, then right
/// column. Index 0 is the "no dots yet" case and is never drawn — an empty
/// cell uses `EMPTY` instead, so the track stays visible.
const GLYPHS: [&str; 9] = ["", "⡀", "⡄", "⡆", "⡇", "⡏", "⡟", "⡿", "⣿"];
/// Baseline track for a cell with nothing filled in.
const EMPTY: &str = "⣀";
/// A braille cell holds this many dots.
const DOTS_PER_CELL: u32 = 8;

/// Render `used` percent as a colored braille bar followed by the percentage.
///
/// `pct_per_dot` is 1 (100 dots over 13 cells) or 5 (20 dots over 3 cells).
/// When `color` is false every SGR sequence is dropped: some consumers render
/// our stdout as literal text rather than as a terminal stream. See
/// <https://no-color.org>.
fn render(used: u32, pct_per_dot: u32, color: bool) -> String {
    let used = used.min(100);
    // A cell holds 8 dots, so 100/pct_per_dot dots need that many cells, with
    // the last one partly filled. Default: 12 full cells + a half cell = 100
    // dots, one dot per percent. Dense: 2 full + a half = 20 dots, 5% per dot.
    // `.max(1)` twice: a zero divisor would trap, and a `pct_per_dot` over 100
    // would leave zero dots, so `cells - 1` below would underflow and panic.
    let total_dots = (100 / pct_per_dot.max(1)).max(1);
    let cells = total_dots.div_ceil(DOTS_PER_CELL);
    let last_cap = total_dots - (cells - 1) * DOTS_PER_CELL;
    let filled = (used + pct_per_dot / 2) / pct_per_dot.max(1);

    let mut out = String::new();
    for i in 0..cells {
        let cap = if i == cells - 1 {
            last_cap
        } else {
            DOTS_PER_CELL
        };
        let n = filled.saturating_sub(i * DOTS_PER_CELL).min(cap);
        let glyph = if n == 0 {
            EMPTY
        } else {
            GLYPHS.get(n as usize).copied().unwrap_or(EMPTY)
        };
        if !color {
            out.push_str(glyph);
            continue;
        }
        if n == 0 {
            out.push_str("\x1b[38;2;60;60;60m");
        } else {
            // Green at the left end, red at the right, ramped per cell.
            let t = if cells > 1 { i * 255 / (cells - 1) } else { 0 };
            let (r, g) = ((2 * t).min(255), (2 * (255 - t)).min(255));
            out.push_str(&format!("\x1b[38;2;{r};{g};0m"));
        }
        out.push_str(glyph);
    }
    if color {
        out.push_str(&format!("\x1b[0m \x1b[2m{used}%\x1b[0m"));
    } else {
        out.push_str(&format!(" {used}%"));
    }
    out
}

/// Write one line to stdout, reporting whether it landed.
///
/// `println!` panics if the write fails, and a closed pipe is ordinary here:
/// `dotbar 50 | head -c1`, or a statusline harness that stops reading. A bar
/// nobody can read is not an error, so a failed write ends the process quietly.
fn emit(line: &str) -> bool {
    let mut out = std::io::stdout().lock();
    out.write_all(line.as_bytes()).is_ok() && out.write_all(b"\n").is_ok()
}

/// Percent used, read from statusline JSON on stdin. `None` means the field is
/// absent or the input is not JSON — the caller prints nothing and exits 0,
/// because a statusline segment with no data should vanish, not complain.
fn used_from_stdin() -> Option<u32> {
    let mut buf = String::new();
    std::io::stdin()
        .take(MAX_STDIN)
        .read_to_string(&mut buf)
        .ok()?;
    extract_used(&buf)
}

/// Percent used, from a statusline JSON document. `None` for anything that is
/// not JSON with a numeric `.context_window.remaining_percentage` — including
/// the wrong types, which read as absent rather than as zero.
fn extract_used(json: &str) -> Option<u32> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let remaining = v
        .get("context_window")?
        .get("remaining_percentage")?
        .as_f64()?;
    Some(round_pct(100.0 - remaining))
}

/// Clamp to 0..=100 and round to the nearest whole percent. NaN reads as 0.
fn round_pct(x: f64) -> u32 {
    if x.is_nan() {
        return 0;
    }
    x.clamp(0.0, 100.0).round() as u32
}

/// Animate 0 -> 100% at one frame per `step`, in place on one line.
fn demo(pct_per_dot: u32, color: bool, step: Duration) {
    let mut out = std::io::stdout().lock();
    for p in 0..=100 {
        // Any write failure means the reader is gone: stop, do not panic.
        if write!(out, "\r\x1b[K{}", render(p, pct_per_dot, color)).is_err() || out.flush().is_err()
        {
            return;
        }
        std::thread::sleep(step);
    }
    let _ = out.write_all(b"\n");
}

/// Hand off to `dotbar-<sub>`, preferring a sibling of this binary over `$PATH`
/// so a checkout's helpers win over an installed copy.
fn dispatch(sub: &str, args: &[String]) -> ExitCode {
    let name = format!("dotbar-{sub}");
    let sibling = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join(&name)))
        .filter(|p| p.is_file());
    let program = sibling.unwrap_or_else(|| name.clone().into());

    match Command::new(&program).args(args).status() {
        Ok(status) => ExitCode::from(status.code().unwrap_or(1).clamp(0, 255) as u8),
        Err(_) => {
            eprintln!("dotbar: '{sub}' is not a dotbar command");
            ExitCode::from(1)
        }
    }
}

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let dense = args.first().is_some_and(|a| a == "--dense");
    if dense {
        args.remove(0);
    }
    let pct_per_dot = if dense { 5 } else { 1 };
    let color = std::env::var_os("NO_COLOR").is_none();

    match args.split_first() {
        Some((first, rest)) => match first.parse::<f64>() {
            Ok(pct) => {
                emit(&render(round_pct(pct), pct_per_dot, color));
            }
            Err(_) => match first.as_str() {
                "demo" => demo(pct_per_dot, color, Duration::from_millis(50)),
                "demo-slow" => demo(pct_per_dot, color, Duration::from_secs(1)),
                "install-claude" => return install::run(dense, rest),
                sub => {
                    // Forward the flag we consumed, so the helper sees it too.
                    let mut fwd: Vec<String> = Vec::new();
                    if dense {
                        fwd.push("--dense".into());
                    }
                    fwd.extend_from_slice(rest);
                    return dispatch(sub, &fwd);
                }
            },
        },
        // No arguments: statusline mode. No data means no output.
        None => match used_from_stdin() {
            Some(used) => {
                emit(&render(used, pct_per_dot, color));
            }
            None => return ExitCode::SUCCESS,
        },
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(used: u32, pct_per_dot: u32) -> String {
        render(used, pct_per_dot, false)
    }

    #[test]
    fn widths_are_fixed() {
        // 13 cells at 1%/dot, 3 at 5%/dot, in every fill state.
        for used in 0..=100 {
            assert_eq!(
                plain(used, 1).chars().count() - format!(" {used}%").len(),
                13
            );
            assert_eq!(
                plain(used, 5).chars().count() - format!(" {used}%").len(),
                3
            );
        }
    }

    #[test]
    fn endpoints_are_empty_and_full() {
        assert_eq!(plain(0, 1), "⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀ 0%");
        assert!(plain(100, 1).starts_with("⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⡇"));
        assert_eq!(plain(0, 5), "⣀⣀⣀ 0%");
        assert!(plain(100, 5).starts_with("⣿⣿⡇"));
    }

    #[test]
    fn fill_is_monotonic() {
        let dots = |s: &str| s.chars().filter(|c| *c != '⣀').count();
        for used in 1..=100u32 {
            assert!(dots(&plain(used, 1)) >= dots(&plain(used - 1, 1)));
        }
    }

    #[test]
    fn color_is_opt_out_only() {
        assert!(render(50, 1, true).contains("\x1b[38;2;"));
        assert!(!render(50, 1, false).contains('\x1b'));
    }

    #[test]
    fn percentages_are_clamped_and_rounded() {
        assert_eq!(round_pct(-5.0), 0);
        assert_eq!(round_pct(150.0), 100);
        assert_eq!(round_pct(f64::NAN), 0);
        assert_eq!(round_pct(49.6), 50);
    }

    #[test]
    fn round_pct_survives_every_float() {
        // The `as u32` cast is only sound because clamp runs first, and NaN
        // clamps to NaN rather than to a bound -- hence the explicit guard.
        for x in [
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::MAX,
            f64::MIN,
            f64::MIN_POSITIVE,
            -0.0,
            1e308,
            -1e308,
            f64::EPSILON,
        ] {
            assert!(round_pct(x) <= 100, "{x} escaped the clamp");
        }
        assert_eq!(round_pct(f64::INFINITY), 100);
        assert_eq!(round_pct(f64::NEG_INFINITY), 0);
        assert_eq!(round_pct(-0.0), 0);
    }

    #[test]
    fn render_survives_any_pct_per_dot() {
        // `pct_per_dot` over 100 used to leave zero cells, so `cells - 1`
        // underflowed and panicked. Only 1 and 5 are reachable from main
        // today, which is exactly why the rest needs pinning down.
        for pd in [0u32, 1, 3, 5, 7, 33, 99, 100, 101, 255, 1000, u32::MAX] {
            for used in [0u32, 1, 50, 99, 100] {
                let s = render(used, pd, false);
                assert!(s.ends_with(&format!(" {used}%")), "pd={pd} used={used}");
                assert!(
                    s.chars().count() > format!(" {used}%").len(),
                    "pd={pd} produced no cells"
                );
            }
        }
    }

    #[test]
    fn render_clamps_used_above_100() {
        // Callers all clamp first, so this only fires if one stops.
        assert_eq!(render(u32::MAX, 1, false), render(100, 1, false));
        assert_eq!(render(101, 5, false), render(100, 5, false));
    }

    #[test]
    fn malformed_json_is_absent_not_zero() {
        // Every one of these must render nothing at all. Reading them as 0%
        // would paint an empty bar and imply the context window is untouched.
        for bad in [
            "",
            "   ",
            "not json",
            "{",
            "null",
            "[]",
            "[1,2,3]",
            "42",
            "\"a string\"",
            r#"{"context_window":null}"#,
            r#"{"context_window":42}"#,
            r#"{"context_window":{}}"#,
            r#"{"context_window":{"remaining_percentage":null}}"#,
            r#"{"context_window":{"remaining_percentage":"50"}}"#,
            r#"{"context_window":{"remaining_percentage":[50]}}"#,
            r#"{"context_window":{"remaining_percentage":{"v":50}}}"#,
            r#"{"context_window":{"remaining_percentage":true}}"#,
            r#"{"other":{"remaining_percentage":50}}"#,
            r#"{"context_window":{"remaining_percentage":50}"#,
        ] {
            assert_eq!(extract_used(bad), None, "{bad:?} should have been absent");
        }
    }

    #[test]
    fn out_of_range_json_still_lands_in_range() {
        for (json, want) in [
            (r#"{"context_window":{"remaining_percentage":100}}"#, 0),
            (r#"{"context_window":{"remaining_percentage":0}}"#, 100),
            (r#"{"context_window":{"remaining_percentage":24.3}}"#, 76),
            // Out of contract, but a harness bug must not produce a bad bar.
            (r#"{"context_window":{"remaining_percentage":-1000}}"#, 100),
            (r#"{"context_window":{"remaining_percentage":1e308}}"#, 0),
            (r#"{"context_window":{"remaining_percentage":-1e308}}"#, 100),
            (r#"{"context_window":{"remaining_percentage":1e-308}}"#, 100),
        ] {
            assert_eq!(extract_used(json), Some(want), "{json}");
        }
    }

    #[test]
    fn deeply_nested_json_does_not_blow_the_stack() {
        // serde_json has a recursion limit; the point is that hitting it is a
        // None, not an abort.
        let deep = format!("{}{}", "[".repeat(2000), "]".repeat(2000));
        assert_eq!(extract_used(&deep), None);
    }
}
