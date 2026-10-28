//! /intro — the logo-intro toggle. The flag lives in config.json's `"intro"`
//! field (the same one core's composer reads); this plugin owns the verb.

use serde_json::{Value, json};

use crate::paths::gray_home;

fn read_flag() -> Option<bool> {
    let raw = std::fs::read_to_string(gray_home().join("config.json")).ok()?;
    serde_json::from_str::<Value>(&raw).ok()?["intro"].as_bool()
}

fn write_flag(on: bool) -> Result<(), String> {
    let path = gray_home().join("config.json");
    let mut doc: Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| json!({}));
    if !doc.is_object() {
        doc = json!({});
    }
    doc["intro"] = json!(on);
    let text = serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())?;
    crate::paths::write_atomic(&path, &(text + "\n")).map_err(|e| e.to_string())
}

/// `command/run` for /intro — `on|off`, bare reports state.
pub fn intro_command(argv: &[String]) -> Value {
    match argv.first().map(String::as_str) {
        Some("on") | Some("off") => {
            let on = argv[0] == "on";
            match write_flag(on) {
                Ok(()) => json!({"text": if on {
                    "intro on — the logo animates at startup"
                } else {
                    "intro off — the settled banner shows instead"
                }}),
                Err(e) => json!({"text": format!("intro not saved: {e}")}),
            }
        }
        None => json!({"text": format!(
            "intro is {} — /intro on|off",
            if read_flag().unwrap_or(true) { "on" } else { "off" }
        )}),
        Some(other) => json!({"text": format!("usage: /intro [on|off] (got `{other}`)")}),
    }
}
