use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::host::{self, Host};
use crate::paths;

const CHECK_TTL: Duration = Duration::from_secs(20);
const UPDATE_TTL: Duration = Duration::from_secs(600);
const PROVIDER_CLIS: &[&str] = &[
    "claude",
    "codex",
    "opencode",
    "devin",
    "cursor-agent",
    "aider",
];
const RESUME_LABEL: &str = "Continue sessions after restart";

pub struct Out {
    pub ok: bool,
    pub text: String,
}

pub fn run_cmd(argv: &[String], ttl: Duration) -> Out {
    let Some((bin, args)) = argv.split_first() else {
        return Out {
            ok: false,
            text: "empty command".into(),
        };
    };
    let child = Command::new(bin)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("NO_COLOR", "1")
        .env("CI", "1")
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => {
            return Out {
                ok: false,
                text: format!("{bin}: {e}"),
            };
        }
    };
    let pipe = |r: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut s = String::new();
            if let Some(mut r) = r {
                let _ = r.read_to_string(&mut s);
            }
            s
        })
    };
    let so = pipe(
        child
            .stdout
            .take()
            .map(|r| Box::new(r) as Box<dyn Read + Send>),
    );
    let se = pipe(
        child
            .stderr
            .take()
            .map(|r| Box::new(r) as Box<dyn Read + Send>),
    );
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) if start.elapsed() > ttl => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(40)),
            Err(_) => break None,
        }
    };
    let mut text = so.join().unwrap_or_default();
    let err = se.join().unwrap_or_default();
    if !err.trim().is_empty() {
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(&err);
    }
    match status {
        Some(s) => Out {
            ok: s.success(),
            text,
        },
        None => Out {
            ok: false,
            text: format!("{text}\n(timed out after {}s)", ttl.as_secs()),
        },
    }
}

fn sv(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| s.to_string()).collect()
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\x1b' {
            if it.peek() == Some(&'[') {
                it.next();
                for d in it.by_ref() {
                    if d.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

fn first_line(s: &str) -> String {
    strip_ansi(s)
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("")
        .to_string()
}

#[derive(Clone)]
enum Method {
    Npm(String),
    Brew(String),
    Pipx(String),
    Uv(String),
    SelfUpdate(Vec<String>),
}

#[derive(Clone)]
struct Cli {
    name: &'static str,
    path: PathBuf,
    resolved: PathBuf,
    method: Option<Method>,
    current: String,
    latest: Option<String>,
    has_update: Option<bool>,
    running: bool,
    blocked: Option<String>,
}

fn npm_package(resolved: &Path) -> Option<(String, PathBuf)> {
    let s = resolved.to_string_lossy();
    let idx = s.rfind("/node_modules/")?;
    let rest = &s[idx + "/node_modules/".len()..];
    let mut parts = rest.split('/');
    let first = parts.next()?;
    let pkg = if first.starts_with('@') {
        format!("{first}/{}", parts.next()?)
    } else {
        first.to_string()
    };
    let root = PathBuf::from(format!("{}/node_modules/{pkg}", &s[..idx]));
    Some((pkg, root))
}

fn self_updater(name: &str) -> Option<Vec<String>> {
    Some(match name {
        "claude" => sv(&["claude", "update"]),
        "devin" => sv(&["devin", "update"]),
        "opencode" => sv(&["opencode", "upgrade"]),
        "cursor-agent" => sv(&["cursor-agent", "update"]),
        "aider" => sv(&["aider", "--upgrade"]),
        _ => return None,
    })
}

fn method_for(name: &str, resolved: &Path) -> Option<Method> {
    let s = resolved.to_string_lossy();
    if let Some((pkg, _)) = npm_package(resolved) {
        return Some(Method::Npm(pkg));
    }
    if let Some(i) = s.find("/Cellar/") {
        let formula = s[i + 8..].split('/').next().unwrap_or(name).to_string();
        return Some(Method::Brew(formula));
    }
    if let Some(i) = s.find("/pipx/venvs/") {
        return Some(Method::Pipx(
            s[i + 12..].split('/').next().unwrap_or(name).to_string(),
        ));
    }
    if let Some(i) = s.find("/uv/tools/") {
        return Some(Method::Uv(
            s[i + 10..].split('/').next().unwrap_or(name).to_string(),
        ));
    }
    self_updater(name).map(Method::SelfUpdate)
}

fn proc_running(name: &str, resolved: &Path) -> bool {
    let me = std::process::id().to_string();
    let Ok(rd) = std::fs::read_dir("/proc") else {
        return false;
    };
    for e in rd.flatten() {
        let pid = e.file_name().to_string_lossy().to_string();
        if !pid.chars().all(|c| c.is_ascii_digit()) || pid == me {
            continue;
        }
        if std::fs::read_link(e.path().join("exe")).is_ok_and(|x| x == resolved) {
            return true;
        }
        let Ok(cmd) = std::fs::read(e.path().join("cmdline")) else {
            continue;
        };
        let args: Vec<String> = cmd
            .split(|b| *b == 0)
            .take(2)
            .map(|a| String::from_utf8_lossy(a).to_string())
            .collect();
        if args.iter().any(|a| {
            Path::new(a).file_name().is_some_and(|f| f == name) || Path::new(a) == resolved
        }) {
            return true;
        }
    }
    false
}

fn version_of(path: &Path) -> String {
    let out = run_cmd(
        &[path.to_string_lossy().to_string(), "--version".into()],
        Duration::from_secs(8),
    );
    if out.ok {
        first_line(&out.text)
    } else {
        String::new()
    }
}

fn npm_version(resolved: &Path) -> Option<String> {
    let (_, root) = npm_package(resolved)?;
    std::fs::read_to_string(root.join("package.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v["version"].as_str().map(str::to_string))
}

#[cfg(unix)]
fn uid() -> Option<u32> {
    std::fs::read_to_string("/proc/self/status")
        .ok()?
        .lines()
        .find_map(|l| l.strip_prefix("Uid:"))?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

#[cfg(unix)]
fn writable(dir: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    let Ok(m) = std::fs::metadata(dir) else {
        return false;
    };
    match uid() {
        Some(0) => true,
        Some(u) if m.uid() == u => m.mode() & 0o200 != 0,
        _ => m.mode() & 0o002 != 0,
    }
}

// Windows has no uid/mode answer to "can I write here" — create a probe.
#[cfg(not(unix))]
fn writable(dir: &Path) -> bool {
    if !dir.is_dir() {
        return false;
    }
    let probe = dir.join(format!(".qol-wtest-{}", std::process::id()));
    match std::fs::File::create(&probe) {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

fn installed_version(c: &Cli) -> String {
    match &c.method {
        Some(Method::Npm(_)) => npm_version(&c.resolved).unwrap_or_else(|| version_of(&c.path)),
        _ => version_of(&c.path),
    }
}

fn check_cli(name: &'static str) -> Option<Cli> {
    let path = paths::which(name)?;
    let resolved = std::fs::canonicalize(&path).unwrap_or(path.clone());
    let method = method_for(name, &resolved);
    let mut blocked = None;
    if matches!(method, Some(Method::Npm(_)))
        && let Some((_, root)) = npm_package(&resolved)
        && let Some(parent) = root.parent()
        && !writable(parent)
    {
        blocked = Some(format!(
            "{} is not writable (needs sudo)",
            paths::tilde(parent)
        ));
    }
    let mut cli = Cli {
        name,
        running: proc_running(name, &resolved),
        path,
        resolved,
        method,
        current: String::new(),
        latest: None,
        has_update: None,
        blocked,
    };
    cli.current = installed_version(&cli);
    let current = cli.current.clone();
    let (latest, has_update) = match &cli.method {
        Some(Method::Npm(pkg)) => {
            let out = run_cmd(&sv(&["npm", "view", pkg, "version"]), CHECK_TTL);
            let latest = first_line(&out.text);
            if out.ok && !latest.is_empty() {
                let newer = latest != current;
                (Some(latest), Some(newer))
            } else {
                (None, None)
            }
        }
        Some(Method::Brew(f)) => {
            let out = run_cmd(&sv(&["brew", "outdated", "--quiet", f]), CHECK_TTL);
            (None, out.ok.then(|| !out.text.trim().is_empty()))
        }
        _ => (None, None),
    };
    cli.latest = latest;
    cli.has_update = has_update;
    Some(cli)
}

fn method_label(m: &Method) -> String {
    match m {
        Method::Npm(p) => format!("npm {p}"),
        Method::Brew(f) => format!("brew {f}"),
        Method::Pipx(p) => format!("pipx {p}"),
        Method::Uv(p) => format!("uv tool {p}"),
        Method::SelfUpdate(c) => c.join(" "),
    }
}

fn update_argv(m: &Method) -> Vec<String> {
    match m {
        Method::Npm(p) => sv(&["npm", "install", "-g", &format!("{p}@latest")]),
        Method::Brew(f) => sv(&["brew", "upgrade", f]),
        Method::Pipx(p) => sv(&["pipx", "upgrade", p]),
        Method::Uv(p) => sv(&["uv", "tool", "upgrade", p]),
        Method::SelfUpdate(c) => c.clone(),
    }
}

fn version_token(line: &str) -> Option<String> {
    line.split(|c: char| c.is_whitespace() || c == '(' || c == ')')
        .map(|w| w.trim_start_matches('v'))
        .find(|w| {
            w.split('.').count() >= 2
                && w.split('.')
                    .next()
                    .is_some_and(|p| p.chars().all(|c| c.is_ascii_digit()) && !p.is_empty())
        })
        .map(str::to_string)
}

struct GrayCheck {
    line: String,
    available: bool,
    ok: bool,
}

fn gray_check() -> GrayCheck {
    let out = run_cmd(&sv(&[&paths::gray_bin(), "update", "--check"]), CHECK_TTL);
    let line = first_line(&out.text);
    let l = line.to_ascii_lowercase();
    let available = out.ok
        && (l.contains("available") || line.contains('→') || l.contains("->"))
        && !l.contains("up to date");
    GrayCheck {
        line,
        available,
        ok: out.ok,
    }
}

fn plugin_check() -> (Vec<String>, Option<String>) {
    let out = run_cmd(
        &sv(&[&paths::gray_bin(), "plugin", "update", "--check", "all"]),
        CHECK_TTL,
    );
    if !out.ok {
        return (Vec::new(), Some(first_line(&out.text)));
    }
    let rows = strip_ansi(&out.text)
        .lines()
        .map(str::trim)
        .filter(|l| {
            !l.is_empty()
                && *l != "up to date"
                && !l.contains(": skipped")
                && !l.ends_with(": up to date")
        })
        .map(str::to_string)
        .collect();
    (rows, None)
}

fn running_sessions(host: &Host, session: &str) -> Option<usize> {
    let r = host.call("host/sessions", json!({}), host::HOST_TTL).ok()?;
    Some(
        r.get("sessions")?
            .as_array()?
            .iter()
            .filter(|s| s.get("running").and_then(Value::as_bool) == Some(true))
            .filter(|s| session.is_empty() || s.get("id").and_then(Value::as_str) != Some(session))
            .count(),
    )
}

fn log_path() -> PathBuf {
    paths::qol_dir()
        .join("logs")
        .join(format!("update-{}.log", paths::now()))
}

fn log(path: &Path, title: &str, out: &Out) {
    if let Some(d) = path.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(
            f,
            "== {title} ({})\n{}\n",
            if out.ok { "ok" } else { "failed" },
            strip_ansi(&out.text).trim_end()
        );
    }
}

pub fn run(host: &Host, session: &str) -> Value {
    let gray = std::thread::spawn(gray_check);
    let plugins = std::thread::spawn(plugin_check);
    let clis: Vec<_> = PROVIDER_CLIS
        .iter()
        .map(|n| std::thread::spawn(move || check_cli(n)))
        .collect();
    let gray = gray.join().unwrap_or(GrayCheck {
        line: String::new(),
        available: false,
        ok: false,
    });
    let (plugin_rows, plugin_err) = plugins.join().unwrap_or_default();
    let clis: Vec<Cli> = clis
        .into_iter()
        .filter_map(|h| h.join().ok().flatten())
        .collect();
    let sessions = running_sessions(host, session);

    let mut lines = Vec::new();
    let mut current = Vec::new();
    if gray.available {
        lines.push(format!("• gray: {}", gray.line));
    } else if gray.ok {
        current.push("gray".to_string());
    } else {
        lines.push(format!(
            "• gray: version check failed ({}), skipped",
            if gray.line.is_empty() {
                "no output"
            } else {
                &gray.line
            }
        ));
    }
    if !plugin_rows.is_empty() {
        lines.push(format!("• plugins: {}", plugin_rows.join("; ")));
    } else if let Some(e) = &plugin_err {
        lines.push(format!("• plugins: check failed ({e}), skipped"));
    } else {
        current.push("plugins".to_string());
    }
    let mut todo: Vec<Cli> = Vec::new();
    for c in &clis {
        let Some(m) = &c.method else {
            lines.push(format!("• {}: no known updater, skipped", c.name));
            continue;
        };
        if c.has_update == Some(false) {
            current.push(c.name.to_string());
            continue;
        }
        let what = match (&c.latest, c.has_update) {
            (Some(l), _) => format!("{} → {l} via {}", c.current, method_label(m)),
            (None, Some(true)) => format!("{} has an update via {}", c.current, method_label(m)),
            _ => format!("{} · checks via {}", c.current, method_label(m)),
        };
        if let Some(why) = &c.blocked {
            lines.push(format!("• {}: {what} — skipped, {why}", c.name));
            continue;
        }
        if c.running {
            lines.push(format!(
                "• {}: {what} — skipped, {} is running",
                c.name, c.name
            ));
            continue;
        }
        lines.push(format!("• {}: {what}", c.name));
        todo.push(c.clone());
    }
    if !current.is_empty() {
        lines.push(format!("• current: {}", current.join(", ")));
    }
    let restart = gray.available || !plugin_rows.is_empty();
    if !restart && todo.is_empty() {
        return json!({"text": format!("everything is up to date\n{}", lines.join("\n"))});
    }
    match sessions {
        Some(n) if restart => lines.push(format!(
            "• {n} running session{} will be interrupted",
            if n == 1 { "" } else { "s" }
        )),
        None if restart => {
            lines.push("• running sessions: unknown (host/sessions unavailable)".into())
        }
        _ => {}
    }
    let mut steps = Vec::new();
    if !todo.is_empty() {
        steps.push("provider CLIs");
    }
    if !plugin_rows.is_empty() {
        steps.push("gray plugin update all");
    }
    if gray.available {
        steps.push("gray update");
    }
    if restart {
        steps.push("restart");
    }
    let question = format!(
        "Update everything?\n{}\n\nOrder: {}",
        lines.join("\n"),
        steps.join(" → ")
    );
    let mut questions = vec![json!({
        "id": "confirm",
        "header": "Update",
        "question": question,
        "options": [
            {"label": "Update", "description": steps.join(" → ")},
            {"label": "Cancel", "description": "change nothing"},
        ],
    })];
    if restart {
        questions.push(json!({
            "id": "resume",
            "header": "Restart",
            "question": "After the restart:",
            "options": [{"label": RESUME_LABEL, "description": "reopen this session where it left off"}],
            "multi": true,
            "default_all": true,
        }));
    }
    let answer = match host.ask(Value::Array(questions)) {
        Ok(a) => a,
        Err(e) => {
            return json!({"text": format!("/qol update needs an interactive session ({e}). Pending:\n{}", lines.join("\n"))});
        }
    };
    let yes =
        host::answer_labels(&answer, "confirm").is_some_and(|l| l.iter().any(|s| s == "Update"));
    if !yes {
        return json!({"text": "update cancelled — nothing changed"});
    }
    let resume = host::answer_labels(&answer, "resume")
        .map(|l| l.iter().any(|s| s == RESUME_LABEL))
        .unwrap_or(true);

    let logp = log_path();
    let mut results = Vec::new();
    for c in &todo {
        let Some(m) = &c.method else { continue };
        let out = run_cmd(&update_argv(m), UPDATE_TTL);
        log(&logp, &format!("{} ({})", c.name, method_label(m)), &out);
        let after = installed_version(c);
        let row = if !out.ok {
            format!("✗ {}: failed — {}", c.name, paths::tilde(&logp))
        } else if !after.is_empty() && after != c.current && !c.current.contains(&after) {
            format!("✓ {}: updated {} → {after}", c.name, c.current)
        } else {
            format!("· {}: current ({})", c.name, c.current)
        };
        results.push(row);
    }
    let mut gray_failed = false;
    if !plugin_rows.is_empty() {
        let out = run_cmd(
            &sv(&[&paths::gray_bin(), "plugin", "update", "all"]),
            UPDATE_TTL,
        );
        log(&logp, "gray plugin update all", &out);
        if out.ok {
            for l in strip_ansi(&out.text).lines().map(str::trim) {
                if l.is_empty() || l.contains(": skipped") || l == "up to date" {
                    continue;
                }
                let mark = if l.contains("failed") { "✗" } else { "✓" };
                results.push(format!("{mark} plugin {l}"));
            }
        } else {
            gray_failed = true;
            results.push(format!("✗ plugins: failed — {}", paths::tilde(&logp)));
        }
    }
    if gray.available {
        let out = run_cmd(&sv(&[&paths::gray_bin(), "update"]), UPDATE_TTL);
        log(&logp, "gray update", &out);
        if out.ok {
            results.push(match version_token(&gray.line) {
                Some(v) => format!("✓ gray: updated to {v}"),
                None => "✓ gray: updated".to_string(),
            });
        } else {
            gray_failed = true;
            results.push(format!("✗ gray: failed — {}", paths::tilde(&logp)));
        }
    }
    let mut text = results.join("\n");
    if restart && !gray_failed {
        text.push_str(&format!(
            "\nrestarting{}…",
            if resume { " and resuming" } else { "" }
        ));
        host.say(&text);
        match host.call("host/restart", json!({"resume": resume}), host::HOST_TTL) {
            Ok(_) => {}
            Err(e) => text.push_str(&format!("\n✗ restart: {e} — run /restart")),
        }
    } else if restart {
        text.push_str("\nnot restarting after a failure; see the log, then /restart");
    }
    json!({"text": text})
}
