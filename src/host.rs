use std::collections::HashMap;
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, mpsc};
use std::time::Duration;

use serde_json::{Value, json};

pub const ASK_TTL: Duration = Duration::from_secs(300);
pub const HOST_TTL: Duration = Duration::from_secs(25);
pub const INSTALL_TTL: Duration = Duration::from_secs(600);

pub struct Host {
    out: Mutex<std::io::Stdout>,
    pending: Mutex<HashMap<String, mpsc::Sender<Value>>>,
    counter: AtomicU64,
}

impl Host {
    pub fn new() -> Self {
        Self {
            out: Mutex::new(std::io::stdout()),
            pending: Mutex::new(HashMap::new()),
            counter: AtomicU64::new(0),
        }
    }

    pub fn write(&self, v: &Value) {
        let mut o = self.out.lock().unwrap_or_else(|e| e.into_inner());
        let _ = writeln!(o, "{v}");
        let _ = o.flush();
    }

    pub fn deliver(&self, id: &str, msg: Value) {
        let tx = self
            .pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(id);
        if let Some(tx) = tx {
            let _ = tx.send(msg);
        }
    }

    pub fn call(&self, method: &str, params: Value, ttl: Duration) -> Result<Value, String> {
        let n = self.counter.fetch_add(1, Ordering::Relaxed) + 1;
        let id = format!("qol{n}");
        let (tx, rx) = mpsc::channel();
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id.clone(), tx);
        self.write(&json!({"id": id, "method": method, "params": params}));
        let msg = rx.recv_timeout(ttl).map_err(|_| {
            self.pending
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&id);
            format!("{method}: host did not answer")
        })?;
        if let Some(err) = msg.get("error").filter(|e| !e.is_null()) {
            return Err(err
                .get("message")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| err.to_string()));
        }
        let result = msg.get("result").cloned().unwrap_or(Value::Null);
        if let Some(err) = result.get("error").and_then(Value::as_str) {
            return Err(err.to_string());
        }
        Ok(result)
    }

    pub fn say(&self, text: &str) {
        let _ = self.call("host/say", json!({"text": text}), HOST_TTL);
    }

    pub fn notice(&self, text: &str, action: &str, slot: &str, id: &str) -> bool {
        self.call(
            "host/notice",
            json!({"text": text, "action": action, "slot": slot, "id": id}),
            HOST_TTL,
        )
        .ok()
        .and_then(|v| v.get("shown").and_then(Value::as_bool))
        .unwrap_or(false)
    }

    pub fn ask(&self, questions: Value) -> Result<Value, String> {
        self.call(
            "host/ask",
            json!({"questions": questions, "blocking": true}),
            ASK_TTL,
        )
    }
}

pub fn answer_labels(result: &Value, id: &str) -> Option<Vec<String>> {
    let entry = result.get("answers")?.get(id)?;
    let list = |v: &Value| -> Option<Vec<String>> {
        v.as_array().map(|a| {
            a.iter()
                .filter_map(|e| e.as_str().map(str::to_string))
                .collect()
        })
    };
    entry
        .get("selected")
        .and_then(list)
        .or_else(|| entry.get("answers").and_then(list))
        .or_else(|| list(entry))
}
