use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use crate::detect::{self, ToolDef};
use crate::host::{self, Host};
use crate::paths;
use crate::sources::{self, MASK};
use crate::state::{Action, Import, State};

const PREVIEW_LINES: usize = 14;

enum Kind {
    Rules { src: PathBuf, content: String },
    Skills { links: Vec<(String, PathBuf)> },
    Mcp { servers: Vec<(String, Value)> },
    Plugin { spec: String },
    Env { keys: Vec<String> },
}

struct Item {
    tool: Option<&'static ToolDef>,
    label: String,
    preview: String,
    kind: Kind,
}

fn agents_md() -> PathBuf {
    paths::gray_home().join("AGENTS.md")
}

fn skills_dir() -> PathBuf {
    paths::gray_home().join("skills")
}

fn mcp_json() -> PathBuf {
    paths::gray_home().join("mcp.json")
}

fn marker(tool: &str, src: &Path) -> String {
    format!("gray-qol:{tool}:{}", paths::tilde(src))
}

fn begin(marker: &str) -> String {
    format!("<!-- BEGIN {marker} -->")
}

fn end(marker: &str) -> String {
    format!("<!-- END {marker} -->")
}

fn section(tool: &ToolDef, src: &Path, content: &str) -> String {
    let m = marker(tool.id, src);
    format!(
        "{}\n## Imported from {} ({})\n\n{}\n{}\n",
        begin(&m),
        tool.name,
        paths::tilde(src),
        content.trim(),
        end(&m)
    )
}

fn clip(lines: Vec<String>) -> String {
    let total = lines.len();
    let mut out: Vec<String> = lines.into_iter().take(PREVIEW_LINES).collect();
    if total > PREVIEW_LINES {
        out.push(format!("… +{} more lines", total - PREVIEW_LINES));
    }
    out.join("\n")
}

fn rules_diff(tool: &ToolDef, src: &Path, content: &str) -> String {
    let target = agents_md();
    let existing = std::fs::read_to_string(&target).unwrap_or_default();
    let at = existing.lines().count();
    let mut lines = vec![
        format!("--- {}", paths::tilde(&target)),
        format!("+++ {}", paths::tilde(&target)),
        format!(
            "@@ -{at},0 +{},{} @@",
            at + 1,
            section(tool, src, content).lines().count() + 1
        ),
        "+".to_string(),
    ];
    lines.extend(section(tool, src, content).lines().map(|l| format!("+{l}")));
    clip(lines)
}

fn existing_mcp() -> (Value, bool) {
    match std::fs::read_to_string(mcp_json())
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
    {
        Some(v) if v.is_object() => (v, true),
        _ => (json!({"mcpServers": {}}), mcp_json().exists()),
    }
}

fn gather(tools: &[&'static ToolDef], with_env: bool) -> (Vec<Item>, Vec<String>) {
    let prefix = tools.len() > 1;
    let label = |t: &ToolDef, s: String| {
        if prefix {
            format!("{} · {s}", t.name)
        } else {
            s
        }
    };
    let home = paths::home();
    let agents = std::fs::read_to_string(agents_md()).unwrap_or_default();
    let (mcp, _) = existing_mcp();
    let mut items = Vec::new();
    let mut notes = Vec::new();
    let mut claimed: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut claimed_mcp: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for &t in tools {
        for f in sources::rules_files(t.id) {
            let src = home.join(f);
            let Ok(content) = std::fs::read_to_string(&src) else {
                continue;
            };
            if content.trim().is_empty() {
                continue;
            }
            let file = src
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            if agents.contains(&begin(&marker(t.id, &src))) {
                notes.push(format!("{}: {file} already imported", t.name));
                continue;
            }
            items.push(Item {
                tool: Some(t),
                label: label(
                    t,
                    format!("Rules: {file} ({} lines)", content.lines().count()),
                ),
                preview: rules_diff(t, &src, &content),
                kind: Kind::Rules { src, content },
            });
        }
        let all = sources::skills(t.id);
        let total = all.len();
        let links: Vec<(String, PathBuf)> = all
            .into_iter()
            .filter(|(n, _)| std::fs::symlink_metadata(skills_dir().join(n)).is_err())
            .filter(|(n, _)| claimed.insert(n.clone()))
            .collect();
        if total > links.len() {
            notes.push(format!(
                "{}: {} skills already in {} or offered above",
                t.name,
                total - links.len(),
                paths::tilde(&skills_dir())
            ));
        }
        if !links.is_empty() {
            let mut lines = vec![format!("symlinks in {}:", paths::tilde(&skills_dir()))];
            lines.extend(
                links
                    .iter()
                    .map(|(n, p)| format!("  {n} → {}", paths::tilde(p))),
            );
            items.push(Item {
                tool: Some(t),
                label: label(t, format!("Skills: {} to link", links.len())),
                preview: clip(lines),
                kind: Kind::Skills { links },
            });
        }
        let servers: Vec<(String, Value)> = sources::mcp_servers(t.id)
            .into_iter()
            .filter(|(n, _)| mcp["mcpServers"].get(n).is_none())
            .filter(|(n, _)| claimed_mcp.insert(n.clone()))
            .collect();
        if !servers.is_empty() {
            let src = sources::mcp_source(t.id)
                .map(|p| paths::tilde(&p))
                .unwrap_or_default();
            let mut lines = vec![format!(
                "from {src} into {} (values shown as {MASK}):",
                paths::tilde(&mcp_json())
            )];
            lines.extend(
                servers
                    .iter()
                    .map(|(n, e)| format!("  {n}: {}", sources::describe(e))),
            );
            let names: Vec<&str> = servers.iter().map(|(n, _)| n.as_str()).take(3).collect();
            let more = servers.len().saturating_sub(3);
            let names = if more > 0 {
                format!("{} +{more}", names.join(", "))
            } else {
                names.join(", ")
            };
            items.push(Item {
                tool: Some(t),
                label: label(t, format!("MCP: {names}")),
                preview: clip(lines),
                kind: Kind::Mcp { servers },
            });
        }
        if let Some(sub) = sources::sub_plugin(t.id) {
            if sources::plugin_installed(sub.lock_names) {
                notes.push(format!("{}: subscription plugin already installed", t.name));
            } else {
                items.push(Item {
                    tool: Some(t),
                    label: label(t, format!("Plugin: {} subscription", sub.spec)),
                    preview: format!(
                        "gray plugin install {} — use your {} subscription as a provider (trust prompt follows)",
                        sub.spec, t.name
                    ),
                    kind: Kind::Plugin {
                        spec: sub.spec.to_string(),
                    },
                });
            }
        }
    }
    let keys = sources::env_keys();
    if with_env && !keys.is_empty() {
        items.push(Item {
            tool: None,
            label: format!(
                "Providers: {} env key{} → /connect",
                keys.len(),
                if keys.len() == 1 { "" } else { "s" }
            ),
            preview: format!(
                "found in the environment (names only): {}\nnothing is written; run /connect to add them",
                keys.join(", ")
            ),
            kind: Kind::Env { keys },
        });
    }
    (items, notes)
}

fn append_rules(tool: &ToolDef, src: &Path, content: &str) -> std::io::Result<Action> {
    let path = agents_md();
    let existing = std::fs::read_to_string(&path).ok();
    let created_file = existing.is_none();
    let mut text = existing.unwrap_or_default();
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    if !text.is_empty() {
        text.push('\n');
    }
    text.push_str(&section(tool, src, content));
    paths::write_atomic(&path, &text)?;
    Ok(Action::Rules {
        path,
        marker: marker(tool.id, src),
        created_file,
    })
}

fn merge_mcp(servers: &[(String, Value)]) -> std::io::Result<Action> {
    let path = mcp_json();
    let (mut v, existed) = existing_mcp();
    if !v.get("mcpServers").is_some_and(Value::is_object) {
        v["mcpServers"] = json!({});
    }
    let mut names = Vec::new();
    if let Some(map) = v["mcpServers"].as_object_mut() {
        for (n, e) in servers {
            if !map.contains_key(n) {
                map.insert(n.clone(), e.clone());
                names.push(n.clone());
            }
        }
    }
    let text = serde_json::to_string_pretty(&v).map_err(std::io::Error::other)?;
    paths::write_atomic(&path, &(text + "\n"))?;
    Ok(Action::Mcp {
        path,
        names,
        created_file: !existed,
    })
}

fn apply(host: &Host, items: Vec<Item>) -> (Vec<Action>, Vec<String>) {
    let mut actions = Vec::new();
    let mut lines = Vec::new();
    for item in items {
        let who = item.tool.map(|t| t.name).unwrap_or("env");
        match item.kind {
            Kind::Rules { src, content } => {
                let t = item.tool.expect("rules have a tool");
                match append_rules(t, &src, &content) {
                    Ok(a) => {
                        actions.push(a);
                        lines.push(format!(
                            "✓ rules: {} → {} (marked section)",
                            paths::tilde(&src),
                            paths::tilde(&agents_md())
                        ));
                    }
                    Err(e) => lines.push(format!("✗ rules from {}: {e}", paths::tilde(&src))),
                }
            }
            Kind::Skills { links } => {
                let dir = skills_dir();
                let _ = std::fs::create_dir_all(&dir);
                let mut ok = 0;
                for (name, target) in links {
                    let path = dir.join(&name);
                    match std::os::unix::fs::symlink(&target, &path) {
                        Ok(()) => {
                            ok += 1;
                            actions.push(Action::Symlink { path, target });
                        }
                        Err(e) => lines.push(format!("✗ skill {name}: {e}")),
                    }
                }
                lines.push(format!(
                    "✓ skills: {ok} {who} skills linked into {}",
                    paths::tilde(&dir)
                ));
            }
            Kind::Mcp { servers } => match merge_mcp(&servers) {
                Ok(a) => {
                    if let Action::Mcp { names, .. } = &a {
                        lines.push(format!(
                            "✓ mcp: {} added to {}",
                            names.join(", "),
                            paths::tilde(&mcp_json())
                        ));
                    }
                    actions.push(a);
                }
                Err(e) => lines.push(format!("✗ mcp from {who}: {e}")),
            },
            Kind::Plugin { spec } => {
                match host.call(
                    "host/plugin_install",
                    json!({"spec": spec}),
                    host::INSTALL_TTL,
                ) {
                    Ok(r) if r.get("ok").and_then(Value::as_bool) == Some(true) => {
                        actions.push(Action::Plugin { spec: spec.clone() });
                        lines.push(format!("✓ plugin: {spec} installed"));
                    }
                    Ok(r) => lines.push(format!(
                        "✗ plugin {spec}: {}",
                        r.get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("not installed")
                    )),
                    Err(e) => lines.push(format!("✗ plugin {spec}: {e}")),
                }
            }
            Kind::Env { keys } => lines.push(format!(
                "→ providers: {} found in env — run /connect to add them",
                keys.join(", ")
            )),
        }
    }
    (actions, lines)
}

fn ask_items(
    host: &Host,
    header: &str,
    question: &str,
    items: &[Item],
) -> Result<Vec<String>, String> {
    let options: Vec<Value> = items
        .iter()
        .map(|i| json!({"label": i.label, "description": i.preview}))
        .collect();
    let result = host.ask(json!([{
        "id": "items",
        "header": header,
        "question": question,
        "options": options,
        "multi": true,
        "default_all": true,
    }]))?;
    Ok(host::answer_labels(&result, "items").unwrap_or_default())
}

fn dry_list(items: &[Item]) -> String {
    items
        .iter()
        .map(|i| format!("• {}\n{}", i.label, indent(&i.preview)))
        .collect::<Vec<_>>()
        .join("\n")
}

fn indent(s: &str) -> String {
    s.lines()
        .map(|l| format!("    {l}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn run_setup(host: &Host, tools: Vec<&'static ToolDef>, intro: Option<&str>) -> String {
    let (items, notes) = gather(&tools, true);
    let names: Vec<&str> = tools.iter().map(|t| t.name).collect();
    let mut out = Vec::new();
    if items.iter().all(|i| matches!(i.kind, Kind::Env { .. })) {
        out.push(if tools.is_empty() {
            "no other agent tools detected (/qol scanned shows where it looked)".to_string()
        } else {
            format!("nothing new to import from {}", names.join(", "))
        });
        out.extend(notes);
        if let Some(Kind::Env { keys }) = items.first().map(|i| &i.kind) {
            out.push(format!(
                "→ providers: {} found in env — run /connect to add them",
                keys.join(", ")
            ));
        }
        return out.join("\n");
    }
    let mut question = String::new();
    if let Some(i) = intro {
        question.push_str(i);
        question.push_str("\n\n");
    }
    question.push_str(&format!(
        "Found {}. Import into gray? Nothing is written outside {}.",
        names.join(", "),
        paths::tilde(&paths::gray_home())
    ));
    let picked = match ask_items(host, "Import", &question, &items) {
        Ok(p) => p,
        Err(e) => {
            return format!(
                "/qol setup needs an interactive session ({e}). It would offer:\n{}",
                dry_list(&items)
            );
        }
    };
    if picked.is_empty() {
        let mut state = State::load();
        for t in &tools {
            state.dismissed.insert(t.id.to_string());
            state.notified.insert(detect::notice_id(t.id));
        }
        let _ = state.save();
        return format!(
            "skipped — {} won't be suggested again (/qol setup <tool> still works)",
            names.join(", ")
        );
    }
    let chosen: Vec<Item> = items
        .into_iter()
        .filter(|i| picked.contains(&i.label))
        .collect();
    let used: Vec<String> = tools
        .iter()
        .filter(|t| {
            chosen
                .iter()
                .any(|i| i.tool.is_some_and(|it| it.id == t.id))
        })
        .map(|t| t.id.to_string())
        .collect();
    let (actions, lines) = apply(host, chosen);
    let mut state = State::load();
    for t in &tools {
        state.notified.insert(detect::notice_id(t.id));
    }
    out.extend(lines);
    if !actions.is_empty() {
        let id = state.next_import_id();
        state.imports.push(Import {
            id,
            ts: paths::now(),
            tools: used,
            actions,
        });
        out.push(format!(
            "import #{id} logged in {} · /qol undo reverts it",
            paths::tilde(&crate::state::path())
        ));
    }
    if let Err(e) = state.save() {
        out.push(format!("✗ state: {e}"));
    }
    out.join("\n")
}

pub fn command(host: &Host, tool: Option<&str>) -> Value {
    let tools: Vec<&'static ToolDef> = match tool {
        Some(q) => match detect::tool(q) {
            Some(t) => vec![t],
            None => {
                let ids: Vec<&str> = detect::TOOLS.iter().map(|t| t.id).collect();
                return json!({"text": format!("unknown tool {q:?} — one of: {}", ids.join(", "))});
            }
        },
        None => detect::scan().found.iter().map(|f| f.tool).collect(),
    };
    json!({"text": run_setup(host, tools, None)})
}

pub fn first_run(host: &Host) -> Value {
    let tools: Vec<&'static ToolDef> = detect::scan()
        .found
        .iter()
        .map(|f| f.tool)
        .filter(|t| !State::load().dismissed.contains(t.id))
        .collect();
    let (items, _) = gather(&tools, false);
    let text = if items.is_empty() {
        host.say(crate::WELCOME);
        crate::WELCOME.to_string()
    } else {
        format!(
            "{}\n\n{}",
            crate::WELCOME,
            run_setup(host, tools, Some(crate::WELCOME))
        )
    };
    json!({"connect_picker": true, "text": text})
}

pub fn dismiss(q: &str) -> Value {
    let Some(t) = detect::tool(q) else {
        return json!({"text": format!("unknown tool {q:?}")});
    };
    let mut state = State::load();
    state.dismissed.insert(t.id.to_string());
    state.notified.insert(detect::notice_id(t.id));
    match state.save() {
        Ok(()) => {
            json!({"text": format!("{} dismissed — no more notices (/qol setup {} still works)", t.name, t.id)})
        }
        Err(e) => json!({"text": format!("qol: {e}")}),
    }
}

fn remove_section(path: &Path, marker: &str, created_file: bool) -> Result<String, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let (b, e) = (begin(marker), end(marker));
    let Some(start) = text.find(&b) else {
        return Ok(format!(
            "rules section already gone from {}",
            paths::tilde(path)
        ));
    };
    let Some(stop) = text[start..].find(&e).map(|i| start + i + e.len()) else {
        return Err(format!("unterminated section in {}", paths::tilde(path)));
    };
    let mut head = text[..start].to_string();
    if head.ends_with("\n\n") {
        head.pop();
    }
    let tail = text[stop..].strip_prefix('\n').unwrap_or(&text[stop..]);
    let next = format!("{head}{tail}");
    if created_file && next.trim().is_empty() {
        std::fs::remove_file(path).map_err(|e| e.to_string())?;
        return Ok(format!("removed {}", paths::tilde(path)));
    }
    paths::write_atomic(path, &next).map_err(|e| e.to_string())?;
    Ok(format!("rules section removed from {}", paths::tilde(path)))
}

fn remove_mcp(path: &Path, names: &[String], created_file: bool) -> Result<String, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut v: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    if let Some(map) = v.get_mut("mcpServers").and_then(Value::as_object_mut) {
        for n in names {
            map.remove(n);
        }
    }
    let empty = v["mcpServers"].as_object().is_none_or(Map::is_empty)
        && v.as_object().is_some_and(|o| o.len() == 1);
    if created_file && empty {
        std::fs::remove_file(path).map_err(|e| e.to_string())?;
        return Ok(format!("removed {}", paths::tilde(path)));
    }
    let text = serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?;
    paths::write_atomic(path, &(text + "\n")).map_err(|e| e.to_string())?;
    Ok(format!(
        "mcp: {} removed from {}",
        names.join(", "),
        paths::tilde(path)
    ))
}

fn revert(a: &Action) -> Result<String, String> {
    match a {
        Action::Rules {
            path,
            marker,
            created_file,
        } => remove_section(path, marker, *created_file),
        Action::Symlink { path, target } => match std::fs::read_link(path) {
            Ok(t) if t == *target => std::fs::remove_file(path)
                .map(|()| format!("unlinked {}", paths::tilde(path)))
                .map_err(|e| e.to_string()),
            Ok(_) => Err(format!(
                "{} now points elsewhere, left alone",
                paths::tilde(path)
            )),
            Err(_) => Ok(format!("{} already gone", paths::tilde(path))),
        },
        Action::Mcp {
            path,
            names,
            created_file,
        } => remove_mcp(path, names, *created_file),
        Action::Plugin { spec } => Ok(format!(
            "plugin {spec} left installed — `gray plugin remove {spec}` to drop it"
        )),
    }
}

pub fn undo(n: usize) -> Value {
    let mut state = State::load();
    if state.imports.is_empty() {
        return json!({"text": "nothing to undo"});
    }
    let mut out = Vec::new();
    for _ in 0..n {
        let Some(imp) = state.imports.pop() else {
            break;
        };
        out.push(format!(
            "undo import #{} ({})",
            imp.id,
            imp.tools.join(", ")
        ));
        let mut links = 0;
        for a in imp.actions.iter().rev() {
            match revert(a) {
                Ok(_) if matches!(a, Action::Symlink { .. }) => links += 1,
                Ok(msg) => out.push(format!("  ✓ {msg}")),
                Err(msg) => out.push(format!("  ✗ {msg}")),
            }
        }
        if links > 0 {
            out.push(format!("  ✓ {links} skill symlinks removed"));
        }
    }
    if let Err(e) = state.save() {
        out.push(format!("✗ state: {e}"));
    }
    json!({"text": out.join("\n")})
}
