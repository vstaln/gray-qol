//! /copy — the last assistant message of the current session to the OS
//! clipboard. Core's version read the live agent; the session jsonl holds
//! the same text, so the plugin reads `sessions/<sid>.jsonl` (sid arrives in
//! command/run params.session.id; newest file is the fallback).

use serde_json::{json, Value};

use crate::paths::gray_home;

fn last_assistant_text(path: &std::path::Path) -> Option<String> {
    let raw = std::fs::read_to_string(path).ok()?;
    raw.lines().rev().find_map(|line| {
        if !line.contains("\"assistant\"") {
            return None;
        }
        let v: Value = serde_json::from_str(line).ok()?;
        if v["message"]["role"].as_str() != Some("assistant") {
            return None;
        }
        let text: String = v["message"]["content"]
            .as_array()?
            .iter()
            .filter_map(|b| (b["type"].as_str() == Some("text")).then(|| b["text"].as_str().unwrap_or("").to_string()))
            .collect::<Vec<_>>()
            .join("\n");
        (!text.trim().is_empty()).then(|| text.trim().to_string())
    })
}

fn session_path(sid: &str) -> Option<std::path::PathBuf> {
    if !sid.is_empty() {
        let p = gray_home().join("sessions").join(format!("{sid}.jsonl"));
        if p.is_file() {
            return Some(p);
        }
    }
    std::fs::read_dir(gray_home().join("sessions"))
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
        .max_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok())
}

/// pbcopy / clip / wl-copy / xclip / xsel — first helper found wins.
fn copy_to_clipboard(text: &str) -> bool {
    #[cfg(target_os = "macos")]
    let helpers: &[(&str, &[&str])] = &[("pbcopy", &[])];
    #[cfg(target_os = "windows")]
    let helpers: &[(&str, &[&str])] = &[("clip", &[])];
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let helpers: &[(&str, &[&str])] = &[
        ("wl-copy", &[]),
        ("xclip", &["-selection", "clipboard"]),
        ("xsel", &["--clipboard", "--input"]),
    ];
    for (bin, args) in helpers {
        let Ok(mut c) = std::process::Command::new(bin)
            .args(*args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        else {
            continue;
        };
        use std::io::Write;
        let ok = c
            .stdin
            .as_mut()
            .is_some_and(|i| i.write_all(text.as_bytes()).is_ok())
            && c.wait().is_ok_and(|s| s.success());
        if ok {
            return true;
        }
    }
    false
}

/// `command/run` for /copy — `params.session.id` names the live session.
pub fn copy_command(session_id: &str) -> Value {
    let Some(path) = session_path(session_id) else {
        return json!({"text": "nothing to copy — no session file found"});
    };
    let Some(text) = last_assistant_text(&path) else {
        return json!({"text": "nothing to copy yet — no assistant response this session"});
    };
    if copy_to_clipboard(&text) {
        json!({"text": format!("copied {} chars to clipboard", text.chars().count())})
    } else {
        json!({"text": format!("no clipboard available — response:\n{text}")})
    }
}
