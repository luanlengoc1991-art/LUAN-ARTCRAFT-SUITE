//! LUAN-ARTCRAFT-SUITE: the AI / MCP Command Center panel.
//!
//! - **AI Chat**: a request in plain words (Vietnamese is fine) goes to an AI agent already on this
//!   computer, through that agent's own command-line interface (`claude -p`, `codex exec`,
//!   `cursor-agent -p`), signed in as the person already is: nothing here logs in, asks for an API
//!   key or reads a token. The agent is given this window's MCP server (`vectorcraft-cli mcp
//!   --connect` to the control channel, which the panel turns on, loopback only) and works on the
//!   open document. What it changed shows on the canvas; Keep changes or Undo changes takes all of
//!   its steps back at once.
//! - **MCP Console**: VectorCraft's own MCP tools ([`vectorcraft_mcp::tool_definitions`]) run on the
//!   open document in this process, the same code path an MCP client reaches: JSON arguments, the
//!   result, a history to run again from.
//! - **Connections**: what is on this computer (Claude Code, Cursor, ChatGPT through Codex, the
//!   Claude desktop app) and whether each reaches VectorCraft, found from their own files without
//!   touching credentials; Test Connection runs the MCP server's handshake, Copy Config gives the
//!   entry for another client, Logs what the panel did.
//!
//! Desktop only: the web build has no processes to start (`panel` explains that there).

use egui::{RichText, Ui};

use crate::VectorcraftApp;
use crate::widgets::dim_label;

pub const ID: &str = "aiCenter";

#[cfg(target_arch = "wasm32")]
pub fn show(_app: &mut VectorcraftApp, ui: &mut Ui) {
    dim_label(ui, tl!("The AI / MCP Command Center needs the desktop app."));
}

#[cfg(not(target_arch = "wasm32"))]
pub use desktop::show;

#[cfg(not(target_arch = "wasm32"))]
mod desktop {
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::io::{BufRead, BufReader, Write};
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, Stdio};
    use std::sync::mpsc::{Receiver, Sender, channel};
    use std::time::{Duration, Instant};

    use serde_json::{Value, json};
    use vectorcraft_mcp::Backend;

    use super::*;
    use crate::control::{ControlRequest, Outcome};

    /// The MCP server's name in the agents' tool names (`mcp__vectorcraft__draw_shape`).
    const SERVER: &str = "vectorcraft";
    const LOG_LINES: usize = 300;

    #[derive(Clone, Copy, PartialEq, Eq, Default)]
    enum Tab {
        #[default]
        Chat,
        Console,
        Connections,
    }

    /// An AI agent with an official command line this panel can hand a request to.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub(crate) enum Agent {
        Claude,
        Codex,
        Cursor,
    }

    impl Agent {
        const ALL: [Agent; 3] = [Agent::Claude, Agent::Codex, Agent::Cursor];

        fn label(self) -> &'static str {
            match self {
                Agent::Claude => "Claude Code",
                Agent::Codex => "ChatGPT (Codex CLI)",
                Agent::Cursor => "Cursor Agent",
            }
        }

        fn program(self) -> &'static str {
            match self {
                Agent::Claude => "claude",
                Agent::Codex => "codex",
                Agent::Cursor => "cursor-agent",
            }
        }
    }

    #[derive(Clone)]
    pub(crate) enum Line {
        User(String),
        Ai(String),
        Tool(String),
        Error(String),
        /// How the run went: turns, time, estimated cost.
        Info(String),
        /// The account's AI usage limit is reached (the agent's own words): the app works on.
        Limit(String),
    }

    impl Line {
        fn to_pair(&self) -> [String; 2] {
            let (k, t) = match self {
                Line::User(t) => ("user", t),
                Line::Ai(t) => ("ai", t),
                Line::Tool(t) => ("tool", t),
                Line::Error(t) => ("error", t),
                Line::Info(t) => ("info", t),
                Line::Limit(t) => ("limit", t),
            };
            [k.to_string(), t.clone()]
        }

        fn from_pair([k, t]: [String; 2]) -> Line {
            match k.as_str() {
                "user" => Line::User(t),
                "tool" => Line::Tool(t),
                "error" => Line::Error(t),
                "info" => Line::Info(t),
                "limit" => Line::Limit(t),
                _ => Line::Ai(t),
            }
        }
    }

    /// Whether an agent's error says the account's plan has no AI usage left for now.
    pub(crate) fn is_usage_limit(error: &str) -> bool {
        let e = error.to_lowercase();
        ["usage limit", "limit reached", "rate limit", "rate_limit", "out of extra usage", "quota", "insufficient credits", "credit balance"]
            .iter()
            .any(|k| e.contains(k))
    }

    /// The Claude account the agent signs in with (`claude auth status`): never a credential.
    #[derive(Clone)]
    pub(crate) struct Account {
        logged_in: bool,
        /// `claude.ai` (a Claude plan) or an API key method.
        method: String,
        email: String,
        plan: String,
    }

    impl Account {
        pub(crate) fn parse(json: &str) -> Option<Account> {
            let v: Value = serde_json::from_str(json.trim()).ok()?;
            Some(Account {
                logged_in: v["loggedIn"].as_bool()?,
                method: v["authMethod"].as_str().unwrap_or("").to_string(),
                email: v["email"].as_str().unwrap_or("").to_string(),
                plan: v["subscriptionType"].as_str().unwrap_or("").to_string(),
            })
        }

        /// "luan@… · Pro", or the sign-in method when there's no plan.
        fn label(&self) -> String {
            let plan = match self.plan.as_str() {
                "" => self.method.clone(),
                p => {
                    let mut c = p.chars();
                    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
                }
            };
            if self.email.is_empty() { plan } else { format!("{} · {plan}", self.email) }
        }
    }

    fn claude_account(exe: &Path) -> Result<Account, String> {
        let mut cmd = Command::new(exe);
        cmd.args(["auth", "status"]).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
        no_window(&mut cmd);
        let out = cmd.output().map_err(|e| e.to_string())?;
        Account::parse(&String::from_utf8_lossy(&out.stdout)).ok_or_else(|| "unreadable account status".to_string())
    }

    enum Msg {
        Line(Line),
        /// The agent's conversation id, to continue it with the next request.
        Session(String),
        Done(Result<(), String>),
    }

    /// One conversation, as saved on this computer (`ai-chats.json`).
    #[derive(Clone, serde::Serialize, serde::Deserialize)]
    struct SavedChat {
        id: u64,
        title: String,
        agent: String,
        session: Option<String>,
        lines: Vec<[String; 2]>,
    }

    const MAX_SAVED_CHATS: usize = 50;

    fn chats_path() -> Option<PathBuf> {
        let base = std::env::var_os("APPDATA").map(PathBuf::from).or_else(|| dirs_home().map(|h| h.join(".config")))?;
        Some(base.join("VectorCraft").join("ai-chats.json"))
    }

    fn load_chats() -> Vec<SavedChat> {
        chats_path().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }

    fn save_chats(chats: &[SavedChat]) -> Result<(), String> {
        let path = chats_path().ok_or("no folder for saved chats")?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let text = serde_json::to_string(chats).map_err(|e| e.to_string())?;
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
    }

    struct Run {
        child: Child,
        rx: Receiver<Msg>,
        /// The document and its undo depth when the agent started.
        base: Option<(u64, usize)>,
    }

    /// Steps an agent made, waiting for Keep or Undo.
    struct Review {
        uid: u64,
        steps: usize,
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    pub(crate) enum Status {
        Connected,
        Disconnected,
        Unsupported,
    }

    pub(crate) struct Conn {
        name: &'static str,
        status: Status,
        detail: String,
    }

    struct HistoryItem {
        tool: String,
        args: String,
        ok: bool,
    }

    #[derive(Default)]
    struct State {
        tab: Tab,
        // AI Chat
        agents: Option<Vec<(Agent, PathBuf)>>,
        agent: Option<Agent>,
        prompt: String,
        transcript: Vec<Line>,
        run: Option<Run>,
        review: Option<Review>,
        /// The agent's conversation the next request continues (Claude Code), and this chat's id.
        session: Option<String>,
        chat_id: Option<u64>,
        chats: Option<Vec<SavedChat>>,
        // MCP Console
        tools: Vec<Value>,
        filter: String,
        selected: Option<String>,
        args: String,
        result: Option<(bool, String)>,
        history: Vec<HistoryItem>,
        // Connections
        conns: Option<Vec<Conn>>,
        conns_rx: Option<Receiver<Vec<Conn>>>,
        test_rx: Option<Receiver<Result<String, String>>>,
        test: Option<Result<String, String>>,
        log: VecDeque<String>,
        /// The Claude account the agent uses, once read.
        account: Option<Result<Account, String>>,
        account_rx: Option<Receiver<Result<Account, String>>>,
    }

    impl State {
        fn log(&mut self, line: impl Into<String>) {
            if self.log.len() == LOG_LINES {
                self.log.pop_front();
            }
            self.log.push_back(line.into());
        }
    }

    thread_local! {
        static STATE: RefCell<State> = RefCell::new(State::default());
    }

    pub fn show(app: &mut VectorcraftApp, ui: &mut Ui) {
        STATE.with_borrow_mut(|s| {
            poll(app, s, ui.ctx());
            ui.horizontal(|ui| {
                ui.selectable_value(&mut s.tab, Tab::Chat, tl!("AI Chat"));
                ui.selectable_value(&mut s.tab, Tab::Console, tl!("MCP Console"));
                ui.selectable_value(&mut s.tab, Tab::Connections, tl!("Connections"));
            });
            ui.separator();
            match s.tab {
                Tab::Chat => chat(app, s, ui),
                Tab::Console => console(app, s, ui),
                Tab::Connections => connections(app, s, ui),
            }
        });
    }

    /// Takes in what the agent, the connection check and the test sent since the last frame.
    fn poll(app: &mut VectorcraftApp, s: &mut State, ctx: &egui::Context) {
        if let Some(run) = s.run.as_mut() {
            let mut finished = None;
            while let Ok(msg) = run.rx.try_recv() {
                match msg {
                    // The plan's usage limit gets its own line: the app works on, only AI waits.
                    Msg::Line(Line::Error(e)) if is_usage_limit(&e) => s.transcript.push(Line::Limit(e)),
                    Msg::Line(line) => s.transcript.push(line),
                    Msg::Session(id) => s.session = Some(id),
                    Msg::Done(r) => finished = Some(r),
                }
            }
            if let Some(result) = finished {
                let base = run.base;
                let _ = run.child.wait();
                s.run = None;
                match result {
                    Ok(()) => s.log("agent finished"),
                    Err(e) => {
                        s.log(format!("agent failed: {e}"));
                        // Shown once: the agent's own error line may already say it.
                        let shown = s.transcript.last().is_some_and(|l| matches!(l, Line::Error(t) | Line::Limit(t) if *t == e));
                        if !shown {
                            s.transcript.push(if is_usage_limit(&e) { Line::Limit(e) } else { Line::Error(e) });
                        }
                    }
                }
                if let Some((uid, depth)) = base
                    && let Some(d) = app.session.active().filter(|d| d.uid == uid)
                {
                    let steps = d.history.undo.len().saturating_sub(depth);
                    if steps > 0 {
                        s.review = Some(Review { uid, steps });
                    }
                }
                save_current(s);
            } else {
                ctx.request_repaint_after(Duration::from_millis(200));
            }
        }
        if let Some(rx) = &s.conns_rx
            && let Ok(conns) = rx.try_recv()
        {
            s.conns = Some(conns);
            s.conns_rx = None;
        }
        if let Some(rx) = &s.test_rx
            && let Ok(r) = rx.try_recv()
        {
            match &r {
                Ok(t) => s.log(format!("test: {t}")),
                Err(e) => s.log(format!("test failed: {e}")),
            }
            s.test = Some(r);
            s.test_rx = None;
        }
        if let Some(rx) = &s.account_rx
            && let Ok(a) = rx.try_recv()
        {
            s.account = Some(a);
            s.account_rx = None;
        }
        if s.conns_rx.is_some() || s.test_rx.is_some() || s.account_rx.is_some() {
            ctx.request_repaint_after(Duration::from_millis(200));
        }
    }

    /// Reads the Claude account again (after Sign in, or Refresh).
    fn refresh_account(s: &mut State) {
        let Some(exe) = s.agents.as_ref().and_then(|v| v.iter().find(|(a, _)| *a == Agent::Claude)).map(|(_, p)| p.clone()) else { return };
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(claude_account(&exe));
        });
        s.account = None;
        s.account_rx = Some(rx);
    }

    /// Who the agent works as: the Claude account and plan, Sign in when there's none, and the
    /// plan's usage page. Requests always use the plan's usage, never an API key.
    fn account_row(s: &mut State, ui: &mut Ui) {
        if s.account.is_none() && s.account_rx.is_none() {
            refresh_account(s);
        }
        ui.horizontal_wrapped(|ui| match &s.account {
            None => {
                ui.spinner();
            }
            Some(Ok(a)) if a.logged_in => {
                ui.label(RichText::new(format!("Claude: {}", a.label())).small()).on_hover_text(tl!("Uses your Claude plan's usage, no API key."));
                if ui.small_button(tl!("Plan usage")).clicked() {
                    ui.ctx().open_url(egui::OpenUrl::new_tab("https://claude.ai/settings/usage"));
                }
            }
            Some(_) => {
                ui.label(RichText::new(tl!("Not signed in to Claude")).small().color(ui.visuals().warn_fg_color));
                if ui.small_button(tl!("Sign in")).clicked() {
                    sign_in(s);
                }
                if ui.small_button(tl!("Refresh")).clicked() {
                    refresh_account(s);
                }
            }
        });
    }

    /// Claude Code's own sign-in (`claude auth login`) in a console window: it opens the browser
    /// to sign in to the Claude account. The panel never sees a credential.
    fn sign_in(s: &mut State) {
        let Some(exe) = s.agents.as_ref().and_then(|v| v.iter().find(|(a, _)| *a == Agent::Claude)).map(|(_, p)| p.clone()) else { return };
        let started = if cfg!(windows) {
            Command::new("cmd").args(["/c", "start", "Claude"]).arg(&exe).args(["auth", "login"]).spawn()
        } else {
            Command::new(&exe).args(["auth", "login"]).spawn()
        };
        match started {
            Ok(_) => s.log("started claude auth login"),
            Err(e) => s.log(format!("claude auth login: {e}")),
        }
    }

    // ---------------------------------------------------------------------------------- AI Chat

    fn chat(app: &mut VectorcraftApp, s: &mut State, ui: &mut Ui) {
        let agents = s.agents.get_or_insert_with(find_agents).clone();
        if agents.is_empty() {
            // On one line: the translation tests read each translated literal as written.
            #[rustfmt::skip]
            let none = tl!("No AI agent found on this computer. Install Claude Code, Codex CLI or Cursor Agent, or run tools from the MCP Console.");
            ui.label(RichText::new(none).small());
        } else {
            if s.agent.is_none_or(|a| !agents.iter().any(|(x, _)| *x == a)) {
                s.agent = agents.first().map(|(a, _)| *a);
            }
            ui.horizontal(|ui| {
                dim_label(ui, tl!("Agent:"));
                let current = s.agent.map_or("", |a| a.label());
                egui::ComboBox::from_id_salt("aiCenterAgent").selected_text(current).show_ui(ui, |ui| {
                    for (a, _) in &agents {
                        ui.selectable_value(&mut s.agent, Some(*a), a.label());
                    }
                });
            });
            if s.agent == Some(Agent::Claude) {
                account_row(s, ui);
            }
        }
        chat_bar(s, ui);
        // A fixed-height transcript that scrolls, so the input below always stays in view however
        // long the conversation gets (a floating panel grows with its content otherwise).
        let height = (ui.available_height() - 150.0).clamp(120.0, 380.0);
        egui::ScrollArea::vertical()
            .id_salt("aiCenterTranscript")
            .max_height(height)
            .min_scrolled_height(height)
            .auto_shrink([false, false])
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
            .stick_to_bottom(true)
            .show(ui, |ui| transcript(s, ui));
        if let Some(review) = &s.review {
            let steps = review.steps;
            ui.horizontal_wrapped(|ui| {
                dim_label(ui, &format!("{} {steps}", tl!("Changes on the canvas:")));
                if ui.button(tl!("Keep changes")).clicked() {
                    s.log(format!("kept {steps} steps"));
                    s.review = None;
                } else if ui.button(tl!("Undo changes")).clicked() {
                    undo_steps(app, s);
                }
            });
        }
        let running = s.run.is_some();
        ui.add(
            egui::TextEdit::multiline(&mut s.prompt)
                .hint_text(tl!("Describe what to do, e.g. Change the logo to teal"))
                .desired_rows(2)
                .desired_width(f32::INFINITY),
        );
        ui.horizontal(|ui| {
            if running {
                ui.spinner();
                if ui.button(tl!("Stop")).clicked()
                    && let Some(run) = s.run.as_mut()
                {
                    let _ = run.child.kill();
                    s.log("agent stopped");
                }
            } else {
                let can = s.agent.is_some() && !s.prompt.trim().is_empty();
                if ui.add_enabled(can, egui::Button::new(tl!("Send"))).clicked() {
                    send(app, s, ui.ctx());
                }
            }
        });
    }

    /// The conversation, consecutive tool calls on one line.
    fn transcript(s: &State, ui: &mut Ui) {
        let mut tools: Vec<&str> = Vec::new();
        let flush = |tools: &mut Vec<&str>, ui: &mut Ui| {
            if !tools.is_empty() {
                ui.label(RichText::new(format!("⚙ {}", tools.join(" · "))).small().weak());
                tools.clear();
            }
        };
        for line in &s.transcript {
            if let Line::Tool(t) = line {
                tools.push(t);
                continue;
            }
            flush(&mut tools, ui);
            match line {
                Line::User(t) => ui.label(RichText::new(format!("› {t}")).strong()),
                Line::Ai(t) => ui.label(t),
                Line::Error(t) => ui.label(RichText::new(t).color(ui.visuals().error_fg_color)),
                Line::Info(t) => ui.label(RichText::new(t).small().weak()),
                Line::Limit(t) => {
                    // On one line: the translation tests read each translated literal as written.
                    #[rustfmt::skip]
                    let limit = tl!("Your Claude plan's AI usage limit is reached. The app keeps working; AI requests work again when the limit resets.");
                    ui.label(RichText::new(limit).color(ui.visuals().warn_fg_color));
                    ui.label(RichText::new(t).small().weak())
                }
                Line::Tool(_) => continue,
            };
        }
        flush(&mut tools, ui);
    }

    /// New chat, and the chats saved on this computer to reopen.
    fn chat_bar(s: &mut State, ui: &mut Ui) {
        let running = s.run.is_some();
        ui.horizontal(|ui| {
            if ui.add_enabled(!running, egui::Button::new(tl!("New chat"))).clicked() {
                save_current(s);
                s.transcript.clear();
                s.session = None;
                s.chat_id = None;
                s.review = None;
            }
            ui.add_enabled_ui(!running, |ui| {
                ui.menu_button(tl!("Chat history"), |ui| {
                    let chats = s.chats.get_or_insert_with(load_chats);
                    if chats.is_empty() {
                        dim_label(ui, tl!("No saved chats"));
                    }
                    let mut open = None;
                    for c in chats.iter().rev() {
                        if ui.button(format!("{} · {}", c.title, c.agent)).clicked() {
                            open = Some(c.clone());
                            ui.close();
                        }
                    }
                    if let Some(c) = open {
                        save_current(s);
                        s.transcript = c.lines.into_iter().map(Line::from_pair).collect();
                        s.session = c.session;
                        s.chat_id = Some(c.id);
                        s.review = None;
                    }
                });
            });
        });
    }

    /// Saves the current conversation (new, or over its earlier copy).
    fn save_current(s: &mut State) {
        let Some(first) = s.transcript.iter().find_map(|l| if let Line::User(t) = l { Some(t.clone()) } else { None }) else { return };
        let id = *s.chat_id.get_or_insert_with(|| {
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(0))
        });
        let title: String = first.chars().take(40).collect();
        let agent = s.agent.map_or("", |a| a.label()).to_string();
        let chat = SavedChat { id, title, agent, session: s.session.clone(), lines: s.transcript.iter().map(Line::to_pair).collect() };
        let chats = s.chats.get_or_insert_with(load_chats);
        chats.retain(|c| c.id != id);
        chats.push(chat);
        if chats.len() > MAX_SAVED_CHATS {
            let extra = chats.len() - MAX_SAVED_CHATS;
            chats.drain(..extra);
        }
        let result = save_chats(chats);
        if let Err(e) = result {
            s.log(format!("couldn't save the chat: {e}"));
        }
    }

    /// Undoes the steps the last agent run made (when its document is still the active one).
    fn undo_steps(app: &mut VectorcraftApp, s: &mut State) {
        let Some(review) = s.review.take() else { return };
        if app.session.active().is_none_or(|d| d.uid != review.uid) {
            s.transcript.push(Line::Error("The document the agent changed isn't the active one any more.".into()));
            return;
        }
        let mut undone = 0;
        for _ in 0..review.steps {
            if app.run("edit.undo", json!({})).is_err() {
                break;
            }
            undone += 1;
        }
        s.log(format!("undid {undone} of {} steps", review.steps));
    }

    fn send(app: &mut VectorcraftApp, s: &mut State, ctx: &egui::Context) {
        let prompt = s.prompt.trim().to_string();
        let Some(agent) = s.agent else { return };
        let Some(exe) = s.agents.as_ref().and_then(|v| v.iter().find(|(a, _)| *a == agent)).map(|(_, p)| p.clone()) else { return };
        s.transcript.push(Line::User(prompt.clone()));
        s.review = None;
        let port = match app.ensure_control() {
            Ok(p) => p,
            Err(e) => {
                s.transcript.push(Line::Error(format!("Couldn't open the control channel: {e}")));
                return;
            }
        };
        let Some(cli) = cli_path() else {
            s.transcript.push(Line::Error("vectorcraft-cli wasn't found next to the app.".into()));
            return;
        };
        let resume = if agent == Agent::Claude { s.session.clone() } else { None };
        let (program, args) = agent_command(agent, &exe, &prompt, &cli, port, resume.as_deref());
        let mut cmd = Command::new(&program);
        cmd.args(&args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        // Requests use the signed-in account's plan, never an API key that would bill per token.
        for key in ["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN", "OPENAI_API_KEY"] {
            cmd.env_remove(key);
        }
        if let Some(dir) = app.session.active().and_then(|d| d.path.as_deref()).and_then(|p| Path::new(p).parent()) {
            cmd.current_dir(dir);
        } else if let Some(home) = dirs_home() {
            cmd.current_dir(home);
        }
        no_window(&mut cmd);
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                s.transcript.push(Line::Error(format!("Couldn't start {}: {e}", agent.label())));
                return;
            }
        };
        s.log(format!("started {} for: {prompt}", agent.label()));
        let base = app.session.active().map(|d| (d.uid, d.history.undo.len()));
        let (tx, rx) = channel();
        let stderr = child.stderr.take();
        let stdout = child.stdout.take();
        let ctx = ctx.clone();
        std::thread::spawn(move || read_agent(agent, stdout, stderr, &tx, &ctx));
        s.run = Some(Run { child, rx, base });
        s.prompt.clear();
    }

    /// What the agent is told besides the request.
    fn system_prompt(port: u16) -> String {
        format!(
            "You are working inside VectorCraft, a vector illustration app, for its user. Its open window is reached through the MCP server \"{SERVER}\" (control channel 127.0.0.1:{port}): carry out the request on the open document with those tools, for real, then say briefly what you did. Answer in the language the user wrote in (often Vietnamese). Don't ask for confirmation for ordinary edits; the user can undo all of your steps at once."
        )
    }

    /// The program and arguments that hand `prompt` to `agent`, with VectorCraft's MCP server for
    /// this window.
    pub(crate) fn agent_command(agent: Agent, exe: &Path, prompt: &str, cli: &Path, port: u16, resume: Option<&str>) -> (PathBuf, Vec<String>) {
        let addr = format!("127.0.0.1:{port}");
        let cli = cli.to_string_lossy().to_string();
        let sys = system_prompt(port);
        let args = match agent {
            Agent::Claude => {
                let config = json!({"mcpServers": {SERVER: {"command": cli, "args": ["mcp", "--connect", addr]}}}).to_string();
                // The same conversation goes on, so "make it bigger" knows what "it" is.
                let resume = resume.map(|id| vec!["--resume".to_string(), id.to_string()]).unwrap_or_default();
                resume
                    .into_iter()
                    .chain([
                        "-p".into(),
                        prompt.into(),
                        "--output-format".into(),
                        "stream-json".into(),
                        "--verbose".into(),
                        "--strict-mcp-config".into(),
                        "--mcp-config".into(),
                        config,
                        "--allowedTools".into(),
                        format!("mcp__{SERVER}"),
                        "--append-system-prompt".into(),
                        sys,
                    ])
                    .collect()
            }
            Agent::Codex => vec![
                "exec".into(),
                "--json".into(),
                "--skip-git-repo-check".into(),
                "-c".into(),
                format!("mcp_servers.{SERVER}.command={}", toml_string(&cli)),
                "-c".into(),
                format!("mcp_servers.{SERVER}.args=[\"mcp\", \"--connect\", {}]", toml_string(&addr)),
                format!("{sys}\n\n{prompt}"),
            ],
            // Cursor Agent uses the MCP servers registered with Cursor (ArtCraft MCP reaches the
            // window on its control port).
            Agent::Cursor => vec!["-p".into(), format!("{sys}\n\n{prompt}"), "--output-format".into(), "text".into()],
        };
        (exe.to_path_buf(), args)
    }

    fn toml_string(s: &str) -> String {
        serde_json::to_string(s).unwrap_or_default()
    }

    fn read_agent(
        agent: Agent,
        stdout: Option<std::process::ChildStdout>,
        stderr: Option<std::process::ChildStderr>,
        tx: &Sender<Msg>,
        ctx: &egui::Context,
    ) {
        let err_tail = std::sync::Arc::new(std::sync::Mutex::new(VecDeque::<String>::new()));
        let tail = err_tail.clone();
        let err_thread = stderr.map(|e| {
            std::thread::spawn(move || {
                for line in BufReader::new(e).lines().map_while(Result::ok) {
                    let mut t = tail.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                    if t.len() == 8 {
                        t.pop_front();
                    }
                    t.push_back(line);
                }
            })
        });
        let mut said = false;
        let mut failed = None;
        if let Some(out) = stdout {
            let mut session_sent = false;
            for raw in BufReader::new(out).lines().map_while(Result::ok) {
                if agent == Agent::Claude
                    && !session_sent
                    && let Some(id) = serde_json::from_str::<Value>(&raw).ok().and_then(|v| v["session_id"].as_str().map(str::to_owned))
                {
                    session_sent = true;
                    let _ = tx.send(Msg::Session(id));
                }
                for line in parse_agent_line(agent, &raw) {
                    match &line {
                        Line::Ai(_) => said = true,
                        Line::Error(e) => failed = Some(e.clone()),
                        _ => {}
                    }
                    let _ = tx.send(Msg::Line(line));
                    ctx.request_repaint();
                }
            }
        }
        if let Some(t) = err_thread {
            let _ = t.join();
        }
        let tail: Vec<String> = err_tail.lock().unwrap_or_else(std::sync::PoisonError::into_inner).iter().cloned().collect();
        let result = match failed {
            Some(e) => Err(e),
            None if !said && !tail.is_empty() => Err(tail.join("\n")),
            None => Ok(()),
        };
        let _ = tx.send(Msg::Done(result));
        ctx.request_repaint();
    }

    /// One line of an agent's output, as transcript lines.
    pub(crate) fn parse_agent_line(agent: Agent, raw: &str) -> Vec<Line> {
        let raw = raw.trim();
        if raw.is_empty() {
            return vec![];
        }
        let short = |name: &str| name.trim_start_matches(&format!("mcp__{SERVER}__")).to_string();
        match agent {
            Agent::Claude => {
                let Ok(v) = serde_json::from_str::<Value>(raw) else { return vec![Line::Ai(raw.into())] };
                match v["type"].as_str() {
                    Some("assistant") => v["message"]["content"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|c| match c["type"].as_str() {
                            Some("text") => c["text"].as_str().filter(|t| !t.trim().is_empty()).map(|t| Line::Ai(t.trim().into())),
                            Some("tool_use") => c["name"].as_str().map(|n| Line::Tool(short(n))),
                            _ => None,
                        })
                        .collect(),
                    Some("result") if v["is_error"].as_bool() == Some(true) => {
                        vec![Line::Error(v["result"].as_str().unwrap_or("the agent reported an error").into())]
                    }
                    // How the run went. On a Claude plan the cost is Claude Code's estimate of
                    // what it would have cost at API prices: the plan's usage is what's spent.
                    Some("result") => {
                        let mut parts = vec!["✓".to_string()];
                        if let Some(ms) = v["duration_ms"].as_f64().filter(|m| m.is_finite()) {
                            parts.push(format!("{:.0} s", ms / 1000.0));
                        }
                        if let Some(n) = v["num_turns"].as_u64() {
                            parts.push(format!("{n} ↻"));
                        }
                        if let Some(c) = v["total_cost_usd"].as_f64().filter(|c| c.is_finite()) {
                            parts.push(format!("≈ ${c:.3}"));
                        }
                        vec![Line::Info(parts.join(" · "))]
                    }
                    _ => vec![],
                }
            }
            Agent::Codex => {
                let Ok(v) = serde_json::from_str::<Value>(raw) else { return vec![Line::Ai(raw.into())] };
                let item = if v["item"].is_object() { &v["item"] } else { &v["msg"] };
                let kind = item["type"].as_str().unwrap_or("");
                if kind.contains("tool") {
                    let name = item["tool"].as_str().or(item["name"].as_str()).unwrap_or(kind);
                    vec![Line::Tool(short(name))]
                } else if let Some(t) = item["text"].as_str().or(item["message"].as_str()).filter(|t| !t.trim().is_empty()) {
                    if kind.contains("error") { vec![Line::Error(t.into())] } else { vec![Line::Ai(t.trim().into())] }
                } else if v["type"].as_str() == Some("error") {
                    vec![Line::Error(v["message"].as_str().unwrap_or("error").into())]
                } else {
                    vec![]
                }
            }
            Agent::Cursor => vec![Line::Ai(raw.into())],
        }
    }

    // ------------------------------------------------------------------------------- MCP Console

    /// This window as an MCP backend: tool calls run the control channel's methods in this process.
    struct Live<'a> {
        app: &'a mut VectorcraftApp,
        ctx: egui::Context,
    }

    impl Backend for Live<'_> {
        fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
            let (req, _rx) = ControlRequest::new(method, params);
            match crate::control::handle(self.app, &self.ctx, &req) {
                Outcome::Done(v) if v["ok"].as_bool() == Some(true) => Ok(v["result"].clone()),
                Outcome::Done(v) => Err(v["error"].as_str().unwrap_or("error").to_string()),
                Outcome::Screenshot { .. } => Err("a window screenshot answers after a frame: use render_preview here".into()),
            }
        }
        fn has_ui(&self) -> bool {
            true
        }
        fn describe(&self) -> String {
            "in-app".into()
        }
    }

    fn console(app: &mut VectorcraftApp, s: &mut State, ui: &mut Ui) {
        if s.tools.is_empty() {
            s.tools = vectorcraft_mcp::tool_definitions();
        }
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut s.filter).hint_text(tl!("Search")).desired_width(f32::INFINITY));
        });
        let needle = s.filter.to_lowercase();
        egui::ScrollArea::vertical().id_salt("aiCenterTools").max_height(140.0).show(ui, |ui| {
            for t in &s.tools {
                let Some(name) = t["name"].as_str() else { continue };
                let desc = t["description"].as_str().unwrap_or("");
                if !needle.is_empty() && !name.to_lowercase().contains(&needle) && !desc.to_lowercase().contains(&needle) {
                    continue;
                }
                let on = s.selected.as_deref() == Some(name);
                if ui.selectable_label(on, name).on_hover_text(desc).clicked() && !on {
                    s.selected = Some(name.to_string());
                    s.args = example_args(&t["inputSchema"]);
                    s.result = None;
                }
            }
        });
        let Some(name) = s.selected.clone() else {
            dim_label(ui, &format!("{} {}", s.tools.len(), tl!("tools")));
            history(s, ui);
            return;
        };
        ui.separator();
        ui.label(RichText::new(&name).strong());
        if let Some(desc) = s.tools.iter().find(|t| t["name"] == name.as_str()).and_then(|t| t["description"].as_str()) {
            ui.label(RichText::new(desc).small());
        }
        dim_label(ui, tl!("Arguments (JSON)"));
        ui.add(egui::TextEdit::multiline(&mut s.args).code_editor().desired_rows(3).desired_width(f32::INFINITY));
        if ui.button(tl!("Execute")).clicked() {
            execute(app, s, &name, ui.ctx());
        }
        if let Some((ok, text)) = &s.result {
            let color = if *ok { ui.visuals().text_color() } else { ui.visuals().error_fg_color };
            egui::ScrollArea::vertical().id_salt("aiCenterResult").max_height(160.0).show(ui, |ui| {
                ui.label(RichText::new(text).monospace().small().color(color));
            });
        }
        history(s, ui);
    }

    fn history(s: &mut State, ui: &mut Ui) {
        if s.history.is_empty() {
            return;
        }
        ui.separator();
        dim_label(ui, tl!("History"));
        let mut pick = None;
        for (i, h) in s.history.iter().enumerate().rev().take(20) {
            let mark = if h.ok { "✓" } else { "✗" };
            if ui.selectable_label(false, format!("{mark} {} {}", h.tool, h.args)).clicked() {
                pick = Some(i);
            }
        }
        if let Some(h) = pick.and_then(|i| s.history.get(i)) {
            s.selected = Some(h.tool.clone());
            s.args = h.args.clone();
        }
    }

    /// `{}` with the required properties of a tool's input schema, as a starting point.
    fn example_args(schema: &Value) -> String {
        let mut o = serde_json::Map::new();
        for key in schema["required"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            let v = match schema["properties"][key]["type"].as_str() {
                Some("number" | "integer") => json!(0),
                Some("boolean") => json!(false),
                Some("array") => json!([]),
                Some("object") => json!({}),
                _ => json!(""),
            };
            o.insert(key.to_string(), v);
        }
        serde_json::to_string_pretty(&Value::Object(o)).unwrap_or_else(|_| "{}".into())
    }

    fn execute(app: &mut VectorcraftApp, s: &mut State, name: &str, ctx: &egui::Context) {
        let text = if s.args.trim().is_empty() { "{}" } else { s.args.as_str() };
        let args: Value = match serde_json::from_str(text) {
            Ok(v @ Value::Object(_)) => v,
            Ok(_) => {
                s.result = Some((false, "The arguments must be a JSON object.".into()));
                return;
            }
            Err(e) => {
                s.result = Some((false, format!("Invalid JSON: {e}")));
                return;
            }
        };
        let started = Instant::now();
        let out = vectorcraft_mcp::call_tool(&mut Live { app, ctx: ctx.clone() }, name, &args).to_value();
        let ok = out["isError"].as_bool() != Some(true);
        let mut text = String::new();
        for c in out["content"].as_array().into_iter().flatten() {
            match c["type"].as_str() {
                Some("text") => text.push_str(c["text"].as_str().unwrap_or("")),
                Some(kind) => text.push_str(&format!("[{kind}]")),
                None => {}
            }
            text.push('\n');
        }
        s.log(format!("{name} {} in {} ms", if ok { "ok" } else { "failed" }, started.elapsed().as_millis()));
        s.history.push(HistoryItem { tool: name.to_string(), args: serde_json::to_string(&args).unwrap_or_default(), ok });
        if s.history.len() > 100 {
            s.history.remove(0);
        }
        s.result = Some((ok, text.trim_end().to_string()));
    }

    // ------------------------------------------------------------------------------- Connections

    fn connections(app: &mut VectorcraftApp, s: &mut State, ui: &mut Ui) {
        if s.conns.is_none() && s.conns_rx.is_none() {
            refresh(app, s);
        }
        ui.horizontal_wrapped(|ui| {
            if ui.button(tl!("Refresh")).clicked() {
                s.agents = None;
                s.account = None;
                refresh(app, s);
            }
            if ui.add_enabled(s.test_rx.is_none(), egui::Button::new(tl!("Test Connection"))).clicked() {
                test(app, s);
            }
            if ui.button(tl!("Copy Config")).clicked() {
                let text = copy_config(app);
                ui.ctx().copy_text(text);
                s.log("copied the MCP config");
            }
        });
        let port = app.control_port;
        ui.horizontal_wrapped(|ui| {
            dim_label(ui, tl!("Live control of this window:"));
            match port {
                Some(p) => {
                    ui.label(format!("127.0.0.1:{p}"));
                }
                None => {
                    if ui.button(tl!("Turn on")).clicked() {
                        match app.ensure_control() {
                            Ok(p) => s.log(format!("control channel on 127.0.0.1:{p}")),
                            Err(e) => s.log(format!("control channel: {e}")),
                        }
                    }
                }
            }
        });
        match &s.conns {
            None => {
                ui.spinner();
            }
            Some(conns) => {
                egui::Grid::new("aiCenterConns").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                    for c in conns {
                        ui.label(RichText::new(c.name).strong());
                        let (text, color) = match c.status {
                            Status::Connected => (tl!("Connected"), egui::Color32::from_rgb(40, 160, 90)),
                            Status::Disconnected => (tl!("Disconnected"), egui::Color32::from_rgb(210, 140, 30)),
                            Status::Unsupported => (tl!("Unsupported"), ui.visuals().weak_text_color()),
                        };
                        ui.label(RichText::new(text).color(color)).on_hover_text(&c.detail);
                        ui.end_row();
                    }
                });
            }
        }
        if s.test_rx.is_some() {
            ui.spinner();
        }
        if let Some(t) = &s.test {
            match t {
                Ok(t) => ui.label(RichText::new(t).small()),
                Err(e) => ui.label(RichText::new(e).small().color(ui.visuals().error_fg_color)),
            };
        }
        egui::CollapsingHeader::new(tl!("Logs")).id_salt("aiCenterLogs").show(ui, |ui| {
            egui::ScrollArea::vertical().max_height(140.0).stick_to_bottom(true).show(ui, |ui| {
                for l in &s.log {
                    ui.label(RichText::new(l).monospace().small());
                }
            });
        });
    }

    fn refresh(app: &VectorcraftApp, s: &mut State) {
        let (tx, rx) = channel();
        let port = app.control_port;
        std::thread::spawn(move || {
            let _ = tx.send(detect(port));
        });
        s.conns_rx = Some(rx);
        s.log("checking connections");
    }

    /// The MCP server entry for this window, for any MCP client.
    fn copy_config(app: &VectorcraftApp) -> String {
        let cli = cli_path().map(|p| p.to_string_lossy().to_string()).unwrap_or_else(|| "vectorcraft-cli".into());
        let args = match app.control_port {
            Some(p) => json!(["mcp", "--connect", format!("127.0.0.1:{p}")]),
            None => json!(["mcp"]),
        };
        serde_json::to_string_pretty(&json!({"mcpServers": {SERVER: {"command": cli, "args": args}}})).unwrap_or_default()
    }

    /// Runs the MCP server's handshake (`initialize`, `tools/list`) the way a client would: against
    /// this window when its control channel is on, else headless.
    fn test(app: &VectorcraftApp, s: &mut State) {
        let (tx, rx) = channel();
        let port = app.control_port;
        s.test = None;
        s.test_rx = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(handshake(port));
        });
    }

    fn handshake(port: Option<u16>) -> Result<String, String> {
        let cli = cli_path().ok_or("vectorcraft-cli wasn't found next to the app")?;
        let mut cmd = Command::new(&cli);
        cmd.arg("mcp");
        if let Some(p) = port {
            cmd.args(["--connect", &format!("127.0.0.1:{p}")]);
        }
        cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
        no_window(&mut cmd);
        let mut child = cmd.spawn().map_err(|e| format!("couldn't start {}: {e}", cli.display()))?;
        let mut stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let msgs = [
            json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "vectorcraft-command-center", "version": "1"}}}),
            json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}}),
        ];
        for m in msgs {
            writeln!(stdin, "{m}").map_err(|e| e.to_string())?;
        }
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut result = Err("no answer from the MCP server".to_string());
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            let Ok(line) = rx.recv_timeout(left) else { break };
            let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
            if v["id"] == 2 {
                let n = v["result"]["tools"].as_array().map_or(0, Vec::len);
                let mode = if port.is_some() { "this window" } else { "headless" };
                result = if n > 0 { Ok(format!("MCP OK: {n} tools ({mode})")) } else { Err(format!("MCP answered without tools: {line}")) };
                break;
            }
        }
        drop(stdin);
        let _ = child.kill();
        let _ = child.wait();
        result
    }

    // ---------------------------------------------------------------------------------- Detection

    fn dirs_home() -> Option<PathBuf> {
        std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from)
    }

    /// `vectorcraft-cli` next to this executable.
    fn cli_path() -> Option<PathBuf> {
        let exe = std::env::current_exe().ok()?;
        let name = if cfg!(windows) { "vectorcraft-cli.exe" } else { "vectorcraft-cli" };
        Some(exe.parent()?.join(name)).filter(|p| p.is_file())
    }

    fn no_window(cmd: &mut Command) {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        #[cfg(not(windows))]
        let _ = cmd;
    }

    /// `program` on PATH (`.exe`, `.cmd`, `.bat` on Windows), or where its installer puts it.
    fn find_program(agent: Agent) -> Option<PathBuf> {
        let program = agent.program();
        let exts: &[&str] = if cfg!(windows) { &[".exe", ".cmd", ".bat"] } else { &[""] };
        if let Some(path) = std::env::var_os("PATH") {
            for dir in std::env::split_paths(&path) {
                for ext in exts {
                    let p = dir.join(format!("{program}{ext}"));
                    if p.is_file() {
                        return Some(p);
                    }
                }
            }
        }
        let home = dirs_home()?;
        for ext in exts {
            let p = home.join(".local").join("bin").join(format!("{program}{ext}"));
            if p.is_file() {
                return Some(p);
            }
        }
        if agent == Agent::Claude {
            // Bundled with the VS Code / Cursor extension: the newest.
            let exe = if cfg!(windows) { "claude.exe" } else { "claude" };
            let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
            for editor in [".vscode", ".cursor", ".vscode-insiders", ".windsurf"] {
                let Ok(entries) = std::fs::read_dir(home.join(editor).join("extensions")) else { continue };
                for e in entries.flatten() {
                    if !e.file_name().to_string_lossy().starts_with("anthropic.claude-code-") {
                        continue;
                    }
                    let p = e.path().join("resources").join("native-binary").join(exe);
                    let t = e.metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
                    if p.is_file() && best.as_ref().is_none_or(|(b, _)| t > *b) {
                        best = Some((t, p));
                    }
                }
            }
            return best.map(|(_, p)| p);
        }
        None
    }

    fn find_agents() -> Vec<(Agent, PathBuf)> {
        Agent::ALL.into_iter().filter_map(|a| find_program(a).map(|p| (a, p))).collect()
    }

    /// Whether an MCP config at `path` has a server that reaches VectorCraft (`artcraft` or
    /// `vectorcraft`). Only its `mcpServers` is looked at.
    fn registered(path: &Path) -> Option<bool> {
        let text = std::fs::read_to_string(path).ok()?;
        let v: Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
        let servers = v["mcpServers"].as_object()?;
        Some(servers.contains_key("artcraft") || servers.contains_key(SERVER))
    }

    fn yes_no(b: Option<bool>) -> &'static str {
        match b {
            Some(true) => "yes",
            Some(false) => "no",
            None => "unknown",
        }
    }

    pub(crate) fn detect(port: Option<u16>) -> Vec<Conn> {
        let home = dirs_home().unwrap_or_default();
        let mut out = Vec::new();
        // Claude Code.
        let claude = find_program(Agent::Claude);
        let claude_mcp = registered(&home.join(".claude.json"));
        out.push(match &claude {
            Some(p) => Conn {
                name: "Claude Code",
                status: Status::Connected,
                detail: format!("CLI: {} · MCP registered: {}", p.display(), yes_no(claude_mcp)),
            },
            None if claude_mcp.is_some() => Conn {
                name: "Claude Code",
                status: Status::Disconnected,
                detail: "Claude Code is set up, but its CLI wasn't found to send requests from here.".into(),
            },
            None => Conn { name: "Claude Code", status: Status::Unsupported, detail: "Claude Code isn't installed.".into() },
        });
        // Cursor.
        let cursor_dir = home.join(".cursor");
        let cursor_mcp = registered(&cursor_dir.join("mcp.json"));
        let cursor_agent = find_program(Agent::Cursor);
        out.push(if let Some(p) = &cursor_agent {
            Conn {
                name: "Cursor",
                status: Status::Connected,
                detail: format!("Cursor Agent: {} · MCP registered: {}", p.display(), yes_no(cursor_mcp)),
            }
        } else if cursor_dir.is_dir() {
            Conn {
                name: "Cursor",
                status: if cursor_mcp == Some(true) { Status::Connected } else { Status::Disconnected },
                detail: format!(
                    "Cursor's chat reaches VectorCraft through MCP (registered: {}). Sending requests from this panel needs the Cursor Agent CLI.",
                    yes_no(cursor_mcp)
                ),
            }
        } else {
            Conn { name: "Cursor", status: Status::Unsupported, detail: "Cursor isn't installed.".into() }
        });
        // ChatGPT: its own app can't reach MCP servers on this computer; Codex CLI signs in with it.
        out.push(match find_program(Agent::Codex) {
            Some(p) => Conn { name: "ChatGPT", status: Status::Connected, detail: format!("Codex CLI (signed in with ChatGPT): {}", p.display()) },
            None => Conn {
                name: "ChatGPT",
                status: Status::Unsupported,
                detail: "The ChatGPT app can't reach MCP servers on this computer. Install Codex CLI to use your ChatGPT account here.".into(),
            },
        });
        // The Claude desktop app (its chat).
        let mut desktop = Vec::new();
        if let Some(appdata) = std::env::var_os("APPDATA") {
            desktop.push(PathBuf::from(appdata).join("Claude").join("claude_desktop_config.json"));
        }
        if let Some(local) = std::env::var_os("LOCALAPPDATA")
            && let Ok(entries) = std::fs::read_dir(PathBuf::from(local).join("Packages"))
        {
            for e in entries.flatten() {
                if e.file_name().to_string_lossy().starts_with("Claude_") {
                    desktop.push(e.path().join("LocalCache").join("Roaming").join("Claude").join("claude_desktop_config.json"));
                }
            }
        }
        let found: Vec<_> = desktop.iter().filter(|p| p.parent().is_some_and(Path::is_dir)).collect();
        out.push(if found.is_empty() {
            Conn { name: "Claude Desktop", status: Status::Unsupported, detail: "The Claude desktop app isn't installed.".into() }
        } else if found.iter().any(|p| registered(p) == Some(true)) {
            Conn { name: "Claude Desktop", status: Status::Connected, detail: "Chat in the Claude app reaches VectorCraft through MCP.".into() }
        } else {
            Conn {
                name: "Claude Desktop",
                status: Status::Disconnected,
                detail: "The Claude app is installed without VectorCraft's MCP (ArtCraft Launcher adds it).".into(),
            }
        });
        // This window.
        out.push(match port {
            Some(p) => Conn { name: "VectorCraft", status: Status::Connected, detail: format!("Control channel 127.0.0.1:{p}") },
            None => {
                Conn { name: "VectorCraft", status: Status::Disconnected, detail: "The control channel is off; Send or Turn on opens it.".into() }
            }
        });
        out
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn claude_gets_this_windows_mcp_server_and_only_its_tools() {
            let (prog, args) = agent_command(
                Agent::Claude,
                Path::new("claude.exe"),
                "Đổi màu logo thành xanh ngọc",
                Path::new("C:/vc/vectorcraft-cli.exe"),
                7981,
                None,
            );
            assert_eq!(prog, PathBuf::from("claude.exe"));
            assert_eq!(args.get(1).map(String::as_str), Some("Đổi màu logo thành xanh ngọc"));
            let at = args.iter().position(|a| a == "--mcp-config").unwrap();
            let config: Value = serde_json::from_str(&args[at + 1]).unwrap();
            assert_eq!(config["mcpServers"]["vectorcraft"]["args"], json!(["mcp", "--connect", "127.0.0.1:7981"]));
            assert!(args.contains(&"--strict-mcp-config".to_string()));
            let allow = args.iter().position(|a| a == "--allowedTools").unwrap();
            assert_eq!(args[allow + 1], "mcp__vectorcraft");
        }

        #[test]
        fn codex_gets_the_server_as_config_overrides() {
            let (_, args) = agent_command(Agent::Codex, Path::new("codex.cmd"), "hi", Path::new("C:\\vc\\vectorcraft-cli.exe"), 7979, None);
            assert_eq!(args[0], "exec");
            assert!(args.iter().any(|a| a == "mcp_servers.vectorcraft.command=\"C:\\\\vc\\\\vectorcraft-cli.exe\""));
            assert!(args.iter().any(|a| a.contains("\"127.0.0.1:7979\"")));
            assert!(args.last().unwrap().ends_with("hi"));
        }

        #[test]
        fn claude_stream_lines_become_transcript_lines() {
            let text = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Đã đổi màu."},{"type":"tool_use","name":"mcp__vectorcraft__set_paint","input":{}}]}}"#;
            let lines = parse_agent_line(Agent::Claude, text);
            assert!(matches!(&lines[0], Line::Ai(t) if t == "Đã đổi màu."));
            assert!(matches!(&lines[1], Line::Tool(t) if t == "set_paint"));
            let err = parse_agent_line(Agent::Claude, r#"{"type":"result","is_error":true,"result":"Not logged in"}"#);
            assert!(matches!(&err[0], Line::Error(t) if t == "Not logged in"));
            assert!(parse_agent_line(Agent::Claude, r#"{"type":"system","subtype":"init"}"#).is_empty());
            assert!(parse_agent_line(Agent::Claude, "   ").is_empty());
        }

        #[test]
        fn a_follow_up_continues_the_claude_conversation() {
            let (_, args) = agent_command(Agent::Claude, Path::new("claude.exe"), "to hơn", Path::new("vc-cli"), 7979, Some("abc-123"));
            assert_eq!(args[..2], ["--resume".to_string(), "abc-123".to_string()]);
            assert_eq!(args[2], "-p");
        }

        #[test]
        fn a_finished_run_reports_time_turns_and_cost() {
            let lines = parse_agent_line(
                Agent::Claude,
                r#"{"type":"result","is_error":false,"duration_ms":23400,"num_turns":4,"total_cost_usd":0.0521,"session_id":"x"}"#,
            );
            assert!(
                matches!(&lines[0], Line::Info(t) if t == "✓ · 23 s · 4 ↻ · ≈ $0.052"),
                "{}",
                lines.first().map(|l| l.to_pair()[1].clone()).unwrap_or_default()
            );
        }

        #[test]
        fn a_spent_plan_is_told_apart_from_other_errors() {
            assert!(is_usage_limit("Claude AI usage limit reached|1792123200"));
            assert!(is_usage_limit("5-hour limit reached ∙ resets 3pm"));
            assert!(is_usage_limit("API Error: 429 rate_limit_error"));
            assert!(!is_usage_limit("Not logged in · Please run /login"));
            assert!(!is_usage_limit("tool draw_shape failed: no document"));
        }

        #[test]
        fn the_account_is_read_without_any_credential() {
            let a = Account::parse(r#"{"loggedIn": true, "authMethod": "claude.ai", "email": "me@example.com", "subscriptionType": "pro"}"#).unwrap();
            assert!(a.logged_in);
            assert_eq!(a.label(), "me@example.com · Pro");
            let out = Account::parse(r#"{"loggedIn": false}"#).unwrap();
            assert!(!out.logged_in);
            assert!(Account::parse("not json").is_none());
        }

        #[test]
        fn saved_lines_round_trip() {
            for l in [
                Line::User("a".into()),
                Line::Ai("b".into()),
                Line::Tool("c".into()),
                Line::Error("d".into()),
                Line::Info("e".into()),
                Line::Limit("f".into()),
            ] {
                let pair = l.to_pair();
                assert_eq!(Line::from_pair(pair.clone()).to_pair(), pair);
            }
        }

        #[test]
        fn codex_events_become_transcript_lines() {
            let a = parse_agent_line(Agent::Codex, r#"{"type":"item.completed","item":{"type":"agent_message","text":"Xong"}}"#);
            assert!(matches!(&a[0], Line::Ai(t) if t == "Xong"));
            let t = parse_agent_line(Agent::Codex, r#"{"type":"item.started","item":{"type":"mcp_tool_call","tool":"draw_shape"}}"#);
            assert!(matches!(&t[0], Line::Tool(t) if t == "draw_shape"));
        }

        #[test]
        fn example_args_fill_the_required_fields() {
            let schema = json!({"required": ["shape", "x"], "properties": {"shape": {"type": "string"}, "x": {"type": "number"}}});
            let v: Value = serde_json::from_str(&example_args(&schema)).unwrap();
            assert_eq!(v, json!({"shape": "", "x": 0}));
            assert_eq!(example_args(&json!({})), "{}");
        }

        #[test]
        fn the_console_runs_mcp_tools_on_the_open_document() {
            let mut app = crate::VectorcraftApp::new(vectorcraft_engine::Session::new(), Default::default());
            app.run("file.new", json!({"width": 200, "height": 200})).unwrap();
            let ctx = egui::Context::default();
            let before = app.session.active().map(|d| d.history.undo.len()).unwrap_or(0);
            let r = vectorcraft_mcp::call_tool(
                &mut Live { app: &mut app, ctx },
                "draw_shape",
                &json!({"shape": "ellipse", "x": 10, "y": 10, "width": 50, "height": 40, "fill": "#20b2aa"}),
            )
            .to_value();
            assert_ne!(r["isError"], true, "{r}");
            let after = app.session.active().map(|d| d.history.undo.len()).unwrap_or(0);
            assert!(after > before, "the shape is an undo step");
        }
    }
}
