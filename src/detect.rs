use std::time::Instant;

use serde_json::{Value, json};

use crate::host::Host;
use crate::paths;
use crate::state::{Known, State};

pub struct ToolDef {
    pub id: &'static str,
    pub name: &'static str,
    pub dirs: &'static [&'static str],
    pub files: &'static [&'static str],
    pub bins: &'static [&'static str],
}

pub const TOOLS: &[ToolDef] = &[
    ToolDef {
        id: "claude",
        name: "Claude Code",
        dirs: &[".claude"],
        files: &[".claude.json"],
        bins: &["claude"],
    },
    ToolDef {
        id: "codex",
        name: "Codex",
        dirs: &[".codex"],
        files: &[],
        bins: &["codex"],
    },
    ToolDef {
        id: "opencode",
        name: "opencode",
        dirs: &[".config/opencode"],
        files: &[],
        bins: &["opencode"],
    },
    ToolDef {
        id: "cursor",
        name: "Cursor",
        dirs: &[".cursor"],
        files: &[],
        bins: &["cursor-agent"],
    },
    ToolDef {
        id: "windsurf",
        name: "Windsurf",
        dirs: &[".windsurf"],
        files: &[],
        bins: &[],
    },
    ToolDef {
        id: "devin",
        name: "Devin",
        dirs: &[".config/devin", ".local/share/devin"],
        files: &[],
        bins: &["devin"],
    },
    ToolDef {
        id: "aider",
        name: "Aider",
        dirs: &[],
        files: &[],
        bins: &["aider"],
    },
];

pub fn tool(query: &str) -> Option<&'static ToolDef> {
    let q = query.trim().to_ascii_lowercase();
    let q = q.strip_suffix("-code").unwrap_or(&q);
    TOOLS.iter().find(|t| {
        t.id == q
            || t.name.to_ascii_lowercase() == q
            || t.name.to_ascii_lowercase().replace(' ', "-") == q
            || t.bins.contains(&q)
    })
}

pub struct Looked {
    pub what: String,
    pub hit: bool,
}

pub struct Found {
    pub tool: &'static ToolDef,
    pub hits: Vec<String>,
}

pub struct Scan {
    pub looked: Vec<(&'static ToolDef, Vec<Looked>)>,
    pub found: Vec<Found>,
    pub elapsed_ms: f64,
}

pub fn scan() -> Scan {
    let start = Instant::now();
    let home = paths::home();
    let mut looked = Vec::new();
    let mut found = Vec::new();
    for t in TOOLS {
        let mut rows = Vec::new();
        let mut hits = Vec::new();
        for d in t.dirs {
            let p = home.join(d);
            let hit = p.is_dir();
            let what = format!("{}/", paths::tilde(&p));
            if hit {
                hits.push(what.clone());
            }
            rows.push(Looked { what, hit });
        }
        for f in t.files {
            let p = home.join(f);
            let hit = p.is_file();
            let what = paths::tilde(&p);
            if hit {
                hits.push(what.clone());
            }
            rows.push(Looked { what, hit });
        }
        for b in t.bins {
            let at = paths::which(b);
            let what = match &at {
                Some(p) => format!("{b} on PATH ({})", paths::tilde(p)),
                None => format!("{b} on PATH"),
            };
            if at.is_some() {
                hits.push(format!("bin:{b}"));
            }
            rows.push(Looked {
                what,
                hit: at.is_some(),
            });
        }
        if !hits.is_empty() {
            found.push(Found { tool: t, hits });
        }
        looked.push((t, rows));
    }
    Scan {
        looked,
        found,
        elapsed_ms: start.elapsed().as_secs_f64() * 1000.0,
    }
}

pub fn notice_id(tool: &str) -> String {
    format!("qol.detect.{tool}")
}

pub fn record(state: &mut State, scan: &Scan) {
    let now = paths::now();
    for f in &scan.found {
        let k = state.known.entry(f.tool.id.to_string()).or_insert(Known {
            fingerprint: Vec::new(),
            first_seen: now,
            last_seen: now,
        });
        k.fingerprint = f.hits.clone();
        k.last_seen = now;
    }
    state.last_scan_ms = Some((scan.elapsed_ms * 100.0).round() / 100.0);
}

pub fn imported(state: &State, tool: &str) -> bool {
    state
        .imports
        .iter()
        .any(|i| i.tools.iter().any(|t| t == tool))
}

pub fn notify(host: &Host) {
    let scan = scan();
    let mut state = State::load();
    record(&mut state, &scan);
    let fresh: Vec<&ToolDef> = scan
        .found
        .iter()
        .map(|f| f.tool)
        .filter(|t| {
            !state.notified.contains(&notice_id(t.id))
                && !state.dismissed.contains(t.id)
                && !imported(&state, t.id)
        })
        .collect();
    if !fresh.is_empty() {
        let names: Vec<&str> = fresh.iter().map(|t| t.name).collect();
        let (action, id) = match fresh.as_slice() {
            [one] => (format!("/qol setup {}", one.id), notice_id(one.id)),
            many => (
                "/qol setup".to_string(),
                notice_id(&many.iter().map(|t| t.id).collect::<Vec<_>>().join("+")),
            ),
        };
        let text = format!("{} detected", names.join(", "));
        if host.notice(&text, &action, "home", &id) {
            for t in &fresh {
                state.notified.insert(notice_id(t.id));
            }
        }
    }
    let _ = state.save();
}

pub fn scanned_report() -> Value {
    let scan = scan();
    let mut state = State::load();
    record(&mut state, &scan);
    let _ = state.save();
    let mut out = format!(
        "qol detector · {:.2} ms · stat-only, no file contents read\n",
        scan.elapsed_ms
    );
    for (t, rows) in &scan.looked {
        let found = scan.found.iter().any(|f| f.tool.id == t.id);
        let tag = if state.dismissed.contains(t.id) {
            " (dismissed)"
        } else if imported(&state, t.id) {
            " (imported)"
        } else {
            ""
        };
        out.push_str(&format!(
            "\n{} {}{}\n",
            if found { "✓" } else { "·" },
            t.name,
            tag
        ));
        for r in rows {
            out.push_str(&format!(
                "    {} {}\n",
                if r.hit { "found  " } else { "absent " },
                r.what
            ));
        }
    }
    let found: Vec<&str> = scan.found.iter().map(|f| f.tool.name).collect();
    out.push_str(&format!(
        "\nfound: {}",
        if found.is_empty() {
            "nothing".to_string()
        } else {
            found.join(", ")
        }
    ));
    if !state.imports.is_empty() {
        out.push_str(&format!(
            "\nimports logged: {} (/qol undo reverts the last)",
            state.imports.len()
        ));
    }
    json!({"text": out})
}
