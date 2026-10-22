//! /hotkeys — the default keybinding table plus ~/.gray/keybindings.json
//! overrides (`*` marks user-set; `"/cmd": "ctrl+x"` binds a command).
//! `reload` is a no-op here — keybindings reload is a core /reload job.

use serde_json::{json, Value};
use std::collections::BTreeMap;

use crate::paths::gray_home;

/// (id, describe, default chords) — the same triple core's keymap ships.
const DEFAULTS: &[(&str, &str, &[&str])] = &[
    ("tui.select.up", "Move popup selection up", &["up", "ctrl+p"]),
    ("tui.select.down", "Move popup selection down", &["down", "ctrl+n"]),
    ("tui.select.cancel", "Close the completion popup", &["escape"]),
    ("tui.input.tab", "Complete the selected entry", &["tab"]),
    ("app.clear", "Clear the draft, or exit on an empty prompt", &["ctrl+c"]),
    ("app.exit", "Exit when the draft is empty", &["ctrl+d"]),
    ("app.interrupt", "Cancel the turn, or clear the draft", &["escape"]),
    ("tui.paste", "Paste image or text from the clipboard", &["ctrl+v", "ctrl+shift+v", "shift+insert", "alt+v"]),
    ("tui.newline", "Insert a new line", &["shift+enter", "alt+enter", "ctrl+j"]),
    ("tui.submit", "Send (queues while a turn runs)", &["enter"]),
    ("tui.history.previous", "Previous prompt from history", &[]),
    ("tui.history.next", "Next prompt from history", &[]),
    ("tui.history.search", "Reverse-search prompt history", &["ctrl+r"]),
    ("tui.cursor.up", "Cursor up, history at the top", &["up"]),
    ("tui.cursor.down", "Cursor down, history at the bottom", &["down"]),
    ("tui.cursor.left", "Cursor left", &["left", "ctrl+b"]),
    ("tui.cursor.right", "Cursor right", &["right", "ctrl+f"]),
    ("tui.cursor.word_left", "Cursor word left", &["alt+left", "ctrl+left", "alt+b"]),
    ("tui.cursor.word_right", "Cursor word right", &["alt+right", "ctrl+right", "alt+f"]),
    ("tui.cursor.line_start", "Cursor to line start", &["home", "ctrl+a"]),
    ("tui.cursor.line_end", "Cursor to line end", &["end", "ctrl+e"]),
    ("tui.delete.backward", "Delete character backward", &["backspace"]),
    ("tui.delete.forward", "Delete character forward", &["delete", "ctrl+d"]),
    ("tui.delete.word_backward", "Delete word backward", &["ctrl+w", "alt+backspace", "ctrl+backspace"]),
    ("tui.delete.word_forward", "Delete word forward", &["alt+d", "alt+delete", "ctrl+delete"]),
    ("tui.delete.to_line_start", "Delete to line start", &["ctrl+u"]),
    ("tui.delete.to_line_end", "Delete to line end", &["ctrl+k"]),
    ("tui.yank", "Yank back the last cleared draft", &["ctrl+y"]),
];

fn overrides() -> (BTreeMap<String, Vec<String>>, Vec<(String, String)>) {
    let Ok(raw) = std::fs::read_to_string(gray_home().join("keybindings.json")) else {
        return (BTreeMap::new(), Vec::new());
    };
    let Ok(v) = serde_json::from_str::<Value>(&raw) else {
        return (BTreeMap::new(), Vec::new());
    };
    let mut keys = BTreeMap::new();
    let mut cmds = Vec::new();
    if let Some(m) = v.as_object() {
        for (id, chord) in m {
            let chords: Vec<String> = match chord {
                Value::String(s) => vec![s.clone()],
                Value::Array(a) => a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect(),
                Value::Null => Vec::new(),
                _ => continue,
            };
            if id.starts_with('/') {
                for c in chords {
                    cmds.push((c, id.clone()));
                }
            } else {
                keys.insert(id.clone(), chords);
            }
        }
    }
    (keys, cmds)
}

fn render() -> String {
    let path = gray_home().join("keybindings.json");
    let (user, cmds) = overrides();
    let mut out = String::from("keybindings:");
    for (id, desc, defaults) in DEFAULTS {
        let custom = user.get(*id);
        let keys = custom
            .map(|c| c.join(", "))
            .unwrap_or_else(|| if defaults.is_empty() { "—".into() } else { defaults.join(", ") });
        let mark = if custom.is_some() { "*" } else { " " };
        out.push_str(&format!("\n {mark} {keys:<46} {id:<28} {desc}"));
    }
    if !cmds.is_empty() {
        out.push_str("\ncommands:");
        for (chord, cmd) in cmds {
            out.push_str(&format!("\n * {chord:<22} {cmd}"));
        }
    }
    out.push_str(&format!(
        "\n\n* = set in {} (pi's ids and key syntax; `\"/cmd\": \"ctrl+x\"` binds a command; /reload applies edits)",
        path.display()
    ));
    out
}

/// `command/run` for /hotkeys.
pub fn hotkeys_command(argv: &[String]) -> Value {
    match argv.first().map(String::as_str) {
        None | Some("list") => json!({"text": render()}),
        Some("reload") => json!({"text": "keybindings reload is a core job — /reload refreshes keys, theme, templates"}),
        Some(other) => json!({"text": format!("usage: /hotkeys [reload] (got `{other}`)")}),
    }
}
