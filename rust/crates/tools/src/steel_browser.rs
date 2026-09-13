//! Visible browser tools: spawn the Node sidecar and drive local Chrome (CDP) or Steel.dev cloud Chrome.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};

use runtime::default_config_home;
use serde::Deserialize;
use serde_json::{json, Value};

const DEFAULT_TIMEOUT_MS: u64 = 900_000;
const DEFAULT_WAIT_MS: u64 = 120_000;

#[derive(Debug, Deserialize)]
pub struct BrowserStartInput {
    #[serde(default)]
    auto_open: Option<bool>,
    #[serde(default)]
    timeout_ms: Option<u64>,
    #[serde(default)]
    backend: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct BrowserNavigateInput {
    url: String,
}

#[derive(Debug, Deserialize)]
pub struct BrowserRefInput {
    #[serde(default)]
    r#ref: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct BrowserTypeInput {
    #[serde(default)]
    r#ref: Option<String>,
    #[serde(default)]
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct BrowserPressInput {
    #[serde(default)]
    key: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct BrowserScrollInput {
    #[serde(default)]
    delta_y: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct BrowserWaitInput {
    #[serde(default)]
    url_contains: Option<String>,
    #[serde(default)]
    title_contains: Option<String>,
    #[serde(default)]
    timeout_ms: Option<u64>,
}

struct Sidecar {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl Sidecar {
    fn spawn() -> Result<Self, String> {
        let driver = resolve_driver_path()?;
        let mut command = Command::new("node");
        command
            .arg(&driver)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        if mock_enabled() {
            command.arg("--mock");
            command.env("CLAW_BROWSER_MOCK", "1");
        }
        if let Some(key) = resolve_steel_api_key() {
            command.env("STEEL_API_KEY", key);
        }
        if let Some(base) = resolve_env_or_dotenv("STEEL_BASE_URL") {
            command.env("STEEL_BASE_URL", base);
        }
        if let Some(cdp) = resolve_env_or_dotenv("CLAW_CHROME_CDP_URL") {
            command.env("CLAW_CHROME_CDP_URL", cdp);
        }
        if let Some(backend) = resolve_env_or_dotenv("CLAW_BROWSER_BACKEND") {
            command.env("CLAW_BROWSER_BACKEND", backend);
        }
        let mut child = command.spawn().map_err(|error| {
            format!(
                "failed to start Steel browser driver (`node {}`): {error}",
                driver.display()
            )
        })?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| String::from("browser driver stdin is unavailable"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| String::from("browser driver stdout is unavailable"))?;
        SIDECAR_PID.store(child.id(), Ordering::SeqCst);
        Ok(Self {
            child,
            stdin: Some(stdin),
            stdout: BufReader::new(stdout),
            next_id: 1,
        })
    }

    fn rpc(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        let payload = json!({ "id": id, "method": method, "params": params });
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| String::from("browser driver stdin is closed"))?;
        writeln!(stdin, "{payload}").map_err(|error| error.to_string())?;
        stdin.flush().map_err(|error| error.to_string())?;
        let message = read_rpc_message(&mut self.stdout, id)?;
        if message.get("ok").and_then(Value::as_bool) != Some(true) {
            let error = message
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("browser driver error");
            return Err(error.to_string());
        }
        Ok(message.get("result").cloned().unwrap_or(Value::Null))
    }
}

fn read_rpc_message(stdout: &mut BufReader<ChildStdout>, id: u64) -> Result<Value, String> {
    for _ in 0..80 {
        let mut line = String::new();
        let bytes = stdout
            .read_line(&mut line)
            .map_err(|error| error.to_string())?;
        if bytes == 0 {
            return Err(String::from("browser driver closed stdout"));
        }
        let trimmed = line.trim();
        if !trimmed.starts_with('{') {
            continue;
        }
        let Ok(message) = serde_json::from_str::<Value>(trimmed) else {
            continue;
        };
        if message.get("id").and_then(Value::as_u64) != Some(id) {
            continue;
        }
        return Ok(message);
    }
    Err(String::from(
        "browser driver did not return a JSON protocol line",
    ))
}

impl Drop for Sidecar {
    fn drop(&mut self) {
        SIDECAR_PID.store(0, Ordering::SeqCst);
        if let Some(mut stdin) = self.stdin.take() {
            let _ = writeln!(
                stdin,
                "{}",
                json!({ "id": 0, "method": "stop", "params": {} })
            );
            let _ = stdin.flush();
        }
        for _ in 0..20 {
            match self.child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) => std::thread::sleep(std::time::Duration::from_millis(50)),
                Err(_) => break,
            }
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn sidecar_slot() -> &'static Mutex<Option<Sidecar>> {
    static SLOT: OnceLock<Mutex<Option<Sidecar>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

static SIDECAR_PID: AtomicU32 = AtomicU32::new(0);

fn kill_pid(pid: u32) {
    if pid == 0 {
        return;
    }
    #[cfg(unix)]
    {
        let _ = Command::new("kill")
            .args(["-KILL", &pid.to_string()])
            .status();
    }
    #[cfg(not(unix))]
    {
        let _ = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/F"])
            .status();
    }
}

/// Kill the browser sidecar without waiting on the RPC mutex.
/// Safe to call from a Ctrl+C handler while `rpc()` is blocked on a pipe read.
pub fn abort_browser() {
    let pid = SIDECAR_PID.swap(0, Ordering::SeqCst);
    kill_pid(pid);
    if let Ok(mut guard) = sidecar_slot().try_lock() {
        if let Some(mut sidecar) = guard.take() {
            let _ = sidecar.child.kill();
        }
    }
}

fn with_sidecar<T>(
    ensure: bool,
    callback: impl FnOnce(&mut Sidecar) -> Result<T, String>,
) -> Result<T, String> {
    let mut guard = sidecar_slot()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if ensure && guard.is_none() {
        *guard = Some(Sidecar::spawn()?);
    }
    let sidecar = guard
        .as_mut()
        .ok_or_else(|| String::from("No browser session. Call BrowserStart first."))?;
    callback(sidecar)
}

fn mock_enabled() -> bool {
    matches!(
        std::env::var("CLAW_BROWSER_MOCK").as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE")
    )
}

fn resolve_driver_path() -> Result<PathBuf, String> {
    if let Ok(path) = std::env::var("CLAW_BROWSER_DRIVER") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Ok(path);
        }
        return Err(format!(
            "CLAW_BROWSER_DRIVER does not exist: {}",
            path.display()
        ));
    }
    let manifest_candidate = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join("browser/steel-driver.js");
    if manifest_candidate.is_file() {
        return Ok(manifest_candidate);
    }
    if let Ok(cwd) = std::env::current_dir() {
        if let Some(found) = walk_for_driver(&cwd) {
            return Ok(found);
        }
    }
    Err(String::from(
        "could not find browser/steel-driver.js; set CLAW_BROWSER_DRIVER",
    ))
}

fn walk_for_driver(start: &Path) -> Option<PathBuf> {
    let mut current = Some(start);
    while let Some(dir) = current {
        let candidate = dir.join("browser/steel-driver.js");
        if candidate.is_file() {
            return Some(candidate);
        }
        current = dir.parent();
    }
    None
}

fn parse_dotenv(content: &str) -> HashMap<String, String> {
    let mut values = HashMap::new();
    for raw in content.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line).trim();
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let mut value = value.trim().to_string();
        if (value.starts_with('"') && value.ends_with('"'))
            || (value.starts_with('\'') && value.ends_with('\''))
        {
            value = value[1..value.len() - 1].to_string();
        }
        if !key.trim().is_empty() && !value.is_empty() {
            values.insert(key.trim().to_string(), value);
        }
    }
    values
}

fn dotenv_search_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        let mut current = Some(cwd);
        while let Some(dir) = current {
            roots.push(dir.clone());
            current = dir.parent().map(Path::to_path_buf);
        }
    }
    if let Some(home) = std::env::var_os("CLAW_CONFIG_HOME") {
        roots.push(PathBuf::from(home));
    }
    roots
}

fn dotenv_value(key: &str) -> Option<String> {
    for root in dotenv_search_roots() {
        let path = root.join(".env");
        let Ok(content) = std::fs::read_to_string(path) else {
            continue;
        };
        if let Some(value) = parse_dotenv(&content)
            .get(key)
            .filter(|value| !value.is_empty())
            .cloned()
        {
            return Some(value);
        }
    }
    None
}

fn resolve_env_or_dotenv(key: &str) -> Option<String> {
    match std::env::var(key) {
        Ok(value) if !value.is_empty() => Some(value),
        _ => dotenv_value(key),
    }
}

fn resolve_steel_api_key() -> Option<String> {
    resolve_env_or_dotenv("STEEL_API_KEY")
}

fn resolve_backend(input: Option<&str>) -> String {
    if let Some(value) = input.map(str::trim).filter(|value| !value.is_empty()) {
        return value.to_ascii_lowercase();
    }
    resolve_env_or_dotenv("CLAW_BROWSER_BACKEND")
        .map(|value| value.to_ascii_lowercase())
        .unwrap_or_else(|| String::from("local"))
}

fn steel_setup_error() -> String {
    let cwd = std::env::current_dir()
        .ok()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| String::from("unknown"));
    format!(
        "Steel is not configured. Set STEEL_API_KEY in the environment or in a .env file in this directory or a parent (cwd is {cwd}). Copy .env.example to the repo root .env (https://app.steel.dev/settings/api-keys)."
    )
}

fn profile_path() -> PathBuf {
    default_config_home().join("steel-profile.json")
}

fn load_profile_id() -> Option<String> {
    let content = std::fs::read_to_string(profile_path()).ok()?;
    let value: Value = serde_json::from_str(&content).ok()?;
    value
        .get("profileId")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(ToOwned::to_owned)
}

fn persist_profile_id(profile_id: Option<&str>) -> Result<(), String> {
    let Some(profile_id) = profile_id.filter(|id| !id.is_empty()) else {
        return Ok(());
    };
    let path = profile_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    std::fs::write(
        path,
        serde_json::to_string_pretty(&json!({ "profileId": profile_id }))
            .map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())
}

fn auto_open_enabled(override_value: Option<bool>) -> bool {
    if let Some(value) = override_value {
        return value;
    }
    !matches!(
        std::env::var("CLAW_BROWSER_AUTO_OPEN").as_deref(),
        Ok("0") | Ok("false") | Ok("FALSE")
    )
}

fn open_watch_url(url: &str) -> Option<String> {
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(target_os = "windows") {
        "cmd"
    } else {
        "xdg-open"
    };
    let mut command = Command::new(opener);
    if cfg!(target_os = "windows") {
        command.args(["/C", "start", "", url]);
    } else {
        command.arg(url);
    }
    match command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
    {
        Ok(status) if status.success() => Some(format!("opened live viewer: {url}")),
        _ => Some(format!("open the live viewer yourself: {url}")),
    }
}

const MAX_BROWSER_SNAPSHOT_CHARS: usize = 12_000;

fn truncate_snapshot_text(snapshot: &str) -> String {
    if snapshot.len() <= MAX_BROWSER_SNAPSHOT_CHARS {
        return snapshot.to_string();
    }
    let mut end = MAX_BROWSER_SNAPSHOT_CHARS;
    while end > 0 && !snapshot.is_char_boundary(end) {
        end -= 1;
    }
    let cut = snapshot[..end].rfind('\n').unwrap_or(end);
    format!("{}\n…(snapshot truncated)", &snapshot[..cut])
}

/// Compact JSON for the model: drop the duplicate `nodes` array and cap snapshot size.
fn model_json(mut value: Value) -> Result<String, String> {
    if let Some(url) = value.get("url").and_then(Value::as_str) {
        runtime::set_outbound_page_url(url);
    }
    if let Some(object) = value.as_object_mut() {
        object.remove("nodes");
        let snapshot = object
            .get("snapshot")
            .and_then(Value::as_str)
            .map(str::to_owned);
        if let Some(snapshot) = snapshot {
            if snapshot.len() > MAX_BROWSER_SNAPSHOT_CHARS {
                object.insert(
                    "snapshot".to_string(),
                    json!(truncate_snapshot_text(&snapshot)),
                );
                object.insert("truncated".to_string(), json!(true));
            }
        }
    }
    serde_json::to_string(&value).map_err(|error| error.to_string())
}

fn attach_watch_hint(mut value: Value) -> Value {
    if let Some(url) = value
        .get("watchUrl")
        .or_else(|| value.get("debugUrl"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
    {
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "hint".to_string(),
                json!(format!("Watch (interactive): {url}")),
            );
        }
    }
    value
}

/// Release any live Steel session. Safe to call on CLI shutdown.
pub fn shutdown_browser() {
    let mut guard = sidecar_slot()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *guard = None;
}

pub fn run_browser_start(input: BrowserStartInput) -> Result<String, String> {
    let backend = resolve_backend(input.backend.as_deref());
    if backend != "local" && backend != "steel" {
        return Err(format!(
            "Unknown browser backend '{backend}'. Use steel or local."
        ));
    }
    if !mock_enabled() && backend == "steel" && resolve_steel_api_key().is_none() {
        return Err(steel_setup_error());
    }
    if backend == "local" {
        if let Ok(existing) = with_sidecar(false, |sidecar| sidecar.rpc("status", json!({}))) {
            if existing.get("live").and_then(Value::as_bool) == Some(true)
                && existing.get("backend").and_then(Value::as_str) != Some("steel")
            {
                let mut output = attach_watch_hint(existing);
                if let Some(object) = output.as_object_mut() {
                    object.insert(
                        "hint".to_string(),
                        json!("Already attached to your Chrome. Watch that window."),
                    );
                }
                return model_json(output);
            }
        }
        if !mock_enabled() {
            eprintln!("Attaching to Chrome — click Allow if a dialog appears. Ctrl+C aborts.");
        }
    }
    let result = with_sidecar(true, |sidecar| {
        sidecar.rpc(
            "start",
            json!({
                "profileId": load_profile_id(),
                "timeoutMs": input.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS),
                "backend": backend,
                "cdpUrl": resolve_env_or_dotenv("CLAW_CHROME_CDP_URL"),
                "autoLaunch": true,
            }),
        )
    })?;
    persist_profile_id(result.get("profileId").and_then(Value::as_str))?;
    let watch_url = result
        .get("watchUrl")
        .or_else(|| result.get("debugUrl"))
        .and_then(Value::as_str)
        .filter(|url| !url.is_empty())
        .map(ToOwned::to_owned);
    let mut output = attach_watch_hint(result);
    if backend != "local" && auto_open_enabled(input.auto_open) {
        if let Some(url) = watch_url.as_deref() {
            if let Some(opened) = open_watch_url(url) {
                if let Some(object) = output.as_object_mut() {
                    object.insert("viewer".to_string(), json!(opened));
                }
            }
        }
    }
    model_json(output)
}

pub fn run_browser_navigate(input: BrowserNavigateInput) -> Result<String, String> {
    if input.url.trim().is_empty() {
        return Err(String::from("url is required"));
    }
    model_json(with_sidecar(false, |sidecar| {
        sidecar.rpc("navigate", json!({ "url": input.url.trim() }))
    })?)
}

pub fn run_browser_snapshot() -> Result<String, String> {
    model_json(with_sidecar(false, |sidecar| {
        sidecar.rpc("snapshot", json!({}))
    })?)
}

pub fn run_browser_click(input: BrowserRefInput) -> Result<String, String> {
    let reference = input
        .r#ref
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| String::from("ref is required"))?;
    model_json(with_sidecar(false, |sidecar| {
        sidecar.rpc("click", json!({ "ref": reference }))
    })?)
}

pub fn run_browser_type(input: BrowserTypeInput) -> Result<String, String> {
    let reference = input
        .r#ref
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| String::from("ref is required"))?;
    let text = input
        .text
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| String::from("text is required"))?;
    model_json(with_sidecar(false, |sidecar| {
        sidecar.rpc("type", json!({ "ref": reference, "text": text }))
    })?)
}

pub fn run_browser_press(input: BrowserPressInput) -> Result<String, String> {
    let key = input
        .key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| String::from("key is required"))?;
    model_json(with_sidecar(false, |sidecar| {
        sidecar.rpc("press", json!({ "key": key }))
    })?)
}

pub fn run_browser_scroll(input: BrowserScrollInput) -> Result<String, String> {
    model_json(with_sidecar(false, |sidecar| {
        sidecar.rpc("scroll", json!({ "deltaY": input.delta_y.unwrap_or(400) }))
    })?)
}

pub fn run_browser_wait(input: BrowserWaitInput) -> Result<String, String> {
    model_json(with_sidecar(false, |sidecar| {
        sidecar.rpc(
            "wait",
            json!({
                "urlContains": input.url_contains,
                "titleContains": input.title_contains,
                "timeoutMs": input.timeout_ms.unwrap_or(DEFAULT_WAIT_MS),
            }),
        )
    })?)
}

pub fn run_browser_stop() -> Result<String, String> {
    let result = match with_sidecar(false, |sidecar| sidecar.rpc("stop", json!({}))) {
        Ok(value) => value,
        Err(error) if error.contains("No browser session") => {
            json!({ "released": false, "sessionId": Value::Null })
        }
        Err(error) => return Err(error),
    };
    persist_profile_id(result.get("profileId").and_then(Value::as_str))?;
    shutdown_browser();
    model_json(result)
}

pub fn run_browser_status() -> Result<String, String> {
    match with_sidecar(false, |sidecar| sidecar.rpc("status", json!({}))) {
        Ok(value) => model_json(attach_watch_hint(value)),
        Err(error) if error.contains("No browser session") => model_json(json!({ "live": false })),
        Err(error) => Err(error),
    }
}

fn attach_local_chrome() -> Result<Value, String> {
    let raw = run_browser_start(BrowserStartInput {
        auto_open: Some(false),
        timeout_ms: Some(DEFAULT_TIMEOUT_MS),
        backend: Some(String::from("local")),
    })?;
    Ok(serde_json::from_str(&raw).unwrap_or(json!({ "raw": raw })))
}

fn format_attached_text(started: &Value) -> String {
    let url = started
        .get("url")
        .and_then(Value::as_str)
        .unwrap_or("(current tab)");
    let title = started.get("title").and_then(Value::as_str).unwrap_or("");
    let relaunched = started.get("launched").and_then(Value::as_bool) == Some(true);
    format!(
        "Browser\n  Backend          local Chrome\n  Tab              {title}\n  URL              {url}\n  {}\n  Next             Watch this Chrome window. `/browser stop` disconnects Claw without quitting Chrome.",
        if relaunched {
            "Relaunched       Chrome with debugging (your profile, still logged in)"
        } else {
            "Attached         existing Chrome"
        }
    )
}

/// Slash-command handler used by the CLI (`/browser` attaches to local Chrome).
pub fn handle_browser_slash(action: Option<&str>) -> Result<(String, Value), String> {
    match action.unwrap_or("start") {
        "status" => {
            let raw = run_browser_status()?;
            let json: Value = serde_json::from_str(&raw).unwrap_or(json!({ "raw": raw }));
            Ok((format_status_text(&json), json))
        }
        "start" | "local" => {
            let attached = attach_local_chrome()?;
            Ok((
                format_attached_text(&attached),
                json!({
                    "kind": "browser",
                    "action": "start",
                    "status": "ok",
                    "session": attached,
                }),
            ))
        }
        "login" => {
            let attached = attach_local_chrome()?;
            let mail = run_browser_navigate(BrowserNavigateInput {
                url: String::from("https://mail.google.com"),
            })?;
            let message = format!(
                "{}\n  Site             Gmail\n  Snapshot\n{mail}",
                format_attached_text(&attached)
            );
            Ok((
                message,
                json!({
                    "kind": "browser",
                    "action": "login",
                    "status": "ok",
                    "session": attached,
                }),
            ))
        }
        "open" => {
            let raw = run_browser_status()?;
            let json: Value = serde_json::from_str(&raw).unwrap_or(json!({ "raw": raw }));
            let url = json
                .get("watchUrl")
                .or_else(|| json.get("debugUrl"))
                .and_then(Value::as_str);
            let Some(url) = url.filter(|value| !value.is_empty()) else {
                if json.get("backend").and_then(Value::as_str) == Some("local")
                    || json.get("live").and_then(Value::as_bool) == Some(true)
                {
                    return Ok((
                        "Browser\n  Viewer           your local Chrome window (no separate Steel player)".to_string(),
                        json!({
                            "kind": "browser",
                            "action": "open",
                            "status": "ok",
                            "backend": "local",
                        }),
                    ));
                }
                return Err(String::from(
                    "No live browser session. Run /browser to attach to Chrome.",
                ));
            };
            let opened = open_watch_url(url).unwrap_or_else(|| url.to_string());
            Ok((
                format!("Browser\n  Viewer           {opened}"),
                json!({
                    "kind": "browser",
                    "action": "open",
                    "status": "ok",
                    "watchUrl": url,
                }),
            ))
        }
        "stop" => {
            let raw = run_browser_stop()?;
            let json: Value = serde_json::from_str(&raw).unwrap_or(json!({ "raw": raw }));
            let local = json.get("backend").and_then(Value::as_str) == Some("local");
            let line = if local {
                "Browser\n  Session          disconnected (Chrome left running)"
            } else {
                "Browser\n  Session          released"
            };
            Ok((
                line.to_string(),
                json!({
                    "kind": "browser",
                    "action": "stop",
                    "status": "ok",
                    "result": json,
                }),
            ))
        }
        "help" => {
            Ok((
                "Browser\n  Usage            /browser\n  /browser         Attach to your Chrome (opens chrome://inspect/#remote-debugging if needed)\n  status           Show whether a session is live\n  login            Attach and open Gmail\n  stop             Disconnect; Chrome stays open".to_string(),
                json!({
                    "kind": "browser",
                    "action": "help",
                    "status": "ok",
                }),
            ))
        }
        other => Err(format!(
            "Unknown /browser action '{other}'. Use /browser (attach), status, login, or stop."
        )),
    }
}

fn format_status_text(json: &Value) -> String {
    if json.get("live").and_then(Value::as_bool) != Some(true) {
        return String::from(
            "Browser\n  Session          none\n  Next             /browser (attaches to your Chrome)",
        );
    }
    let backend = json
        .get("backend")
        .and_then(Value::as_str)
        .unwrap_or("steel");
    let watch = if backend == "local" {
        "this Chrome window"
    } else {
        json.get("watchUrl")
            .or_else(|| json.get("debugUrl"))
            .and_then(Value::as_str)
            .unwrap_or("?")
    };
    format!(
        "Browser\n  Backend          {}\n  Session          {}\n  URL              {}\n  Watch            {}",
        backend,
        json.get("sessionId").and_then(Value::as_str).unwrap_or("?"),
        json.get("url").and_then(Value::as_str).unwrap_or("?"),
        watch
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    struct EnvGuard {
        key: &'static str,
        original: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: Option<&str>) -> Self {
            let original = std::env::var_os(key);
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
            Self { key, original }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match self.original.take() {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    fn temp_dir(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should be after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "claw-steel-{label}-{}-{}",
            std::process::id(),
            nanos
        ));
        std::fs::create_dir_all(&path).expect("temp dir");
        path
    }

    #[test]
    fn given_no_env_when_resolve_backend_then_local() {
        let _lock = env_lock();
        let _backend = EnvGuard::set("CLAW_BROWSER_BACKEND", None);
        assert_eq!(resolve_backend(None), "local");
        assert_eq!(resolve_backend(Some("steel")), "steel");
    }

    #[test]
    fn given_no_api_key_when_browser_start_then_setup_error() {
        let _lock = env_lock();
        shutdown_browser();
        let _mock = EnvGuard::set("CLAW_BROWSER_MOCK", None);
        let _key = EnvGuard::set("STEEL_API_KEY", None);
        let _backend = EnvGuard::set("CLAW_BROWSER_BACKEND", None);
        let cwd = temp_dir("no-key");
        let original = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(&cwd).expect("chdir");
        let result = run_browser_start(BrowserStartInput {
            auto_open: Some(false),
            timeout_ms: None,
            backend: Some(String::from("steel")),
        });
        std::env::set_current_dir(original).expect("restore cwd");
        let error = result.expect_err("missing key should fail");
        assert!(error.contains("STEEL_API_KEY"), "unexpected error: {error}");
    }

    #[test]
    fn given_nested_cwd_when_parent_has_env_then_key_is_found() {
        let _lock = env_lock();
        let _key = EnvGuard::set("STEEL_API_KEY", None);
        let root = temp_dir("parent-env");
        let nested = root.join("rust");
        std::fs::create_dir_all(&nested).expect("nested cwd");
        std::fs::write(root.join(".env"), "STEEL_API_KEY=ste-test-from-parent\n")
            .expect("write parent env");
        let original = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(&nested).expect("chdir");
        let found = resolve_steel_api_key();
        std::env::set_current_dir(original).expect("restore cwd");
        assert_eq!(found.as_deref(), Some("ste-test-from-parent"));
    }

    #[test]
    fn given_mock_driver_when_gmail_flow_then_snapshot_and_send() {
        let _lock = env_lock();
        shutdown_browser();
        let _mock = EnvGuard::set("CLAW_BROWSER_MOCK", Some("1"));
        let _open = EnvGuard::set("CLAW_BROWSER_AUTO_OPEN", Some("0"));
        let _backend = EnvGuard::set("CLAW_BROWSER_BACKEND", None);
        let home = temp_dir("profile");
        let _home = EnvGuard::set("CLAW_CONFIG_HOME", Some(home.to_str().unwrap()));

        let started = run_browser_start(BrowserStartInput {
            auto_open: Some(false),
            timeout_ms: None,
            backend: None,
        })
        .expect("mock start");
        assert!(started.contains("local"), "{started}");

        let navigated = run_browser_navigate(BrowserNavigateInput {
            url: String::from("https://mail.google.com"),
        })
        .expect("navigate");
        assert!(navigated.contains("[ref=e1]"), "{navigated}");
        let parsed: Value = serde_json::from_str(&navigated).expect("navigate json");
        assert!(
            parsed.get("nodes").is_none(),
            "nodes array should not be sent to the model: {navigated}"
        );
        assert!(
            parsed.get("snapshot").and_then(Value::as_str).is_some(),
            "snapshot text should remain: {navigated}"
        );

        let clicked = run_browser_click(BrowserRefInput {
            r#ref: Some(String::from("e1")),
        })
        .expect("click compose");
        assert!(clicked.contains("To"), "{clicked}");

        run_browser_type(BrowserTypeInput {
            r#ref: Some(String::from("e3")),
            text: Some(String::from("alice@example.com")),
        })
        .expect("type to");
        run_browser_type(BrowserTypeInput {
            r#ref: Some(String::from("e4")),
            text: Some(String::from("Hello")),
        })
        .expect("type subject");
        let sent = run_browser_click(BrowserRefInput {
            r#ref: Some(String::from("e6")),
        })
        .expect("send");
        assert!(sent.contains("sent"), "{sent}");

        let stopped = run_browser_stop().expect("stop");
        assert!(stopped.contains("released"), "{stopped}");
        thread::sleep(std::time::Duration::from_millis(20));
    }

    #[test]
    fn given_mock_driver_when_execute_tool_then_browser_start_json() {
        let _lock = env_lock();
        shutdown_browser();
        let _mock = EnvGuard::set("CLAW_BROWSER_MOCK", Some("1"));
        let _open = EnvGuard::set("CLAW_BROWSER_AUTO_OPEN", Some("0"));
        let _backend = EnvGuard::set("CLAW_BROWSER_BACKEND", None);
        let home = temp_dir("execute-tool");
        let _home = EnvGuard::set("CLAW_CONFIG_HOME", Some(home.to_str().unwrap()));
        let output =
            crate::execute_tool("BrowserStart", &serde_json::json!({ "auto_open": false }))
                .expect("BrowserStart via execute_tool");
        assert!(output.contains("local"), "{output}");
        crate::execute_tool("BrowserStop", &serde_json::json!({ "noop": true })).expect("stop");
    }

    #[test]
    fn given_mock_driver_when_local_backend_then_start_without_steel_key() {
        let _lock = env_lock();
        shutdown_browser();
        let _mock = EnvGuard::set("CLAW_BROWSER_MOCK", Some("1"));
        let _open = EnvGuard::set("CLAW_BROWSER_AUTO_OPEN", Some("0"));
        let _backend = EnvGuard::set("CLAW_BROWSER_BACKEND", None);
        let _key = EnvGuard::set("STEEL_API_KEY", None);
        let home = temp_dir("local-backend");
        let _home = EnvGuard::set("CLAW_CONFIG_HOME", Some(home.to_str().unwrap()));
        let started = run_browser_start(BrowserStartInput {
            auto_open: Some(false),
            timeout_ms: None,
            backend: Some(String::from("local")),
        })
        .expect("mock local start");
        assert!(started.contains("local"), "{started}");
        crate::execute_tool("BrowserStop", &serde_json::json!({ "noop": true })).expect("stop");
    }

    #[test]
    fn given_oversized_snapshot_when_model_json_then_truncates() {
        let snapshot = (0..800)
            .map(|index| format!("[ref=e{index}] button \"Item {index}\""))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(snapshot.len() > MAX_BROWSER_SNAPSHOT_CHARS);
        let rendered = model_json(json!({
            "url": "https://example.com",
            "title": "Inbox",
            "snapshot": snapshot,
            "nodes": [{"ref": "e1", "role": "button", "name": "x", "tag": "button"}],
        }))
        .expect("model json");
        let parsed: Value = serde_json::from_str(&rendered).expect("parse");
        assert!(parsed.get("nodes").is_none());
        assert_eq!(parsed.get("truncated").and_then(Value::as_bool), Some(true));
        let text = parsed
            .get("snapshot")
            .and_then(Value::as_str)
            .expect("snapshot");
        assert!(text.contains("snapshot truncated"), "{text}");
        assert!(
            text.len() <= MAX_BROWSER_SNAPSHOT_CHARS + 32,
            "{}",
            text.len()
        );
    }

    #[test]
    fn given_empty_url_when_navigate_then_error() {
        let error = run_browser_navigate(BrowserNavigateInput {
            url: String::from("  "),
        })
        .expect_err("empty url");
        assert!(error.contains("url is required"));
    }

    #[test]
    fn abort_browser_is_safe_when_no_session() {
        abort_browser();
        abort_browser();
    }
}
