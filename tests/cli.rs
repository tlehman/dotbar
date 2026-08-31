// clippy.toml's `allow-unwrap-in-tests` only reaches `#[cfg(test)]` modules,
// not an integration-test crate like this one. A failed unwrap here IS the
// failure report, same as an assert.
#![allow(clippy::unwrap_used)]

//! Process-level behaviour: the things that only go wrong once there is a real
//! stdout, a real stdin, and a real exit code.

use std::io::Write as _;
use std::process::{Command, Stdio};

const BIN: &str = env!("CARGO_BIN_EXE_dotbar");

/// Run the binary with `args`, feeding `stdin`, and return (stdout, stderr, code).
fn run(args: &[&str], stdin: &[u8]) -> (String, String, Option<i32>) {
    let mut child = Command::new(BIN)
        .args(args)
        .env("NO_COLOR", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(stdin).ok();
    let out = child.wait_with_output().unwrap();
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
        out.status.code(),
    )
}

/// Run `<BIN> <args> | <sink>` through a shell, so the pipe really does close
/// early. Returns dotbar's own stderr, which carries its exit code as `rc=N`:
/// the pipeline's own status is the sink's, and `sh` has no PIPESTATUS.
fn piped_into(args: &str, sink: &str) -> String {
    let out = Command::new("sh")
        .arg("-c")
        .arg(format!("{{ {BIN} {args}; echo \"rc=$?\" >&2; }} | {sink}"))
        .stderr(Stdio::piped())
        .stdout(Stdio::null())
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn a_closed_pipe_is_not_a_panic() {
    // `dotbar 50 | head -c1` and friends: the reader goes away mid-write.
    // `println!` panics on that, which is why output goes through `emit`.
    for sink in ["head -c 1", "true", "cat > /dev/null"] {
        let stderr = piped_into("50", sink);
        assert!(!stderr.contains("panicked"), "sink {sink}: {stderr}");
        assert!(stderr.contains("rc=0"), "sink {sink}: {stderr}");
    }
}

#[test]
fn a_closed_pipe_stops_the_animation_quietly() {
    let stderr = piped_into("demo", "head -c 10");
    assert!(!stderr.contains("panicked"), "{stderr}");
    assert!(stderr.contains("rc=0"), "{stderr}");
}

#[test]
fn unknown_subcommand_reports_and_fails() {
    let (stdout, stderr, code) = run(&["definitely-not-a-dotbar-helper"], b"");
    assert!(stdout.is_empty());
    assert!(stderr.contains("is not a dotbar command"), "{stderr}");
    assert_eq!(code, Some(1));
}

#[test]
fn a_subcommand_name_with_path_characters_is_not_a_panic() {
    // `format!("dotbar-{sub}")` will happily build a relative path. It must
    // fail to launch like any other missing helper, not crash or escape.
    for sub in [
        "../../bin/sh",
        "/absolute",
        "..",
        "with space",
        "sub;rm -rf /",
    ] {
        let (_, stderr, code) = run(&[sub], b"");
        assert!(!stderr.contains("panicked"), "{sub}: {stderr}");
        assert_eq!(code, Some(1), "{sub}");
    }
}

#[test]
fn numeric_edge_arguments_render_in_range() {
    for (arg, want) in [
        ("nan", "0%"),
        ("inf", "100%"),
        ("-inf", "0%"),
        ("1e400", "100%"),
        ("-1e400", "0%"),
        ("-0", "0%"),
        ("00050", "50%"),
        ("+50", "50%"),
        ("49.6", "50%"),
        ("1e-400", "0%"),
    ] {
        let (stdout, stderr, code) = run(&[arg], b"");
        assert_eq!(code, Some(0), "{arg}");
        assert!(stderr.is_empty(), "{arg}: {stderr}");
        assert!(stdout.trim_end().ends_with(want), "{arg} -> {stdout:?}");
    }
}

#[test]
fn garbage_on_stdin_prints_nothing_and_succeeds() {
    for input in [&b""[..], b"not json", b"\xff\xfe\x00\x01", b"{}"] {
        let (stdout, stderr, code) = run(&[], input);
        assert_eq!(stdout, "", "{input:?}");
        assert!(stderr.is_empty(), "{input:?}: {stderr}");
        assert_eq!(code, Some(0), "{input:?}");
    }
}

#[test]
fn oversized_stdin_is_bounded_not_consumed_whole() {
    // 8 MiB of junk against a 1 MiB cap: it must return promptly with no
    // output rather than buffering whatever a stuck writer sends.
    let junk = vec![b'x'; 8 << 20];
    let (stdout, stderr, code) = run(&[], &junk);
    assert_eq!(stdout, "");
    assert!(stderr.is_empty(), "{stderr}");
    assert_eq!(code, Some(0));
}

#[test]
fn statusline_json_renders() {
    let (stdout, _, code) = run(&[], br#"{"context_window":{"remaining_percentage":24.3}}"#);
    assert!(stdout.trim_end().ends_with(" 76%"), "{stdout:?}");
    assert_eq!(code, Some(0));
}

#[test]
fn dense_is_only_a_leading_flag() {
    let (dense, _, _) = run(&["--dense", "76"], b"");
    let (trailing, _, code) = run(&["76", "--dense"], b"");
    assert_eq!(dense.chars().count(), "⣿⡿⣀ 76%\n".chars().count());
    // A trailing --dense is ignored, not an error: the wide bar still renders.
    assert!(trailing.trim_end().ends_with(" 76%"));
    assert_eq!(code, Some(0));
}

#[test]
fn install_claude_asks_backs_up_and_merges() {
    let dir = std::env::temp_dir().join(format!("dotbar-install-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let settings = dir.join("settings.json");
    let s = settings.to_str().unwrap();

    // Missing file, answer "n": nothing is created, exit 1.
    let (_, err, code) = run(&["install-claude", "--settings", s], b"n\n");
    assert_eq!(code, Some(1), "{err}");
    assert!(!settings.exists());
    assert!(err.contains("\"command\": \"dotbar\""), "{err}");

    // Answer "y": file is created with only the statusLine.
    let (_, err, code) = run(&["install-claude", "--settings", s], b"y\n");
    assert_eq!(code, Some(0), "{err}");
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
    assert_eq!(v["statusLine"]["command"], "dotbar");

    // Second run is a no-op that succeeds without prompting.
    let (_, err, code) = run(&["install-claude", "--settings", s], b"");
    assert_eq!(code, Some(0), "{err}");
    assert!(err.contains("already"), "{err}");

    // Existing foreign statusline plus other keys: prefixed, backed up, --yes skips the prompt.
    std::fs::write(
        &settings,
        r#"{"model":"opus","statusLine":{"type":"command","command":"my.sh"}}"#,
    )
    .unwrap();
    let (_, err, code) = run(
        &["--dense", "install-claude", "--yes", "--settings", s],
        b"",
    );
    assert_eq!(code, Some(0), "{err}");
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
    assert_eq!(
        v["statusLine"]["command"],
        "printf '%s ' \"$(dotbar --dense)\"; my.sh"
    );
    assert_eq!(v["model"], "opus");
    assert!(dir.join("settings.json.bak").exists());

    // Corrupt JSON is refused, not clobbered.
    std::fs::write(&settings, "{oops").unwrap();
    let (_, err, code) = run(&["install-claude", "--yes", "--settings", s], b"");
    assert_eq!(code, Some(1), "{err}");
    assert_eq!(std::fs::read_to_string(&settings).unwrap(), "{oops");

    std::fs::remove_dir_all(&dir).ok();
}
