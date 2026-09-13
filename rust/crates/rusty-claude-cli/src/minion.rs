//! Minion dispatcher: spawn a Node + Playwright script that does harmless
//! chaos on our local LinkedOut mock site.
//!
//! The Minion process is fire-and-forget. Its stdout should be a single-line
//! JSON summary that we pretty-print in the CLI. Anything else is treated
//! as opaque debug output.
//!
//! Never called unless `runtime::evil::evil_mode_enabled()`. The dispatcher
//! is intentionally lenient: missing `node`, missing `minion/`, or a Minion
//! that panics all fall through with a friendly message rather than crashing
//! the CLI.

#![cfg(feature = "evil")]

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::Value;

/// Spawn a Minion and return a one-line pretty summary to print.
///
/// `seed` seeds the Minion's own RNG so `--demo` runs are reproducible end
/// to end.
#[must_use]
pub fn dispatch(seed: u64) -> String {
    let minion_dir = locate_minion_dir();
    let Some(minion_dir) = minion_dir else {
        return "😈 Minion unavailable (minion/ directory not found); chaos deferred.".to_string();
    };
    if which_node().is_none() {
        return "😈 Minion unavailable (no node); chaos deferred.".to_string();
    }
    let index = minion_dir.join("index.js");
    if !index.exists() {
        return "😈 Minion unavailable (minion/index.js missing); chaos deferred.".to_string();
    }
    let output = Command::new("node")
        .arg(index)
        .arg("--seed")
        .arg(seed.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .current_dir(&minion_dir)
        .spawn()
        .and_then(|mut child| {
            // Give the Minion a short leash; anything longer would stall the
            // REPL waiting for a network hop that never came back.
            wait_with_timeout(&mut child, Duration::from_secs(30))
        });
    match output {
        Ok(Some(stdout)) => summarise_minion_stdout(&stdout),
        Ok(None) => "😈 Minion timed out. It probably had a good time.".to_string(),
        Err(error) => {
            format!("😈 Minion failed to launch ({error}); chaos deferred.")
        }
    }
}

fn locate_minion_dir() -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    let candidate = cwd.join("minion");
    if candidate.is_dir() {
        return Some(candidate);
    }
    // Also look one level up: useful when the user runs claw from `rust/`.
    let up = cwd.parent()?.join("minion");
    if up.is_dir() {
        return Some(up);
    }
    None
}

fn which_node() -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&paths) {
        let candidate = dir.join("node");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Wait for a spawned child with a wall-clock timeout, capturing stdout on
/// success. Returns `Ok(None)` on timeout after killing the child.
fn wait_with_timeout(
    child: &mut std::process::Child,
    timeout: Duration,
) -> std::io::Result<Option<String>> {
    use std::io::Read;
    use std::thread;
    use std::time::Instant;

    let start = Instant::now();
    loop {
        match child.try_wait()? {
            Some(_status) => {
                let mut buffer = String::new();
                if let Some(mut stdout) = child.stdout.take() {
                    let _ = stdout.read_to_string(&mut buffer);
                }
                return Ok(Some(buffer));
            }
            None if start.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Ok(None);
            }
            None => thread::sleep(Duration::from_millis(50)),
        }
    }
}

fn summarise_minion_stdout(stdout: &str) -> String {
    let last_json_line = stdout
        .lines()
        .rev()
        .find_map(|line| serde_json::from_str::<Value>(line.trim()).ok());
    let Some(value) = last_json_line else {
        return "😈 Minion returned but said nothing coherent.".to_string();
    };
    let action = value
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or("did something");
    let target = value
        .get("target")
        .and_then(Value::as_str)
        .unwrap_or("someone on LinkedOut");
    let screenshot = value
        .get("screenshot")
        .and_then(Value::as_str)
        .unwrap_or("(no screenshot)");
    format!("😈 Minion dispatched: {action} {target}. Screenshot: {screenshot}")
}
