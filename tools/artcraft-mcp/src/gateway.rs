//! The MCP server Claude Code / Cursor talk to. It lists the tools of every installed app under
//! `<app>_<tool>` and forwards each call to that app's own MCP server, started on first use.

use crate::apps::{self, APPS, Config, Gui, Installed};
use crate::child::{Child, Failure, PROTOCOL};
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const CALL_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const WINDOW_WAIT: Duration = Duration::from_secs(90);
const INSTRUCTIONS_PER_APP: usize = 2500;

#[derive(Default)]
pub struct Options {
    /// Only the artcraft_* tools (for clients that cap the number of tools, e.g. Cursor).
    pub compact: bool,
    /// Never drive app windows.
    pub headless: bool,
    pub config: Option<PathBuf>,
}

#[derive(Default)]
struct Slot {
    child: Option<Child>,
    /// The window is open but bridging to it failed; stay headless until it closes or artcraft_open.
    bridge_error: Option<String>,
    start_error: Option<String>,
}

struct Gateway {
    opts: Options,
    cfg: Config,
    installed: Mutex<Vec<Arc<Installed>>>,
    slots: Mutex<HashMap<&'static str, Arc<Mutex<Slot>>>>,
    /// `{ "<id>": { "stamp": .., "headless": {tools, instructions}, "live": {tools, instructions} } }`
    cache: Mutex<Map<String, Value>>,
    routes: Mutex<HashMap<String, (&'static str, String)>>,
    listed: Mutex<Vec<String>>,
    out: Mutex<std::io::Stdout>,
}

fn cache_path() -> PathBuf {
    apps::data_dir().join("tools-cache.json")
}

fn log_path(id: &str) -> PathBuf {
    apps::data_dir().join("logs").join(format!("{id}.log"))
}

fn port_open(port: u16) -> bool {
    port != 0 && TcpStream::connect_timeout(&SocketAddr::from(([127, 0, 0, 1], port)), Duration::from_millis(400)).is_ok()
}

fn text_result(text: impl Into<String>, is_error: bool) -> Value {
    json!({ "content": [{ "type": "text", "text": text.into() }], "isError": is_error })
}

/// MCP tool names: `[A-Za-z0-9_-]{1,64}`.
fn tool_name(app: &str, tool: &str) -> String {
    let mut s: String = format!("{app}_{tool}").chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect();
    s.truncate(64);
    s
}

impl Gateway {
    fn new(opts: Options) -> Result<Gateway, String> {
        let cfg = Config::load(&opts.config.clone().unwrap_or_else(Config::path))?;
        let installed = apps::discover(&cfg).into_iter().map(Arc::new).collect();
        let cache = std::fs::read_to_string(cache_path())
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default();
        Ok(Gateway {
            opts,
            cfg,
            installed: Mutex::new(installed),
            slots: Mutex::new(HashMap::new()),
            cache: Mutex::new(cache),
            routes: Mutex::new(HashMap::new()),
            listed: Mutex::new(Vec::new()),
            out: Mutex::new(std::io::stdout()),
        })
    }

    fn send(&self, msg: &Value) {
        let mut out = self.out.lock().unwrap();
        let _ = writeln!(out, "{msg}");
        let _ = out.flush();
    }

    fn slot(&self, id: &'static str) -> Arc<Mutex<Slot>> {
        self.slots.lock().unwrap().entry(id).or_default().clone()
    }

    fn find(&self, id: &str) -> Option<Arc<Installed>> {
        self.installed.lock().unwrap().iter().find(|i| i.spec.id.eq_ignore_ascii_case(id)).cloned()
    }

    fn live_allowed(&self, inst: &Installed) -> bool {
        !self.opts.headless && self.cfg.gui != Gui::Never && inst.spec.bridge.is_some()
    }

    /// The app's tools from the cache (headless and live lists merged), if the cache matches the exe.
    fn cached_tools(&self, inst: &Installed) -> Option<Vec<Value>> {
        let cache = self.cache.lock().unwrap();
        let e = cache.get(inst.spec.id)?;
        if e["stamp"].as_str() != Some(inst.stamp.as_str()) {
            return None;
        }
        let mut tools: Vec<Value> = Vec::new();
        for mode in ["headless", "live"] {
            for t in e[mode]["tools"].as_array().into_iter().flatten() {
                if !tools.iter().any(|x| x["name"] == t["name"]) {
                    tools.push(t.clone());
                }
            }
        }
        (e["headless"].is_object() || e["live"].is_object()).then_some(tools)
    }

    fn remember(&self, inst: &Installed, child: &Child) {
        let mut cache = self.cache.lock().unwrap();
        let e = cache.entry(inst.spec.id.to_owned()).or_insert_with(|| json!({}));
        if e["stamp"].as_str() != Some(inst.stamp.as_str()) {
            *e = json!({ "stamp": inst.stamp });
        }
        e[if child.live { "live" } else { "headless" }] = json!({ "tools": child.tools, "instructions": child.instructions });
        let text = Value::Object(cache.clone()).to_string();
        drop(cache);
        let _ = std::fs::create_dir_all(apps::data_dir());
        let _ = std::fs::write(cache_path(), text);
    }

    /// Makes sure the slot holds a running server: the open window's if there is one, else headless.
    fn ensure_child(&self, inst: &Installed, slot: &mut Slot) -> Result<(), String> {
        let want_live = self.live_allowed(inst) && port_open(inst.port);
        if !want_live {
            slot.bridge_error = None;
        }
        let stay_headless = want_live && slot.bridge_error.is_some();
        if let Some(c) = slot.child.as_mut() {
            if c.alive() && (c.live == want_live || (stay_headless && !c.live)) {
                return Ok(());
            }
        }
        slot.child = None;
        let log = log_path(inst.spec.id);
        if let Some(dir) = log.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if want_live && !stay_headless {
            match Child::start(&inst.cli, &inst.bridge_args().unwrap_or_default(), true, &log) {
                Ok(c) => {
                    self.remember(inst, &c);
                    slot.child = Some(c);
                    slot.start_error = None;
                    return Ok(());
                }
                Err(e) => slot.bridge_error = Some(e),
            }
        }
        match Child::start(&inst.cli, &inst.headless_args(&self.cfg), false, &log) {
            Ok(c) => {
                self.remember(inst, &c);
                slot.child = Some(c);
                slot.start_error = None;
                Ok(())
            }
            Err(e) => {
                slot.start_error = Some(e.clone());
                Err(e)
            }
        }
    }

    /// Re-scans installed apps and starts (headless) the ones whose tools aren't cached yet.
    fn ensure_tools(&self) {
        let found: Vec<Arc<Installed>> = apps::discover(&self.cfg).into_iter().map(Arc::new).collect();
        *self.installed.lock().unwrap() = found.clone();
        let missing: Vec<Arc<Installed>> = found.into_iter().filter(|i| self.cached_tools(i).is_none()).collect();
        std::thread::scope(|s| {
            for inst in &missing {
                s.spawn(move || {
                    let slot = self.slot(inst.spec.id);
                    // An app busy with a call is running, so its tools will be cached by then.
                    if let Ok(mut slot) = slot.try_lock() {
                        let _ = self.ensure_child(inst, &mut slot);
                    }
                });
            }
        });
    }

    fn meta_tools(&self) -> Vec<Value> {
        let ids: Vec<&str> = APPS.iter().map(|a| a.id).collect();
        let mut tools = vec![
            json!({
                "name": "artcraft_apps",
                "description": "List the 13 ArtCraft apps: which are installed on this machine, where, whether the app's window is open and driven live, and how many tools each has.",
                "inputSchema": { "type": "object", "properties": {} },
                "annotations": { "readOnlyHint": true },
            }),
            json!({
                "name": "artcraft_open",
                "description": "Open an ArtCraft app's window (or attach to it if already open) so that app's tools drive the visible window instead of a headless engine. Use it when the user wants to see the work in the app. Not available for PdfCraft (headless only).",
                "inputSchema": { "type": "object", "properties": { "app": { "type": "string", "enum": ids, "description": "App id, e.g. vectorcraft" } }, "required": ["app"] },
            }),
            json!({
                "name": "artcraft_call",
                "description": "Call any tool of an ArtCraft app by its own name (as that app's MCP server names it), e.g. app=vectorcraft tool=draw_shape.",
                "inputSchema": { "type": "object", "properties": {
                    "app": { "type": "string", "enum": ids },
                    "tool": { "type": "string", "description": "The app's tool name, without the app prefix" },
                    "arguments": { "type": "object", "description": "The tool's arguments" }
                }, "required": ["app", "tool"] },
            }),
        ];
        if self.opts.compact {
            tools.push(json!({
                "name": "artcraft_tools",
                "description": "List one ArtCraft app's tools with their descriptions and input schemas, to use with artcraft_call.",
                "inputSchema": { "type": "object", "properties": { "app": { "type": "string", "enum": ids } }, "required": ["app"] },
                "annotations": { "readOnlyHint": true },
            }));
        }
        tools
    }

    /// The full tool list, and the name -> (app, tool) routes behind it.
    fn build_tools(&self) -> Vec<Value> {
        let mut tools = self.meta_tools();
        let mut routes = HashMap::new();
        if !self.opts.compact {
            let installed = self.installed.lock().unwrap().clone();
            for inst in installed {
                for t in self.cached_tools(&inst).unwrap_or_default() {
                    let Some(orig) = t["name"].as_str() else { continue };
                    let mut name = tool_name(inst.spec.id, orig);
                    let mut n = 2;
                    while routes.contains_key(&name) {
                        name = format!("{}{n}", &tool_name(inst.spec.id, orig)[..60.min(name.len())]);
                        n += 1;
                    }
                    let mut t = t.clone();
                    let desc = t["description"].as_str().unwrap_or("").to_owned();
                    t["name"] = json!(name);
                    t["description"] = json!(format!("[{}] {desc}", inst.spec.name));
                    if let Some(title) = t["title"].as_str() {
                        t["title"] = json!(format!("{}: {title}", inst.spec.name));
                    }
                    routes.insert(name, (inst.spec.id, orig.to_owned()));
                    tools.push(t);
                }
            }
        }
        *self.routes.lock().unwrap() = routes;
        tools
    }

    fn list_tools(&self) -> Value {
        self.ensure_tools();
        let tools = self.build_tools();
        *self.listed.lock().unwrap() = tools.iter().filter_map(|t| t["name"].as_str().map(str::to_owned)).collect();
        json!({ "tools": tools })
    }

    /// Tells the client to fetch tools again when an app's list grew (e.g. live-only tools).
    fn notify_if_changed(&self) {
        if self.opts.compact {
            return;
        }
        let names: Vec<String> = self.build_tools().iter().filter_map(|t| t["name"].as_str().map(str::to_owned)).collect();
        let mut listed = self.listed.lock().unwrap();
        if !listed.is_empty() && *listed != names {
            *listed = names;
            drop(listed);
            self.send(&json!({ "jsonrpc": "2.0", "method": "notifications/tools/list_changed" }));
        }
    }

    fn call_app(&self, app: &str, tool: &str, args: Value) -> Result<Value, Value> {
        let Some(inst) = self.find(app).or_else(|| {
            self.ensure_tools();
            self.find(app)
        }) else {
            let known = apps::spec(app).map_or("unknown app", |_| "not installed on this machine (install it with ArtCraft Launcher)");
            return Ok(text_result(format!("{app}: {known}"), true));
        };
        let slot = self.slot(inst.spec.id);
        let mut s = slot.lock().unwrap();
        if let Err(e) = self.ensure_child(&inst, &mut s) {
            return Ok(text_result(format!("{}: can't start its MCP server: {e}", inst.spec.name), true));
        }
        let child = s.child.as_mut().expect("ensure_child");
        let r = child.request("tools/call", json!({ "name": tool, "arguments": args }), CALL_TIMEOUT);
        let result = match r {
            Ok(v) => Ok(v),
            Err(Failure::Rpc(e)) => Err(e),
            Err(f) => {
                let msg = child.describe(f);
                s.child = None;
                return Ok(text_result(format!("{}: {msg}", inst.spec.name), true));
            }
        };
        if child.tools_changed && child.list_tools().is_ok() {
            self.remember(&inst, child);
        }
        drop(s);
        self.notify_if_changed();
        result
    }

    fn open(&self, app: &str) -> Result<String, String> {
        let inst = self.find(app).ok_or_else(|| format!("{app}: not installed on this machine"))?;
        let name = inst.spec.name;
        if !inst.can_open() {
            return Err(format!("{name} has no window to drive over MCP; its tools work headless, just call them."));
        }
        if !self.live_allowed(&inst) {
            return Err("driving app windows is turned off (artcraft-mcp --headless, or \"gui\": \"never\" in config.json)".into());
        }
        let mut launched = false;
        if !port_open(inst.port) {
            let gui = inst.gui.as_ref().expect("can_open");
            let mut cmd = std::process::Command::new(gui);
            // The window outlives this server and must never write into the MCP stream.
            cmd.args(inst.gui_args()).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
            if let Some(dir) = gui.parent() {
                cmd.current_dir(dir);
            }
            // Node-based clients (Claude Code, Cursor) run servers in a kill-on-close job: break away
            // from it so closing the client doesn't close the window (and lose unsaved work).
            #[cfg(windows)]
            let spawned = {
                use std::os::windows::process::CommandExt;
                const DETACHED_PROCESS: u32 = 0x0000_0008;
                const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
                const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
                cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_BREAKAWAY_FROM_JOB);
                cmd.spawn().or_else(|_| {
                    cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
                    cmd.spawn()
                })
            };
            #[cfg(not(windows))]
            let spawned = cmd.spawn();
            spawned.map_err(|e| format!("can't open {}: {e}", gui.display()))?;
            launched = true;
            let start = Instant::now();
            while !port_open(inst.port) {
                if start.elapsed() > WINDOW_WAIT {
                    return Err(format!(
                        "{name} started but its control port {} did not open within {} s. If {name} was already open without --control, close it and try again.",
                        inst.port,
                        WINDOW_WAIT.as_secs()
                    ));
                }
                std::thread::sleep(Duration::from_millis(250));
            }
        }
        let slot = self.slot(inst.spec.id);
        let mut s = slot.lock().unwrap();
        s.bridge_error = None;
        self.ensure_child(&inst, &mut s)?;
        let live = s.child.as_ref().is_some_and(|c| c.live);
        let err = s.bridge_error.clone();
        drop(s);
        self.notify_if_changed();
        if !live {
            return Err(format!("{name} is open on port {} but the bridge failed: {}", inst.port, err.unwrap_or_default()));
        }
        let how = if launched { "opened" } else { "already open, attached" };
        let mut text = format!("{name} {how} (control port {}). Its {}_* tools now drive the window.", inst.port, inst.spec.id);
        if launched {
            text.push_str(" A freshly opened window may have no document: if a tool says so, create or open one first (e.g. command_run with id file.new or file.open).");
        }
        Ok(text)
    }

    fn status(&self) -> Value {
        let installed = self.installed.lock().unwrap().clone();
        let apps: Vec<Value> = APPS
            .iter()
            .map(|spec| {
                let Some(inst) = installed.iter().find(|i| i.spec.id == spec.id) else {
                    return json!({ "app": spec.id, "name": spec.name, "kind": spec.what, "installed": false });
                };
                let slot = self.slot(spec.id);
                let (mode, err) = match slot.try_lock() {
                    Ok(mut s) => {
                        let mode = match s.child.as_mut() {
                            Some(c) => match (c.alive(), c.live) {
                                (true, true) => "live window",
                                (true, false) => "headless",
                                _ => "stopped",
                            },
                            None => "not started",
                        };
                        (mode, s.start_error.clone().or(s.bridge_error.clone()))
                    }
                    Err(_) => ("busy", None),
                };
                json!({
                    "app": spec.id, "name": spec.name, "kind": spec.what, "installed": true,
                    "cli": inst.cli, "window_exe": inst.gui,
                    "control_port": inst.can_open().then_some(inst.port),
                    "window_open": inst.can_open() && port_open(inst.port),
                    "mode": mode,
                    "tools": self.cached_tools(inst).map(|t| t.len()),
                    "error": err,
                })
            })
            .collect();
        json!({ "gateway": env!("CARGO_PKG_VERSION"), "data_dir": apps::data_dir(), "apps": apps })
    }

    fn instructions(&self) -> String {
        let installed = self.installed.lock().unwrap().clone();
        let names: Vec<String> = installed.iter().map(|i| format!("{} ({})", i.spec.name, i.spec.id)).collect();
        let mut s = String::from(
            "ArtCraft suite: one MCP server for all ArtCraft apps on this machine. Each app's tools are named <app>_<tool> (e.g. vectorcraft_draw_shape) and run in that app's own MCP server, started on first use. An app runs headless (no window) unless its window is open with its control channel; call artcraft_open(app) first when the user wants to watch the work in the app window, and the app's tools then drive that window. artcraft_apps shows what is installed and running; artcraft_call(app, tool, arguments) calls a tool by the app's own name.",
        );
        if self.opts.compact {
            s.push_str(" This client sees only the artcraft_* tools: use artcraft_tools(app) to see an app's tools, then artcraft_call.");
        }
        s.push_str(&format!("\nInstalled: {}.", if names.is_empty() { "none found yet".into() } else { names.join(", ") }));
        let cache = self.cache.lock().unwrap();
        for inst in &installed {
            let Some(e) = cache.get(inst.spec.id) else { continue };
            let text = e["live"]["instructions"].as_str().or(e["headless"]["instructions"].as_str()).unwrap_or("");
            if text.is_empty() {
                continue;
            }
            let cut: String = text.chars().take(INSTRUCTIONS_PER_APP).collect();
            s.push_str(&format!("\n\n## {} ({}_* tools)\n{cut}", inst.spec.name, inst.spec.id));
        }
        s
    }

    fn handle_call(&self, params: &Value) -> Result<Value, Value> {
        let name = params["name"].as_str().unwrap_or("");
        let args = if params["arguments"].is_object() { params["arguments"].clone() } else { json!({}) };
        let app = args["app"].as_str().unwrap_or("");
        match name {
            "artcraft_apps" => Ok(text_result(serde_json::to_string_pretty(&self.status()).unwrap_or_default(), false)),
            "artcraft_open" => Ok(match self.open(app) {
                Ok(t) => text_result(t, false),
                Err(e) => text_result(e, true),
            }),
            "artcraft_call" => {
                let tool = args["tool"].as_str().unwrap_or("");
                let a = if args["arguments"].is_object() { args["arguments"].clone() } else { json!({}) };
                self.call_app(app, tool, a)
            }
            "artcraft_tools" => {
                let Some(inst) = self.find(app) else {
                    return Ok(text_result(format!("{app}: not installed on this machine"), true));
                };
                if self.cached_tools(&inst).is_none() {
                    self.ensure_tools();
                }
                let tools: Vec<Value> = self
                    .cached_tools(&inst)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|t| json!({ "name": t["name"], "description": t["description"], "inputSchema": t["inputSchema"] }))
                    .collect();
                Ok(text_result(serde_json::to_string(&tools).unwrap_or_default(), false))
            }
            _ => {
                let known = self.routes.lock().unwrap().get(name).cloned();
                let route = known.or_else(|| {
                    self.ensure_tools();
                    self.build_tools();
                    self.routes.lock().unwrap().get(name).cloned()
                });
                match route {
                    Some((app, tool)) => self.call_app(app, &tool, args),
                    None => Err(json!({ "code": -32602, "message": format!("unknown tool: {name}") })),
                }
            }
        }
    }
}

pub fn serve(opts: Options) -> Result<(), String> {
    let gw = Arc::new(Gateway::new(opts)?);
    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                gw.send(&json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32700, "message": e.to_string() } }));
                continue;
            }
        };
        let Some(method) = msg["method"].as_str() else { continue };
        let id = msg["id"].clone();
        if id.is_null() {
            continue; // a notification
        }
        let reply = |r: Result<Value, Value>| match r {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err(error) => json!({ "jsonrpc": "2.0", "id": id, "error": error }),
        };
        match method {
            "initialize" => {
                let asked = msg["params"]["protocolVersion"].as_str().unwrap_or(PROTOCOL);
                let version = if ["2024-11-05", "2025-03-26", "2025-06-18"].contains(&asked) { asked } else { PROTOCOL };
                gw.send(&reply(Ok(json!({
                    "protocolVersion": version,
                    "capabilities": { "tools": { "listChanged": true } },
                    "serverInfo": { "name": "artcraft", "title": "ArtCraft suite", "version": env!("CARGO_PKG_VERSION") },
                    "instructions": gw.instructions(),
                }))));
            }
            "ping" => gw.send(&reply(Ok(json!({})))),
            "tools/list" => {
                let r = gw.list_tools();
                gw.send(&reply(Ok(r)));
            }
            "tools/call" => {
                let gw = gw.clone();
                let params = msg["params"].clone();
                std::thread::spawn(move || {
                    let msg = match gw.handle_call(&params) {
                        Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
                        Err(error) => json!({ "jsonrpc": "2.0", "id": id, "error": error }),
                    };
                    gw.send(&msg);
                });
            }
            "resources/list" => gw.send(&reply(Ok(json!({ "resources": [] })))),
            "resources/templates/list" => gw.send(&reply(Ok(json!({ "resourceTemplates": [] })))),
            "prompts/list" => gw.send(&reply(Ok(json!({ "prompts": [] })))),
            _ => gw.send(&reply(Err(json!({ "code": -32601, "message": format!("method not found: {method}") })))),
        }
    }
    // stdin closed: the client is gone. Stop the apps' servers (windows stay open).
    gw.slots.lock().unwrap().clear();
    Ok(())
}

/// `artcraft-mcp --list`: what is installed, and whether each app's MCP server answers.
pub fn print_apps(opts: Options) -> Result<(), String> {
    let gw = Gateway::new(opts)?;
    println!("config: {}", gw.opts.config.clone().unwrap_or_else(Config::path).display());
    println!("data:   {}", apps::data_dir().display());
    let installed = gw.installed.lock().unwrap().clone();
    for spec in APPS {
        let Some(inst) = installed.iter().find(|i| i.spec.id == spec.id) else {
            println!("{:<12} not installed", spec.id);
            continue;
        };
        let start = Instant::now();
        let mut slot = Slot::default();
        let r = gw.ensure_child(inst, &mut slot);
        let mode = slot.child.as_ref().map_or("", |c| if c.live { "live window" } else { "headless" });
        match r {
            Ok(()) => println!(
                "{:<12} {:>4} tools  {mode:<11} {:>5} ms  {}",
                spec.id,
                slot.child.as_ref().map_or(0, |c| c.tools.len()),
                start.elapsed().as_millis(),
                inst.cli.display()
            ),
            Err(e) => println!("{:<12} ERROR {}: {e}", spec.id, inst.cli.display()),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_names_are_valid_mcp_names() {
        assert_eq!(tool_name("pdfcraft", "page.rotate"), "pdfcraft_page_rotate");
        assert_eq!(tool_name("vectorcraft", "draw_shape"), "vectorcraft_draw_shape");
        assert_eq!(tool_name("gridcraft", &"x".repeat(100)).len(), 64);
    }
}
