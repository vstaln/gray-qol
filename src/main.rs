mod auto_continue;
mod cd;
mod copy;
mod detect;
mod export;
mod host;
mod hotkeys;
mod intro;
mod links;
mod notify;
mod panel;
mod paths;
mod prompt;
mod recap;
mod sessions;
mod settings;
mod setup;
mod sources;
mod state;
mod suggest;
mod templates;
mod title;
mod update;
mod usage;
mod usage_warn;

use std::io::{BufRead, IsTerminal};
use std::sync::Arc;

use serde_json::{Value, json};

use host::Host;

pub const PLUGIN_NAME: &str = "quality-of-life";
pub const PROTOCOL: &str = "1.1";
pub const COMMAND: &str = "/qol";

pub const WELCOME: &str = "Welcome to gray by alignment\ngray is a minimal agent that runs tools, edits code, and works with any model provider.";

const USAGE: &str = "/qol               sessions, features, detected tools, updates\n\
/qol setup [tool]   import rules, skills, MCP servers and a subscription plugin\n\
/qol undo [n]       revert the last n imports (default 1)\n\
/qol dismiss <tool> never mention a detected tool again\n\
/qol scanned        what the detector looked at and found\n\
/qol update         update provider CLIs, plugins and gray in one go\n\
/qol first-run      the first-run welcome\n\
/qol sessions dismiss  clear the interrupted-session hint (Interrupted notice in /qol hides it for good)\n\
\n\
/cd [path]          change gray's working directory and resume here (alias: /chdir)";

const COMMANDS: &[(&str, &str)] = &[
    ("/qol", "detect other agent tools and import their setup"),
    (
        "/setup",
        "import rules, skills and MCP servers from other tools",
    ),
    ("/cost", "show token usage and cost estimates"),
    ("/copy", "copy the last reply to the clipboard"),
    (
        "/export",
        "export the session as markdown or text (clipboard or file)",
    ),
    ("/hotkeys", "show keybindings and your overrides"),
    ("/intro", "toggle the logo intro"),
    ("/cd", "change the working directory and resume here"),
    ("/chdir", "change the working directory and resume here"),
];

fn manifest() -> Value {
    let prompts = templates::list();
    let mut commands: Vec<String> = COMMANDS
        .iter()
        .map(|(name, _)| (*name).to_string())
        .collect();
    let mut descriptions = serde_json::Map::new();
    for (name, description) in COMMANDS {
        descriptions.insert((*name).to_string(), json!(description));
    }
    let mut completion: Vec<String> = [
        "setup",
        "undo",
        "dismiss",
        "scanned",
        "update",
        "first-run",
        "cd",
        "sessions",
    ]
    .iter()
    .map(|s| (*s).to_string())
    .collect();
    for template in &prompts {
        let name = format!("/{}", template.name);
        descriptions.insert(name.clone(), json!(template.description));
        commands.push(name);
        completion.push(template.name.clone());
    }
    json!({
        "name": PLUGIN_NAME,
        "version": env!("CARGO_PKG_VERSION"),
        "protocol": PROTOCOL,
        "description": "Detect other agent tools, import their setup, update everything",
        "tools": [],
        "commands": commands,
        "command_descriptions": descriptions,
        "turn_local": {"/cost": ["*"], "/copy": ["*"], "/export": ["*"], "/hotkeys": ["*"], "/intro": ["*"], "/qol": ["scanned"]},
        "hooks": ["session_start", "turn_end", "user_return"],
        "first_run": true,
        "capabilities": [
            "host.say",
            "host.links",
            "host.turn",
            "host.sessions",
            "session.read",
            "session.write",
            "composer.ghost",
            "plugins.manage",
            "host.ask",
            "host.restart",
        ],
        "settings": settings::manifest_rows(),
        "completion": completion,
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
    if name == "/cd" || name == "/chdir" {
        return cd::command(host, &argv);
    }
    let session = params["session"]["id"].as_str().unwrap_or("").to_string();
    let words: Vec<&str> = argv.iter().map(String::as_str).collect();
    match name {
        "/cost" => return usage::usage_command(&argv),
        "/copy" => return copy::copy_command(&session),
        "/export" => return export::export_command(Some(host), &session, &argv),
        "/hotkeys" => return hotkeys::hotkeys_command(&argv),
        "/intro" => return intro::intro_command(&argv),
        _ => {}
    }
    let rest = params.get("raw").and_then(Value::as_str).unwrap_or("");
    if let Some(outcome) = templates::run(&templates::list(), name, rest) {
        return outcome;
    }
    if name == "/update" {
        return match words.first() {
            Some(&"everything") => update::run(host, &session),
            _ => json!({"text": "usage: /update everything"}),
        };
    }
    match words.as_slice() {
        [] => panel::open(host, params),
        ["--pick", .., pick] => panel::pick(host, params, &session, pick),
        ["help"] => json!({"text": USAGE}),
        ["setup", rest @ ..] => setup::command(host, rest.first().copied()),
        ["cd", ..] | ["chdir", ..] => cd::command(host, &argv[1..]),
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
        ["sessions", "dismiss"] => sessions::dismiss_command(host, params),
        ["sessions"] => json!({"text": "usage: /qol sessions dismiss"}),
        _ => json!({"text": format!("unknown: /qol {}\n{USAGE}", argv.join(" "))}),
    }
}

fn on_event(host: &Arc<Host>, params: &Value) {
    match params.get("type").and_then(Value::as_str) {
        Some("session_start") => {
            templates::session_start(host, params);
            links::session_start(host, params);
            if prompt::interactive(params) {
                sessions::session_start(host, params);
                detect::notify(host);
                let host = Arc::clone(host);
                let _ = std::thread::spawn(move || update::notify(&host));
            }
        }
        Some("user_return") => {
            sessions::refresh(host, params);
            recap::user_return(host, params);
        }
        Some("turn_end") => {
            usage::record_turn(&prompt::session_id(params), &params["usage"]);
            sessions::refresh(host, params);
            auto_continue::turn_end(host, params);
            notify::turn_end(host, params);
            for worker in [title::turn_end, suggest::turn_end, usage_warn::turn_end] {
                let host = Arc::clone(host);
                let params = params.clone();
                let _ = std::thread::spawn(move || worker(&host, &params));
            }
        }
        _ => {}
    }
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
    let mut workers: Vec<std::thread::JoinHandle<()>> = Vec::new();
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
                workers.push(std::thread::spawn(move || on_event(&host, &params)));
            }
            continue;
        };
        let method = method.to_string();
        let host = host.clone();
        workers.push(std::thread::spawn(move || {
            let reply = handle(&host, id, &method, &params);
            host.write(&reply);
        }));
    }
    // stdin closed — let in-flight workers finish their replies before
    // exit, or a fast harness (tests, conformance probes) loses them.
    for w in workers {
        let _ = w.join();
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("manifest") => println!("{}", manifest()),
        Some("sidecar") => sidecar(),
        Some("update") => update::cli(),
        Some("usage") | Some("cost") => {
            let _ = args;
            println!(
                "{}",
                usage::usage_command(&args[1..])["text"]
                    .as_str()
                    .unwrap_or("")
            );
        }
        Some("copy") => println!("{}", copy::copy_command("")["text"].as_str().unwrap_or("")),
        Some("export") => println!(
            "{}",
            export::export_command(None, "", &args[1..])["text"]
                .as_str()
                .unwrap_or("")
        ),
        Some("hotkeys") => println!(
            "{}",
            hotkeys::hotkeys_command(&args[1..])["text"]
                .as_str()
                .unwrap_or("")
        ),
        Some("intro") => println!(
            "{}",
            intro::intro_command(&args[1..])["text"]
                .as_str()
                .unwrap_or("")
        ),
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
