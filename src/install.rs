//! `dotbar install-claude`: wire dotbar into Claude Code's statusline.
//!
//! Reads `~/.claude/settings.json`, shows the exact `statusLine` entry it
//! intends to write, and asks before touching anything. An existing statusline
//! command is kept and prefixed with dotbar rather than replaced.

use std::io::{BufRead as _, Write as _};
use std::path::PathBuf;
use std::process::ExitCode;

use serde_json::{Map, Value, json};

/// What `merge` decided, so the prompt can say it in words.
#[derive(Debug, PartialEq, Eq)]
pub enum Plan {
    /// No `statusLine` was set; dotbar becomes the whole statusline.
    Fresh,
    /// A command was set; dotbar is prepended on the same line.
    Prefixed(String),
    /// The command already mentions dotbar. Nothing to do.
    AlreadyInstalled,
}

/// The dotbar invocation for the statusline. `--dense` gives the 3-cell bar.
fn dotbar_cmd(dense: bool) -> &'static str {
    if dense { "dotbar --dense" } else { "dotbar" }
}

/// Merge a `statusLine` into `settings`, returning the new document and what
/// changed. Errors when the file holds something that is not a JSON object,
/// because rewriting it would destroy whatever the user meant by it.
pub fn merge(settings: Value, dense: bool) -> Result<(Value, Plan), String> {
    let mut obj = match settings {
        Value::Object(o) => o,
        Value::Null => Map::new(),
        other => return Err(format!("settings.json is not an object: {other}")),
    };
    let bar = dotbar_cmd(dense);
    let existing = obj
        .get("statusLine")
        .and_then(|s| s.get("command"))
        .and_then(Value::as_str)
        .map(str::to_owned);

    let plan = match existing {
        Some(cmd) if cmd.contains("dotbar") => {
            return Ok((Value::Object(obj), Plan::AlreadyInstalled));
        }
        Some(cmd) => Plan::Prefixed(cmd),
        None => Plan::Fresh,
    };
    let command = match &plan {
        // `$(...)` strips the trailing newline, so the bar and the old
        // statusline share a line. stdin is consumed by dotbar, which is why
        // it goes first; the old command gets nothing on stdin. Statuslines
        // that read the JSON themselves need `tee`, and that is the user's
        // call, so the prompt shows the full command before writing.
        Plan::Prefixed(old) => format!("printf '%s ' \"$({bar})\"; {old}"),
        _ => bar.to_owned(),
    };
    obj.insert(
        "statusLine".into(),
        json!({ "type": "command", "command": command }),
    );
    Ok((Value::Object(obj), plan))
}

/// `$CLAUDE_CONFIG_DIR/settings.json`, else `~/.claude/settings.json`.
fn default_settings_path() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("CLAUDE_CONFIG_DIR") {
        return Some(PathBuf::from(dir).join("settings.json"));
    }
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".claude").join("settings.json"))
}

/// Ask `[y/N]` on stdin. Anything but a leading `y` or `Y` is a no.
fn confirm() -> bool {
    let mut err = std::io::stderr().lock();
    if write!(err, "Write it? [y/N] ").is_err() || err.flush().is_err() {
        return false;
    }
    let mut line = String::new();
    if std::io::stdin().lock().read_line(&mut line).is_err() {
        return false;
    }
    matches!(line.trim_start().chars().next(), Some('y' | 'Y'))
}

/// Entry point for `dotbar install-claude`. `dense` is the leading flag
/// consumed by main; `args` are the rest.
pub fn run(dense: bool, args: &[String]) -> ExitCode {
    let mut yes = false;
    let mut path: Option<PathBuf> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--yes" | "-y" => yes = true,
            "--settings" => match it.next() {
                Some(p) => path = Some(PathBuf::from(p)),
                None => return fail("--settings needs a path"),
            },
            other => return fail(&format!("unknown option '{other}'")),
        }
    }
    let Some(path) = path.or_else(default_settings_path) else {
        return fail("cannot locate settings.json: set HOME or CLAUDE_CONFIG_DIR");
    };

    let current = match std::fs::read_to_string(&path) {
        Ok(text) => match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(e) => return fail(&format!("{}: not valid JSON: {e}", path.display())),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Value::Null,
        Err(e) => return fail(&format!("{}: {e}", path.display())),
    };

    let (merged, plan) = match merge(current, dense) {
        Ok(m) => m,
        Err(e) => return fail(&e),
    };
    let entry = merged.get("statusLine").cloned().unwrap_or(Value::Null);
    let entry = serde_json::to_string_pretty(&entry).unwrap_or_default();

    let mut err = std::io::stderr().lock();
    let _ = match &plan {
        Plan::AlreadyInstalled => {
            let _ = writeln!(
                err,
                "{}: statusLine already runs dotbar:\n{entry}",
                path.display()
            );
            return ExitCode::SUCCESS;
        }
        Plan::Fresh => writeln!(
            err,
            "{}: no statusLine set. Proposed:\n{entry}",
            path.display()
        ),
        Plan::Prefixed(old) => writeln!(
            err,
            "{}: statusLine currently runs:\n  {old}\nProposed, with dotbar prefixed:\n{entry}",
            path.display()
        ),
    };
    drop(err);

    if !yes && !confirm() {
        eprintln!(
            "Not written. To do it by hand, add the entry above to {}.",
            path.display()
        );
        return ExitCode::from(1);
    }

    if let Some(dir) = path.parent()
        && let Err(e) = std::fs::create_dir_all(dir)
    {
        return fail(&format!("{}: {e}", dir.display()));
    }
    if path.exists() {
        let bak = path.with_extension("json.bak");
        if let Err(e) = std::fs::copy(&path, &bak) {
            return fail(&format!("backup {}: {e}", bak.display()));
        }
        eprintln!("Backed up to {}", bak.display());
    }
    let mut text = serde_json::to_string_pretty(&merged).unwrap_or_default();
    text.push('\n');
    match std::fs::write(&path, text) {
        Ok(()) => {
            eprintln!(
                "Wrote {}. Restart Claude Code to see the bar.",
                path.display()
            );
            ExitCode::SUCCESS
        }
        Err(e) => fail(&format!("{}: {e}", path.display())),
    }
}

fn fail(msg: &str) -> ExitCode {
    eprintln!("dotbar install-claude: {msg}");
    ExitCode::from(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd(v: &Value) -> &str {
        v["statusLine"]["command"].as_str().unwrap()
    }

    #[test]
    fn fresh_settings_get_a_plain_dotbar() {
        let (v, plan) = merge(Value::Null, false).unwrap();
        assert_eq!(plan, Plan::Fresh);
        assert_eq!(cmd(&v), "dotbar");
        assert_eq!(v["statusLine"]["type"], "command");
        let (v, _) = merge(json!({"model": "opus"}), true).unwrap();
        assert_eq!(cmd(&v), "dotbar --dense");
        assert_eq!(v["model"], "opus", "other keys survive");
    }

    #[test]
    fn existing_command_is_prefixed_not_replaced() {
        let old = json!({"statusLine": {"type": "command", "command": "~/bar.sh"}});
        let (v, plan) = merge(old, false).unwrap();
        assert_eq!(plan, Plan::Prefixed("~/bar.sh".into()));
        assert_eq!(cmd(&v), "printf '%s ' \"$(dotbar)\"; ~/bar.sh");
    }

    #[test]
    fn already_installed_is_a_no_op() {
        let old = json!({"statusLine": {"type": "command", "command": "dotbar --dense"}});
        let (v, plan) = merge(old.clone(), false).unwrap();
        assert_eq!(plan, Plan::AlreadyInstalled);
        assert_eq!(v, old);
    }

    #[test]
    fn non_object_settings_are_refused() {
        for bad in [json!([]), json!(42), json!("x"), json!(true)] {
            assert!(merge(bad, false).is_err());
        }
    }

    #[test]
    fn statusline_without_a_command_string_counts_as_fresh() {
        let old = json!({"statusLine": {"type": "command", "command": 7}});
        let (v, plan) = merge(old, false).unwrap();
        assert_eq!(plan, Plan::Fresh);
        assert_eq!(cmd(&v), "dotbar");
    }
}
