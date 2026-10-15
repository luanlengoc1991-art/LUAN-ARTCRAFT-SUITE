//! One app's own MCP server (`<id>-cli mcp ...`) running as a child process, spoken to over stdio.

use serde_json::{Value, json};
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const PROTOCOL: &str = "2025-06-18";

pub struct Child {
    proc: std::process::Child,
    stdin: ChildStdin,
    rx: Receiver<Value>,
    stderr: Arc<Mutex<VecDeque<String>>>,
    next_id: u64,
    /// Driving the app's open window (true) or a headless engine (false).
    pub live: bool,
    pub tools: Vec<Value>,
    pub instructions: Option<String>,
    /// The server said its tool list changed.
    pub tools_changed: bool,
}

/// A JSON-RPC error the app returned, as opposed to the app failing to answer at all.
pub enum Failure {
    Rpc(Value),
    Transport(String),
}

impl Child {
    pub fn start(cli: &Path, args: &[String], live: bool, log: &Path) -> Result<Child, String> {
        let mut cmd = Command::new(cli);
        cmd.args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        if let Some(dir) = cli.parent() {
            cmd.current_dir(dir);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let mut proc = cmd.spawn().map_err(|e| format!("can't start {}: {e}", cli.display()))?;
        let stdin = proc.stdin.take().expect("piped");
        let stdout = proc.stdout.take().expect("piped");
        let stderr_pipe = proc.stderr.take().expect("piped");

        let (tx, rx) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if let Ok(v) = serde_json::from_str::<Value>(line.trim()) {
                    if tx.send(v).is_err() {
                        break;
                    }
                }
            }
        });
        let stderr = Arc::new(Mutex::new(VecDeque::new()));
        let tail = stderr.clone();
        let log = log.to_path_buf();
        std::thread::spawn(move || {
            let mut file = std::fs::OpenOptions::new().create(true).append(true).open(&log).ok();
            for line in BufReader::new(stderr_pipe).lines() {
                let Ok(line) = line else { break };
                if let Some(f) = file.as_mut() {
                    let _ = writeln!(f, "{line}");
                }
                let mut t = tail.lock().unwrap();
                if t.len() == 12 {
                    t.pop_front();
                }
                t.push_back(line);
            }
        });

        let mut child = Child { proc, stdin, rx, stderr, next_id: 1, live, tools: Vec::new(), instructions: None, tools_changed: false };
        let init = json!({
            "protocolVersion": PROTOCOL,
            "capabilities": {},
            "clientInfo": { "name": "artcraft-mcp", "version": env!("CARGO_PKG_VERSION") },
        });
        let result = child.request("initialize", init, Duration::from_secs(60)).map_err(|f| child.describe(f))?;
        child.instructions = result["instructions"].as_str().filter(|s| !s.is_empty()).map(str::to_owned);
        child.notify("notifications/initialized", json!({})).map_err(|e| child.describe(Failure::Transport(e)))?;
        child.list_tools().map_err(|f| child.describe(f))?;
        Ok(child)
    }

    pub fn list_tools(&mut self) -> Result<(), Failure> {
        let mut tools = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let params = match &cursor {
                Some(c) => json!({ "cursor": c }),
                None => json!({}),
            };
            let page = self.request("tools/list", params, Duration::from_secs(60))?;
            tools.extend(page["tools"].as_array().cloned().unwrap_or_default());
            match page["nextCursor"].as_str() {
                Some(c) if !c.is_empty() => cursor = Some(c.to_owned()),
                _ => break,
            }
        }
        self.tools = tools;
        self.tools_changed = false;
        Ok(())
    }

    pub fn alive(&mut self) -> bool {
        matches!(self.proc.try_wait(), Ok(None))
    }

    fn send(&mut self, msg: &Value) -> Result<(), String> {
        let mut line = msg.to_string();
        line.push('\n');
        self.stdin.write_all(line.as_bytes()).and_then(|()| self.stdin.flush()).map_err(|e| format!("the app's MCP server stopped: {e}"))
    }

    pub fn notify(&mut self, method: &str, params: Value) -> Result<(), String> {
        self.send(&json!({ "jsonrpc": "2.0", "method": method, "params": params }))
    }

    /// Sends a request and waits for its answer. Requests the server sends meanwhile get a
    /// "not supported" answer; notifications are noted.
    pub fn request(&mut self, method: &str, params: Value, timeout: Duration) -> Result<Value, Failure> {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })).map_err(Failure::Transport)?;
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let msg = match self.rx.recv_timeout(left) {
                Ok(m) => m,
                Err(RecvTimeoutError::Timeout) => {
                    return Err(Failure::Transport(format!("no answer to {method} within {} s", timeout.as_secs())));
                }
                Err(RecvTimeoutError::Disconnected) => return Err(Failure::Transport("the app's MCP server exited".into())),
            };
            if let Some(m) = msg["method"].as_str() {
                if m == "notifications/tools/list_changed" {
                    self.tools_changed = true;
                }
                if !msg["id"].is_null() {
                    let reply = json!({ "jsonrpc": "2.0", "id": msg["id"], "error": { "code": -32601, "message": "not supported by artcraft-mcp" } });
                    let _ = self.send(&reply);
                }
                continue;
            }
            if msg["id"].as_u64() != Some(id) {
                continue;
            }
            if !msg["error"].is_null() {
                return Err(Failure::Rpc(msg["error"].clone()));
            }
            return Ok(msg["result"].clone());
        }
    }

    /// A failure as text, with the last lines the app wrote to stderr.
    pub fn describe(&self, f: Failure) -> String {
        let mut s = match f {
            Failure::Rpc(e) => e["message"].as_str().map_or_else(|| e.to_string(), str::to_owned),
            Failure::Transport(s) => s,
        };
        let tail = self.stderr.lock().unwrap();
        if !tail.is_empty() {
            s.push_str("\n--- app stderr ---\n");
            s.push_str(&tail.iter().cloned().collect::<Vec<_>>().join("\n"));
        }
        s
    }
}

impl Drop for Child {
    fn drop(&mut self) {
        let _ = self.proc.kill();
        let _ = self.proc.wait();
    }
}
