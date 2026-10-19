use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::paths;

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Known {
    pub fingerprint: Vec<String>,
    pub first_seen: u64,
    pub last_seen: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    Rules {
        path: PathBuf,
        marker: String,
        created_file: bool,
    },
    Symlink {
        path: PathBuf,
        target: PathBuf,
    },
    Copy {
        path: PathBuf,
        target: PathBuf,
    },
    Mcp {
        path: PathBuf,
        names: Vec<String>,
        created_file: bool,
    },
    Plugin {
        spec: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Import {
    pub id: u64,
    pub ts: u64,
    pub tools: Vec<String>,
    pub actions: Vec<Action>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct State {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub known: BTreeMap<String, Known>,
    #[serde(default)]
    pub notified: BTreeSet<String>,
    #[serde(default)]
    pub dismissed: BTreeSet<String>,
    #[serde(default)]
    pub imports: Vec<Import>,
    #[serde(default)]
    pub last_scan_ms: Option<f64>,
}

pub fn path() -> PathBuf {
    paths::qol_dir().join("state.json")
}

impl State {
    pub fn load() -> Self {
        std::fs::read_to_string(path())
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&mut self) -> std::io::Result<()> {
        self.version = 1;
        let text = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        paths::write_atomic(&path(), &(text + "\n"))
    }

    pub fn next_import_id(&self) -> u64 {
        self.imports.iter().map(|i| i.id).max().unwrap_or(0) + 1
    }
}
