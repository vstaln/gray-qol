use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::host::Host;
use crate::{paths, prompt, settings};

const WARN_AT: [f64; 3] = [75.0, 90.0, 95.0];
const REARM_DROP: f64 = 10.0;
const MIN_GAP: Duration = Duration::from_secs(300);
const LIST_TIMEOUT: Duration = Duration::from_secs(4);
const USAGE_TIMEOUT: Duration = Duration::from_secs(25);
const FORGET_SECS: u64 = 14 * 86_400;

static LAST: Mutex<Option<(String, Instant)>> = Mutex::new(None);

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq)]
struct Warned {
    at: f64,
    seen: u64,
}

pub fn turn_end(host: &Host, params: &Value) {
    if !prompt::interactive(params) || !settings::on("usage_warnings") {
        return;
    }
    let Some(provider_id) = params["session"]["model"]["provider_id"].as_str() else {
        return;
    };
    {
        let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((id, at)) = last.as_ref()
            && id == provider_id
            && at.elapsed() < MIN_GAP
        {
            return;
        }
        *last = Some((provider_id.to_string(), Instant::now()));
    }
    for note in warnings(provider_id) {
        host.say(&note);
    }
}

fn warnings(provider_id: &str) -> Vec<String> {
    let plugin = provider_id.split(':').next().unwrap_or_default();
    let Some(argv) = cli_argv(plugin) else {
        return Vec::new();
    };
    let Ok(list) = run_json(&argv, &["account", "list", "--json"], LIST_TIMEOUT) else {
        return Vec::new();
    };
    let serving = serving(&list);
    let mut args = vec!["account", "usage", "--json"];
    args.extend(serving.as_deref());
    let Ok(doc) = run_json(&argv, &args, USAGE_TIMEOUT) else {
        return Vec::new();
    };
    let provider = doc["provider"]
        .as_str()
        .or_else(|| doc["plugin"].as_str())
        .or_else(|| list["provider"].as_str())
        .unwrap_or(plugin);
    let provider = provider.rsplit(':').next().unwrap_or(provider).trim();
    let Some(account) = doc["accounts"].as_array().and_then(|accounts| {
        accounts
            .iter()
            .find(|a| a["label"].as_str() == serving.as_deref())
            .or_else(|| accounts.first())
    }) else {
        return Vec::new();
    };
    let usage = &account["usage"];
    let title = usage["title"].as_str();
    let who = serving.as_deref();
    let now = paths::now();
    let mut warned = load();
    let mut out = Vec::new();
    for window in usage["windows"].as_array().into_iter().flatten() {
        let Some(id) = window["id"].as_str().or_else(|| window["label"].as_str()) else {
            continue;
        };
        let Some(pct) = window["used_percent"].as_f64() else {
            continue;
        };
        let key = format!("{provider}|{}|{}", who.unwrap_or(""), id.to_lowercase());
        if let Some(prev) = warned.get(&key).copied()
            && pct < prev.at - REARM_DROP
        {
            warned.remove(&key);
        }
        if pct >= 100.0 {
            continue;
        }
        let Some(&at) = WARN_AT.iter().rev().find(|&&t| pct >= t) else {
            continue;
        };
        match warned.get_mut(&key) {
            Some(prev) if prev.at >= at => {
                prev.seen = now;
                continue;
            }
            _ => {}
        }
        warned.insert(key, Warned { at, seen: now });
        let label = window["label"]
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(id);
        let name = match title {
            Some(t) => format!("{t} limit ({label})"),
            None => format!("{label} limit"),
        };
        let on = who.map(|a| format!(" on {a}")).unwrap_or_default();
        let resets = window["resets_at"]
            .as_str()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|t| {
                let local = t.with_timezone(&chrono::Local);
                if t.timestamp() - now as i64 > 20 * 3_600 {
                    format!(" It resets {}.", local.format("%a %H:%M"))
                } else {
                    format!(" It resets at {}.", local.format("%H:%M"))
                }
            })
            .unwrap_or_default();
        out.push(format!(
            "Heads up, you've used over {at:.0}% of your {name}{on}.{resets}"
        ));
    }
    warned.retain(|_, w| now.saturating_sub(w.seen) < FORGET_SECS);
    save(&warned);
    out
}

fn serving(doc: &Value) -> Option<String> {
    match doc["serving"]["kind"].as_str() {
        Some("account") => return doc["serving"]["label"].as_str().map(str::to_string),
        Some(_) => return None,
        None => {}
    }
    let accounts = doc["accounts"].as_array()?;
    accounts
        .iter()
        .find(|a| a["pinned"] == true)
        .or_else(|| accounts.first())
        .and_then(|a| a["label"].as_str())
        .map(str::to_string)
}

fn cli_argv(plugin: &str) -> Option<Vec<String>> {
    let raw = std::fs::read(paths::gray_home().join("plugins").join("lock.json")).ok()?;
    let lock: Value = serde_json::from_slice(&raw).ok()?;
    let entry = &lock["plugins"][plugin];
    if entry["enabled"] == false {
        return None;
    }
    let pick = |v: &Value| -> Option<Vec<String>> {
        let argv: Vec<String> = v
            .as_array()?
            .iter()
            .filter_map(|s| s.as_str().map(str::to_string))
            .collect();
        (!argv.is_empty()).then_some(argv)
    };
    pick(&entry["cli_argv"]).or_else(|| pick(&entry["argv"]))
}

fn run_json(argv: &[String], args: &[&str], timeout: Duration) -> Result<Value, String> {
    let (prog, base) = argv.split_first().ok_or("empty argv")?;
    let mut child = std::process::Command::new(prog)
        .args(base)
        .args(args)
        .env("GRAY_BIN", paths::gray_bin())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(15)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("timed out".into());
            }
            Err(e) => return Err(e.to_string()),
        }
    }
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err("command failed".into());
    }
    serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())
}

fn path() -> PathBuf {
    paths::qol_dir().join("usage-warned.json")
}

fn load() -> BTreeMap<String, Warned> {
    std::fs::read(path())
        .ok()
        .and_then(|raw| serde_json::from_slice(&raw).ok())
        .unwrap_or_default()
}

fn save(warned: &BTreeMap<String, Warned>) {
    let path = path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(text) = serde_json::to_string(warned) {
        let _ = paths::write_atomic(&path, &text);
    }
}
