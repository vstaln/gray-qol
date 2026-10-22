mod detect;
mod host;
mod mascot;
mod paths;
mod setup;
mod sources;
mod state;
mod update;

use std::io::{BufRead, IsTerminal};
use std::sync::Arc;

use serde_json::{Value, json};

use host::Host;

pub const PLUGIN_NAME: &str = "qol";
pub const PROTOCOL: &str = "1.1";
pub const COMMAND: &str = "/qol";

pub const WELCOME: &str = "Welcome to gray by alignment\ngray is a minimal agent that runs tools, edits code, and works with any model provider.";

const USAGE: &str = "/qol setup [tool]   import rules, skills, MCP servers and a subscription plugin\n\
/qol undo [n]       revert the last n imports (default 1)\n\
/qol dismiss <tool> never mention a detected tool again\n\
/qol scanned        what the detector looked at and found\n\
/qol update         update provider CLIs, plugins and gray in one go\n\
/qol first-run      the first-run welcome";

fn manifest() -> Value {
    json!({
        "name": PLUGIN_NAME,
        "version": env!("CARGO_PKG_VERSION"),
        "protocol": PROTOCOL,
        "description": "Detect other agent tools, import their setup, update everything",
        "tools": [],
        "commands": [COMMAND, "/setup", "/hehe"],
        "hooks": ["session_start"],
        "first_run": true,
        "capabilities": ["host.say", "host.ask", "plugins.manage", "host.sessions", "host.restart"],
        "completion": ["setup", "undo", "dismiss", "scanned", "update", "first-run"],
    })
}

fn argv_of(params: &Value) -> Vec<String> {
    let argv: Vec<String> = params
        .get("argv")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|e| e.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    if !argv.is_empty() {
        return argv;
    }
    params
        .get("args")
        .and_then(Value::as_str)
        .map(|s| s.split_whitespace().map(str::to_string).collect())
        .unwrap_or_default()
}

fn command_run(host: &Host, params: &Value) -> Value {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or(COMMAND);
    let argv = argv_of(params);
    if name == "/hehe" {
        return json!({
            "terminal": ["hehe"],
            "text": "graychan needs a terminal — run /qol first-run or `gray qol hehe` outside gray",
        });
    }
    if name == "/setup" {
        // Same verb surface as the old core /setup: [tool] imports,
        // undo/dismiss/scanned manage the log.
        let words: Vec<&str> = argv.iter().map(String::as_str).collect();
        return match words.as_slice() {
            ["undo"] => setup::undo(1),
            ["undo", n] => match n.parse::<usize>() {
                Ok(n) if n > 0 => setup::undo(n),
                _ => json!({"text": "usage: /setup undo [n]"}),
            },
            ["dismiss", tool] => setup::dismiss(tool),
            ["dismiss"] => json!({"text": "usage: /setup dismiss <tool>"}),
            ["scanned"] => detect::scanned_report(),
            _ => setup::command(host, words.first().copied()),
        };
    }
    let session = params["session"]["id"].as_str().unwrap_or("").to_string();
    let words: Vec<&str> = argv.iter().map(String::as_str).collect();
    if name == "/update" {
        return match words.first() {
            Some(&"everything") => update::run(host, &session),
            _ => json!({"text": "usage: /update everything"}),
        };
    }
    match words.as_slice() {
        [] | ["help"] => json!({"text": USAGE}),
        ["setup", rest @ ..] => setup::command(host, rest.first().copied()),
        ["undo"] => setup::undo(1),
        ["undo", n] => match n.parse::<usize>() {
            Ok(n) if n > 0 => setup::undo(n),
            _ => json!({"text": "usage: /qol undo [n]"}),
        },
        ["dismiss", tool] => setup::dismiss(tool),
        ["dismiss"] => json!({"text": "usage: /qol dismiss <tool>"}),
        ["scanned"] => detect::scanned_report(),
        ["update", ..] => update::run(host, &session),
        ["first-run"] => setup::first_run(host),
        _ => json!({"text": format!("unknown: /qol {}\n{USAGE}", argv.join(" "))}),
    }
}

fn on_event(host: &Host, params: &Value) {
    if params.get("type").and_then(Value::as_str) != Some("session_start") {
        return;
    }
    if params.get("interactive").and_then(Value::as_bool) == Some(false) {
        return;
    }
    detect::notify(host);
}

fn handle(host: &Host, id: Value, method: &str, params: &Value) -> Value {
    match method {
        "plugin/manifest" => json!({"id": id, "result": manifest()}),
        "command/run" => json!({"id": id, "result": command_run(host, params)}),
        other => json!({"id": id, "error": {
            "code": -32601,
            "message": format!("method not found: {other}")
        }}),
    }
}

fn sidecar() {
    let host = Arc::new(Host::new());
    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let Ok(v) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let method = v.get("method").and_then(Value::as_str).unwrap_or("");
        if method == "plugin/shutdown" {
            break;
        }
        if method.is_empty() {
            if let Some(id) = v.get("id").and_then(Value::as_str) {
                host.deliver(id, v.clone());
            }
            continue;
        }
        let params = v.get("params").cloned().unwrap_or(Value::Null);
        let Some(id) = v.get("id").cloned() else {
            if method == "event/notify" {
                let host = host.clone();
                std::thread::spawn(move || on_event(&host, &params));
            }
            continue;
        };
        let method = method.to_string();
        let host = host.clone();
        std::thread::spawn(move || {
            let reply = handle(&host, id, &method, &params);
            host.write(&reply);
        });
    }
    std::process::exit(0);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("manifest") => println!("{}", manifest()),
        Some("sidecar") => sidecar(),
        Some("update") => update::cli(),
        Some("hehe") => {
            if !mascot::print_mascot() {
                println!("graychan needs a terminal (20x10+) and a little color");
            }
        }
        Some("scanned") => {
            println!(
                "{}",
                detect::scanned_report()["text"].as_str().unwrap_or("")
            )
        }
        Some(_) => println!("{USAGE}"),
        None if std::io::stdin().is_terminal() => {
            eprintln!("qol is a gray plugin sidecar — install with `gray plugin install`");
        }
        None => sidecar(),
    }
}
