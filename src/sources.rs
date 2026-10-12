use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use crate::paths;

pub const MASK: &str = "•••";

pub fn rules_files(tool: &str) -> &'static [&'static str] {
    match tool {
        "claude" => &[".claude/CLAUDE.md"],
        "codex" => &[".codex/AGENTS.md"],
        "opencode" => &[".config/opencode/AGENTS.md"],
        "devin" => &[".config/devin/AGENTS.md"],
        "windsurf" => &[".codeium/windsurf/memories/global_rules.md"],
        _ => &[],
    }
}

pub fn skill_dirs(tool: &str) -> &'static [&'static str] {
    match tool {
        "claude" => &[".claude/skills"],
        "codex" => &[".codex/skills"],
        "opencode" => &[".config/opencode/skills", ".config/opencode/skill"],
        "devin" => &[".config/devin/skills"],
        "cursor" => &[".cursor/skills"],
        _ => &[],
    }
}

pub struct SubPlugin {
    pub spec: &'static str,
    pub lock_names: &'static [&'static str],
}

pub fn sub_plugin(tool: &str) -> Option<SubPlugin> {
    let (spec, lock_names): (&str, &[&str]) = match tool {
        "claude" => ("claude", &["claude", "claude-sub"]),
        "codex" => ("codex", &["codex", "codex-sub"]),
        "opencode" => ("opencode-sub", &["opencode", "opencode-sub"]),
        "devin" => ("devin", &["devin", "devin-sub"]),
        _ => return None,
    };
    Some(SubPlugin { spec, lock_names })
}

pub fn plugin_installed(names: &[&str]) -> bool {
    let lock = paths::gray_home().join("plugins/lock.json");
    let Some(v) = std::fs::read_to_string(lock)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
    else {
        return false;
    };
    names.iter().any(|n| {
        v["plugins"]
            .get(*n)
            .is_some_and(|e| e.get("removed").and_then(Value::as_bool) != Some(true))
    })
}

pub fn skills(tool: &str) -> Vec<(String, PathBuf)> {
    let home = paths::home();
    let mut out: Vec<(String, PathBuf)> = Vec::new();
    for d in skill_dirs(tool) {
        let Ok(rd) = std::fs::read_dir(home.join(d)) else {
            continue;
        };
        let mut entries: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
        entries.sort();
        for p in entries {
            if !p.join("SKILL.md").is_file() {
                continue;
            }
            let Some(name) = p.file_name().map(|n| n.to_string_lossy().to_string()) else {
                continue;
            };
            if out.iter().any(|(n, _)| *n == name) {
                continue;
            }
            out.push((name, p));
        }
    }
    out
}

pub fn strip_jsonc(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let mut in_str = false;
    while i < chars.len() {
        let c = chars[i];
        if in_str {
            out.push(c);
            if c == '\\' && i + 1 < chars.len() {
                out.push(chars[i + 1]);
                i += 2;
                continue;
            }
            if c == '"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        match (c, chars.get(i + 1)) {
            ('"', _) => {
                in_str = true;
                out.push(c);
                i += 1;
            }
            ('/', Some('/')) => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            ('/', Some('*')) => {
                i += 2;
                while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                    i += 1;
                }
                i += 2;
            }
            (',', _) => {
                let mut j = i + 1;
                while j < chars.len() && chars[j].is_whitespace() {
                    j += 1;
                }
                if !matches!(chars.get(j), Some('}') | Some(']')) {
                    out.push(c);
                }
                i += 1;
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

fn read_json(path: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text)
        .ok()
        .or_else(|| serde_json::from_str(&strip_jsonc(&text)).ok())
}

fn str_map(v: Option<&Value>) -> Map<String, Value> {
    v.and_then(Value::as_object)
        .map(|m| {
            m.iter()
                .filter(|(_, v)| v.is_string())
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        })
        .unwrap_or_default()
}

fn str_list(v: Option<&Value>) -> Vec<Value> {
    v.and_then(Value::as_array)
        .map(|a| a.iter().filter(|e| e.is_string()).cloned().collect())
        .unwrap_or_default()
}

fn entry(
    command: Option<String>,
    args: Vec<Value>,
    env: Map<String, Value>,
    url: Option<String>,
    headers: Map<String, Value>,
    disabled: bool,
) -> Option<Value> {
    let mut e = Map::new();
    if let Some(c) = command.filter(|c| !c.is_empty()) {
        e.insert("command".into(), json!(c));
        if !args.is_empty() {
            e.insert("args".into(), Value::Array(args));
        }
        if !env.is_empty() {
            e.insert("env".into(), Value::Object(env));
        }
    } else {
        let u = url.filter(|u| !u.is_empty())?;
        e.insert("url".into(), json!(u));
        if !headers.is_empty() {
            e.insert("headers".into(), Value::Object(headers));
        }
    }
    if disabled {
        e.insert("disabled".into(), json!(true));
    }
    Some(Value::Object(e))
}

fn claude_style(v: &Value) -> Option<Value> {
    entry(
        v.get("command").and_then(Value::as_str).map(str::to_string),
        str_list(v.get("args")),
        str_map(v.get("env")),
        v.get("url")
            .or_else(|| v.get("serverUrl"))
            .and_then(Value::as_str)
            .map(str::to_string),
        str_map(v.get("headers")),
        v.get("disabled").and_then(Value::as_bool) == Some(true),
    )
}

fn servers_from(map: Option<&Value>, f: impl Fn(&Value) -> Option<Value>) -> Vec<(String, Value)> {
    map.and_then(Value::as_object)
        .map(|m| {
            m.iter()
                .filter_map(|(k, v)| f(v).map(|e| (k.clone(), e)))
                .collect()
        })
        .unwrap_or_default()
}

pub fn mcp_source(tool: &str) -> Option<PathBuf> {
    let home = paths::home();
    let p = match tool {
        "claude" => home.join(".claude.json"),
        "codex" => home.join(".codex/config.toml"),
        "opencode" => {
            let json = home.join(".config/opencode/opencode.json");
            if json.is_file() {
                json
            } else {
                home.join(".config/opencode/opencode.jsonc")
            }
        }
        "devin" => home.join(".config/devin/mcp_config.json"),
        "cursor" => home.join(".cursor/mcp.json"),
        "windsurf" => home.join(".codeium/windsurf/mcp_config.json"),
        _ => return None,
    };
    p.is_file().then_some(p)
}

pub fn mcp_servers(tool: &str) -> Vec<(String, Value)> {
    let Some(path) = mcp_source(tool) else {
        return Vec::new();
    };
    match tool {
        "codex" => codex_servers(&path),
        "opencode" => {
            let Some(v) = read_json(&path) else {
                return Vec::new();
            };
            servers_from(v.get("mcp"), |s| {
                let cmd = str_list(s.get("command"));
                let (command, args) = match cmd.split_first() {
                    Some((c, rest)) => (c.as_str().map(str::to_string), rest.to_vec()),
                    None => (None, Vec::new()),
                };
                entry(
                    command,
                    args,
                    str_map(s.get("environment")),
                    s.get("url").and_then(Value::as_str).map(str::to_string),
                    str_map(s.get("headers")),
                    s.get("enabled").and_then(Value::as_bool) == Some(false),
                )
            })
        }
        _ => {
            let Some(v) = read_json(&path) else {
                return Vec::new();
            };
            servers_from(v.get("mcpServers"), claude_style)
        }
    }
}

fn codex_servers(path: &Path) -> Vec<(String, Value)> {
    let Some(v) = std::fs::read_to_string(path)
        .ok()
        .and_then(|t| toml::from_str::<toml::Value>(&t).ok())
        .and_then(|t| serde_json::to_value(t).ok())
    else {
        return Vec::new();
    };
    servers_from(v.get("mcp_servers"), |s| {
        let mut headers = str_map(s.get("http_headers"));
        if let Some(var) = s.get("bearer_token_env_var").and_then(Value::as_str) {
            headers.insert("Authorization".into(), json!(format!("Bearer ${{{var}}}")));
        }
        entry(
            s.get("command").and_then(Value::as_str).map(str::to_string),
            str_list(s.get("args")),
            str_map(s.get("env")),
            s.get("url").and_then(Value::as_str).map(str::to_string),
            headers,
            s.get("enabled").and_then(Value::as_bool) == Some(false),
        )
    })
}

fn secretish(s: &str) -> bool {
    let l = s.to_ascii_lowercase();
    [
        "key", "token", "secret", "password", "passwd", "auth", "bearer", "cred",
    ]
    .iter()
    .any(|w| l.contains(w))
}

fn mask_arg(prev: Option<&str>, arg: &str) -> String {
    if let Some((k, _)) = arg.split_once('=')
        && secretish(k)
    {
        return format!("{k}={MASK}");
    }
    if prev.is_some_and(|p| p.starts_with('-') && secretish(p)) {
        return MASK.to_string();
    }
    if arg.len() >= 24
        && !arg.contains('/')
        && arg
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
        && arg.chars().any(|c| c.is_ascii_digit())
    {
        return MASK.to_string();
    }
    mask_url(arg)
}

fn mask_url(s: &str) -> String {
    if !(s.starts_with("http://") || s.starts_with("https://")) {
        return s.to_string();
    }
    let s = match s.split_once('?') {
        Some((base, _)) => format!("{base}?{MASK}"),
        None => s.to_string(),
    };
    match s.split_once("://") {
        Some((scheme, rest)) if rest.split('/').next().is_some_and(|h| h.contains('@')) => {
            let after = rest.split_once('@').map(|(_, a)| a).unwrap_or(rest);
            format!("{scheme}://{MASK}@{after}")
        }
        _ => s,
    }
}

pub fn describe(entry: &Value) -> String {
    let mut out = String::new();
    if let Some(c) = entry.get("command").and_then(Value::as_str) {
        out.push_str(c);
        let args = str_list(entry.get("args"));
        let mut prev: Option<&str> = None;
        for a in args.iter().filter_map(Value::as_str) {
            out.push(' ');
            out.push_str(&mask_arg(prev, a));
            prev = Some(a);
        }
    } else if let Some(u) = entry.get("url").and_then(Value::as_str) {
        out.push_str(&mask_url(u));
    }
    for key in ["env", "headers"] {
        let m = str_map(entry.get(key));
        if !m.is_empty() {
            let keys: Vec<String> = m.keys().map(|k| format!("{k}={MASK}")).collect();
            out.push_str(&format!(" [{key}: {}]", keys.join(", ")));
        }
    }
    if entry.get("disabled").and_then(Value::as_bool) == Some(true) {
        out.push_str(" (disabled)");
    }
    out
}

pub fn env_keys() -> Vec<String> {
    let mut keys: Vec<String> = std::env::vars_os()
        .filter_map(|(k, v)| {
            let k = k.to_string_lossy().to_string();
            (k.ends_with("_API_KEY") && !v.is_empty()).then_some(k)
        })
        .collect();
    keys.sort();
    keys
}
