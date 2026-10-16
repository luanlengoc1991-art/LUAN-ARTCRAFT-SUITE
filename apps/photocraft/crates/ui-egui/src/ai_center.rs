//! LUAN-ARTCRAFT-SUITE: the AI / MCP Command Center (Window › AI / MCP Command Center).
//!
//! - **AI Chat**: a request in plain words (Vietnamese is fine) goes to an AI agent already on this
//!   computer through that agent's own command line (`claude -p`, `codex exec`, `cursor-agent -p`),
//!   signed in as the person already is: nothing here logs in, asks for an API key or reads a
//!   credential. The agent is given PhotoCraft's MCP server bridged to this window
//!   (`photocraft-cli mcp --bridge` to the control channel, which the panel turns on, loopback
//!   only, token in a file) and works on the open document. Keep changes or Undo changes takes all
//!   of its steps back at once; follow-ups continue the same conversation.
//! - **MCP Console**: PhotoCraft's own MCP tools, called through the same server an agent uses
//!   (a `photocraft-cli mcp --bridge` session per call, on a worker thread: the window answers its
//!   control requests meanwhile), with JSON arguments, the result and a history.
//! - **Connections**: Claude Code, Cursor, ChatGPT (through Codex CLI), the Claude desktop app and
//!   this window: Connected / Disconnected / Unsupported, read from their own files without
//!   touching credentials; Test Connection, Copy Config, Logs.
//!
//! Desktop only: the web build has no processes to start.

use egui::{Context, RichText, Ui};

use crate::PhotocraftApp;

/// Window › AI / MCP Command Center.
pub const COMMAND: &str = "window.aiCenter";

#[cfg(target_arch = "wasm32")]
pub fn toggle() {}

#[cfg(target_arch = "wasm32")]
pub fn window(_app: &mut PhotocraftApp, _ctx: &Context) {}

#[cfg(not(target_arch = "wasm32"))]
pub use desktop::{toggle, window};

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

    use super::*;

    /// The MCP server's name in the agents' tool names (`mcp__photocraft__command_run`).
    const SERVER: &str = "photocraft";
    const LOG_LINES: usize = 300;
    const MAX_SAVED_CHATS: usize = 50;

    #[derive(Clone, Copy, PartialEq, Eq, Default)]
    enum Tab {
        #[default]
        Chat,
        Console,
        Connections,
    }

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
        Session(String),
        Done(Result<(), String>),
    }

    struct Run {
        child: Child,
        rx: Receiver<Msg>,
        /// The document (its index) and its undo depth when the agent started.
        base: Option<(usize, usize)>,
    }

    struct Review {
        index: usize,
        steps: usize,
    }

    #[derive(Clone, serde::Serialize, serde::Deserialize)]
    struct SavedChat {
        id: u64,
        title: String,
        agent: String,
        session: Option<String>,
        lines: Vec<[String; 2]>,
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
        open: bool,
        tab: Tab,
        // AI Chat
        agents: Option<Vec<(Agent, PathBuf)>>,
        agent: Option<Agent>,
        prompt: String,
        transcript: Vec<Line>,
        run: Option<Run>,
        review: Option<Review>,
        session: Option<String>,
        chat_id: Option<u64>,
        chats: Option<Vec<SavedChat>>,
        // MCP Console
        tools: Option<Vec<Value>>,
        tools_rx: Option<Receiver<Result<Vec<Value>, String>>>,
        tools_error: Option<String>,
        filter: String,
        selected: Option<String>,
        args: String,
        call_rx: Option<Receiver<Result<Value, String>>>,
        result: Option<(bool, String)>,
        history: Vec<HistoryItem>,
        // Connections
        conns: Option<Vec<Conn>>,
        conns_rx: Option<Receiver<Vec<Conn>>>,
        test_rx: Option<Receiver<Result<String, String>>>,
        test: Option<Result<String, String>>,
        log: VecDeque<String>,
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

    /// Shows or hides the window (Window › AI / MCP Command Center).
    pub fn toggle() {
        STATE.with_borrow_mut(|s| s.open = !s.open);
    }

    /// Draws the window when it is open, and takes in what its workers sent either way.
    pub fn window(app: &mut PhotocraftApp, ctx: &Context) {
        STATE.with_borrow_mut(|s| {
            poll(app, s, ctx);
            if !s.open {
                return;
            }
            let mut open = true;
            let right = ctx.content_rect().right_top() + egui::vec2(-380.0, 90.0);
            egui::Window::new(tl!("AI / MCP Command Center"))
                .id(egui::Id::new("aiCenterWindow"))
                .open(&mut open)
                .default_pos(right)
                .default_width(360.0)
                .resizable(true)
                .show(ctx, |ui| {
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
            s.open = open;
        });
    }

    fn poll(app: &mut PhotocraftApp, s: &mut State, ctx: &Context) {
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
                        let shown = s.transcript.last().is_some_and(|l| matches!(l, Line::Error(t) | Line::Limit(t) if *t == e));
                        if !shown {
                            s.transcript.push(if is_usage_limit(&e) { Line::Limit(e) } else { Line::Error(e) });
                        }
                    }
                }
                if let Some((index, depth)) = base
                    && app.session.active_index() == Some(index)
                    && let Some(d) = app.session.active()
                {
                    let steps = d.history.past_len().saturating_sub(depth);
                    if steps > 0 {
                        s.review = Some(Review { index, steps });
                    }
                }
                save_current(s);
            } else {
                ctx.request_repaint_after(Duration::from_millis(200));
            }
        }
        if let Some(rx) = &s.tools_rx
            && let Ok(r) = rx.try_recv()
        {
            match r {
                Ok(t) => {
                    s.log(format!("{} tools", t.len()));
                    s.tools = Some(t);
                }
                Err(e) => {
                    s.log(format!("tools/list failed: {e}"));
                    s.tools_error = Some(e);
                }
            }
            s.tools_rx = None;
        }
        if let Some(rx) = &s.call_rx
            && let Ok(r) = rx.try_recv()
        {
            let (ok, text) = match r {
                Ok(v) => {
                    let ok = v["isError"].as_bool() != Some(true);
                    let mut text = String::new();
                    for c in v["content"].as_array().into_iter().flatten() {
                        match c["type"].as_str() {
                            Some("text") => text.push_str(c["text"].as_str().unwrap_or("")),
                            Some(kind) => text.push_str(&format!("[{kind}]")),
                            None => {}
                        }
                        text.push('\n');
                    }
                    (ok, text.trim_end().to_string())
                }
                Err(e) => (false, e),
            };
            if let Some(h) = s.history.last_mut() {
                h.ok = ok;
            }
            s.log(format!("tool call {}", if ok { "ok" } else { "failed" }));
            s.result = Some((ok, text));
            s.call_rx = None;
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
        if s.tools_rx.is_some() || s.call_rx.is_some() || s.conns_rx.is_some() || s.test_rx.is_some() || s.account_rx.is_some() {
            ctx.request_repaint_after(Duration::from_millis(200));
        }
    }

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

    fn chat(app: &mut PhotocraftApp, s: &mut State, ui: &mut Ui) {
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
                ui.label(tl!("Agent:"));
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
        // A fixed-height transcript that scrolls, so the input below always stays in view.
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
                ui.label(format!("{} {steps}", tl!("Changes on the canvas:")));
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
                .hint_text(tl!("Describe what to do, e.g. Remove the background of this photo"))
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
                // Without an agent, commands the app does itself still run.
                let can = !s.prompt.trim().is_empty();
                if ui.add_enabled(can, egui::Button::new(tl!("Send"))).clicked() {
                    send(app, s, ui.ctx());
                }
            }
        });
    }

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
                        ui.label(tl!("No saved chats"));
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

    fn chats_path() -> Option<PathBuf> {
        let base = std::env::var_os("APPDATA").map(PathBuf::from).or_else(|| dirs_home().map(|h| h.join(".config")))?;
        Some(base.join("PhotoCraft").join("ai-chats.json"))
    }

    fn load_chats() -> Vec<SavedChat> {
        chats_path().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }

    fn save_current(s: &mut State) {
        let Some(first) = s.transcript.iter().find_map(|l| if let Line::User(t) = l { Some(t.clone()) } else { None }) else { return };
        let id = *s
            .chat_id
            .get_or_insert_with(|| std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(0)));
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
        let saved = chats_path().ok_or_else(|| "no folder for saved chats".to_string()).and_then(|path| {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            let text = serde_json::to_string(&*chats).map_err(|e| e.to_string())?;
            let tmp = path.with_extension("tmp");
            std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
            std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
        });
        if let Err(e) = saved {
            s.log(format!("couldn't save the chat: {e}"));
        }
    }

    fn undo_steps(app: &mut PhotocraftApp, s: &mut State) {
        let Some(review) = s.review.take() else { return };
        if app.session.active_index() != Some(review.index) {
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

    /// Runs the request in the app when it names a command (no AI, no usage): see
    /// [`local_command`]. False when it's one for the AI.
    fn run_locally(app: &mut PhotocraftApp, s: &mut State, ctx: &Context, prompt: &str) -> bool {
        let lang = crate::i18n::current();
        let commands: Vec<(String, Vec<String>)> = crate::menus::menu_items(app)
            .into_iter()
            .filter(|m| m.enabled && !m.id.is_empty())
            .map(|m| {
                let mut names = vec![m.label.clone(), crate::i18n::tr_id(lang, &m.id, &m.label).to_string()];
                if let Some(last) = m.path.last() {
                    names.push(format!("{last} {}", m.label));
                }
                (m.id, names)
            })
            .collect();
        let Some((id, params)) = local_command(prompt, &commands, "layer.new.layer") else { return false };
        s.transcript.push(Line::User(prompt.to_string()));
        s.review = None;
        match crate::menus::invoke(app, ctx, &id, params) {
            Ok(_) => {
                s.transcript.push(Line::Info(format!("⚡ {} ({id})", tl!("Done directly, no AI used."))));
                s.log(format!("ran {id} without AI"));
            }
            Err(e) => s.transcript.push(Line::Error(format!("{id}: {e}"))),
        }
        save_current(s);
        true
    }

    fn send(app: &mut PhotocraftApp, s: &mut State, ctx: &Context) {
        let prompt = s.prompt.trim().to_string();
        if run_locally(app, s, ctx, &prompt) {
            s.prompt.clear();
            return;
        }
        let Some(agent) = s.agent else {
            s.transcript.push(Line::User(prompt));
            // On one line: the translation tests read each translated literal as written.
            #[rustfmt::skip]
            let none = tl!("No AI agent found on this computer. Install Claude Code, Codex CLI or Cursor Agent, or run tools from the MCP Console.");
            s.transcript.push(Line::Error(none.to_string()));
            s.prompt.clear();
            return;
        };
        let Some(exe) = s.agents.as_ref().and_then(|v| v.iter().find(|(a, _)| *a == agent)).map(|(_, p)| p.clone()) else { return };
        s.transcript.push(Line::User(prompt.clone()));
        s.review = None;
        let (port, token) = match app.ensure_control() {
            Ok(c) => c,
            Err(e) => {
                s.transcript.push(Line::Error(format!("Couldn't open the control channel: {e}")));
                return;
            }
        };
        let Some(cli) = cli_path() else {
            s.transcript.push(Line::Error("photocraft-cli wasn't found next to the app.".into()));
            return;
        };
        let resume = if agent == Agent::Claude { s.session.clone() } else { None };
        let (program, args) = agent_command(agent, &exe, &prompt, &cli, port, &token, resume.as_deref());
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
        let base = app.session.active_index().zip(app.session.active().map(|d| d.history.past_len()));
        let (tx, rx) = channel();
        let stderr = child.stderr.take();
        let stdout = child.stdout.take();
        let ctx = ctx.clone();
        std::thread::spawn(move || read_agent(agent, stdout, stderr, &tx, &ctx));
        s.run = Some(Run { child, rx, base });
        s.prompt.clear();
    }

    fn system_prompt(port: u16) -> String {
        format!(
            "You are working inside PhotoCraft, an image editor, for its user. Its open window is reached through the MCP server \"{SERVER}\" (control channel 127.0.0.1:{port}): carry out the request on the open document with those tools, for real (find command ids with command_list, run them with command_run or command_batch, look with render_preview), then say briefly what you did. Answer in the language the user wrote in (often Vietnamese). Don't ask for confirmation for ordinary edits; the user can undo all of your steps at once."
        )
    }

    /// The MCP server entry that reaches this window.
    fn server_args(port: u16, token: &Path) -> Vec<String> {
        vec!["mcp".into(), "--bridge".into(), format!("127.0.0.1:{port}"), "--control-token-file".into(), token.to_string_lossy().to_string()]
    }

    pub(crate) fn agent_command(agent: Agent, exe: &Path, prompt: &str, cli: &Path, port: u16, token: &Path, resume: Option<&str>) -> (PathBuf, Vec<String>) {
        let cli = cli.to_string_lossy().to_string();
        let server = server_args(port, token);
        let sys = system_prompt(port);
        let args = match agent {
            Agent::Claude => {
                let config = json!({"mcpServers": {SERVER: {"command": cli, "args": server}}}).to_string();
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
                format!("mcp_servers.{SERVER}.args={}", serde_json::to_string(&server).unwrap_or_default()),
                format!("{sys}\n\n{prompt}"),
            ],
            Agent::Cursor => vec!["-p".into(), format!("{sys}\n\n{prompt}"), "--output-format".into(), "text".into()],
        };
        (exe.to_path_buf(), args)
    }

    fn toml_string(s: &str) -> String {
        serde_json::to_string(s).unwrap_or_default()
    }

    fn read_agent(agent: Agent, stdout: Option<std::process::ChildStdout>, stderr: Option<std::process::ChildStderr>, tx: &Sender<Msg>, ctx: &Context) {
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
                    // On a Claude plan the cost is Claude Code's estimate at API prices.
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

    // ------------------------------------------------------------------------- Without AI (free)

    /// Everyday phrases (Vietnamese and English) for commands the app runs itself, by command id.
    /// `NEW_LAYER` stands for the app's own New Layer command.
    const ALIASES: &[(&str, &str)] = &[
        ("hoàn tác", "edit.undo"),
        ("undo", "edit.undo"),
        ("làm lại", "edit.redo"),
        ("redo", "edit.redo"),
        ("tạo layer mới", "NEW_LAYER"),
        ("thêm layer mới", "NEW_LAYER"),
        ("thêm layer", "NEW_LAYER"),
        ("layer mới", "NEW_LAYER"),
        ("tạo lớp mới", "NEW_LAYER"),
        ("new layer", "NEW_LAYER"),
        ("chọn tất cả", "select.all"),
        ("select all", "select.all"),
        ("bỏ chọn", "select.deselect"),
        ("deselect", "select.deselect"),
        ("lưu", "file.save"),
        ("lưu file", "file.save"),
        ("save", "file.save"),
        ("sao chép", "edit.copy"),
        ("copy", "edit.copy"),
        ("dán", "edit.paste"),
        ("paste", "edit.paste"),
    ];

    /// Commands a name alone never runs: they close the window or the document.
    const NOT_BY_NAME: &[&str] = &["file.exit", "app.quit", "file.close", "file.closeAll", "file.revert"];

    /// Text as compared: lower case, single spaces, no polite lead-in, no trailing dots or marks.
    pub(crate) fn normalize(s: &str) -> String {
        let mut t = s.trim().to_lowercase();
        for lead in ["hãy ", "vui lòng ", "làm ơn ", "please "] {
            if let Some(rest) = t.strip_prefix(lead) {
                t = rest.to_string();
            }
        }
        let t = t.trim_end_matches(['.', '…', '!', '?', ' ']);
        t.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// A request the app does itself, without AI: `/command.id {json}`, an everyday phrase
    /// ([`ALIASES`]) or a command's name as its menu shows it, in English or the UI's language.
    /// `commands` holds the enabled commands with their names.
    pub(crate) fn local_command(prompt: &str, commands: &[(String, Vec<String>)], new_layer: &str) -> Option<(String, Value)> {
        let p = prompt.trim();
        let known = |id: &str| commands.iter().any(|(c, _)| c == id);
        if let Some(rest) = p.strip_prefix('/') {
            let (id, json) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
            let params = if json.trim().is_empty() { json!({}) } else { serde_json::from_str(json.trim()).ok()? };
            return known(id).then(|| (id.to_string(), params));
        }
        let n = normalize(p);
        if n.is_empty() || n.split_whitespace().count() > 6 {
            return None;
        }
        for (phrase, id) in ALIASES {
            let id = if *id == "NEW_LAYER" { new_layer } else { id };
            if n == *phrase && known(id) {
                return Some((id.to_string(), json!({})));
            }
        }
        commands
            .iter()
            .filter(|(id, _)| !NOT_BY_NAME.contains(&id.as_str()))
            .find(|(_, names)| names.iter().any(|x| normalize(x) == n))
            .map(|(id, _)| (id.clone(), json!({})))
    }

    // ------------------------------------------------------------------------------- MCP Console

    /// One request to PhotoCraft's MCP server bridged to this window: start it, shake hands, ask,
    /// stop it. Runs on a worker thread: the window answers the control requests meanwhile.
    pub(crate) fn mcp_request(port: u16, token: &Path, method: &str, params: Value, timeout: Duration) -> Result<Value, String> {
        let cli = cli_path().ok_or("photocraft-cli wasn't found next to the app")?;
        let mut cmd = Command::new(&cli);
        cmd.args(server_args(port, token)).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
        no_window(&mut cmd);
        let mut child = cmd.spawn().map_err(|e| format!("couldn't start {}: {e}", cli.display()))?;
        let mut stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let msgs = [
            json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "photocraft-command-center", "version": "1"}}}),
            json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
            json!({"jsonrpc": "2.0", "id": 2, "method": method, "params": params}),
        ];
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        // initialize first; rmcp answers requests only after its handshake.
        let mut result = Err("no answer from the MCP server".to_string());
        let deadline = Instant::now() + timeout;
        let mut sent = 0;
        for m in &msgs[..1] {
            writeln!(stdin, "{m}").map_err(|e| e.to_string())?;
            sent += 1;
        }
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            let Ok(line) = rx.recv_timeout(left) else { break };
            let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
            if v["id"] == 1 && sent == 1 {
                for m in &msgs[1..] {
                    writeln!(stdin, "{m}").map_err(|e| e.to_string())?;
                }
                sent = 3;
            } else if v["id"] == 2 {
                result = if v["error"].is_object() { Err(v["error"]["message"].as_str().unwrap_or("MCP error").to_string()) } else { Ok(v["result"].clone()) };
                break;
            }
        }
        drop(stdin);
        let _ = child.kill();
        let _ = child.wait();
        result
    }

    fn console(app: &mut PhotocraftApp, s: &mut State, ui: &mut Ui) {
        if s.tools.is_none() && s.tools_rx.is_none() && s.tools_error.is_none() {
            match app.ensure_control() {
                Ok((port, token)) => {
                    let (tx, rx) = channel();
                    std::thread::spawn(move || {
                        let r = mcp_request(port, &token, "tools/list", json!({}), Duration::from_secs(60))
                            .map(|v| v["tools"].as_array().cloned().unwrap_or_default());
                        let _ = tx.send(r);
                    });
                    s.tools_rx = Some(rx);
                }
                Err(e) => s.tools_error = Some(e),
            }
        }
        if let Some(e) = s.tools_error.clone() {
            ui.label(RichText::new(e).color(ui.visuals().error_fg_color));
            if ui.button(tl!("Refresh")).clicked() {
                s.tools_error = None;
            }
            return;
        }
        let Some(tools) = s.tools.clone() else {
            ui.spinner();
            return;
        };
        ui.add(egui::TextEdit::singleline(&mut s.filter).hint_text(tl!("Search")).desired_width(f32::INFINITY));
        let needle = s.filter.to_lowercase();
        egui::ScrollArea::vertical().id_salt("aiCenterTools").max_height(140.0).show(ui, |ui| {
            for t in &tools {
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
            ui.label(format!("{} {}", tools.len(), tl!("tools")));
            history(s, ui);
            return;
        };
        ui.separator();
        ui.label(RichText::new(&name).strong());
        if let Some(desc) = tools.iter().find(|t| t["name"] == name.as_str()).and_then(|t| t["description"].as_str()) {
            ui.label(RichText::new(desc).small());
        }
        ui.label(tl!("Arguments (JSON)"));
        ui.add(egui::TextEdit::multiline(&mut s.args).code_editor().desired_rows(3).desired_width(f32::INFINITY));
        let busy = s.call_rx.is_some();
        ui.horizontal(|ui| {
            if ui.add_enabled(!busy, egui::Button::new(tl!("Execute"))).clicked() {
                execute(app, s, &name);
            }
            if busy {
                ui.spinner();
            }
        });
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
        ui.label(tl!("History"));
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

    fn execute(app: &mut PhotocraftApp, s: &mut State, name: &str) {
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
        let (port, token) = match app.ensure_control() {
            Ok(c) => c,
            Err(e) => {
                s.result = Some((false, e));
                return;
            }
        };
        s.history.push(HistoryItem { tool: name.to_string(), args: serde_json::to_string(&args).unwrap_or_default(), ok: false });
        if s.history.len() > 100 {
            s.history.remove(0);
        }
        let (tx, rx) = channel();
        let name = name.to_string();
        std::thread::spawn(move || {
            let _ = tx.send(mcp_request(port, &token, "tools/call", json!({"name": name, "arguments": args}), Duration::from_secs(600)));
        });
        s.call_rx = Some(rx);
        s.result = None;
    }

    // ------------------------------------------------------------------------------- Connections

    fn connections(app: &mut PhotocraftApp, s: &mut State, ui: &mut Ui) {
        if s.conns.is_none() && s.conns_rx.is_none() {
            refresh(app, s);
        }
        ui.horizontal_wrapped(|ui| {
            if ui.button(tl!("Refresh")).clicked() {
                s.agents = None;
                s.account = None;
                s.tools = None;
                s.tools_error = None;
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
        ui.horizontal_wrapped(|ui| {
            ui.label(tl!("Live control of this window:"));
            match app.control_port {
                Some(p) => {
                    ui.label(format!("127.0.0.1:{p}"));
                }
                None => {
                    if ui.button(tl!("Turn on")).clicked() {
                        match app.ensure_control() {
                            Ok((p, _)) => s.log(format!("control channel on 127.0.0.1:{p}")),
                            Err(e) => s.log(format!("control channel: {e}")),
                        }
                        refresh(app, s);
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

    fn refresh(app: &PhotocraftApp, s: &mut State) {
        let (tx, rx) = channel();
        let port = app.control_port;
        std::thread::spawn(move || {
            let _ = tx.send(detect(port));
        });
        s.conns_rx = Some(rx);
        s.log("checking connections");
    }

    fn copy_config(app: &PhotocraftApp) -> String {
        let cli = cli_path().map(|p| p.to_string_lossy().to_string()).unwrap_or_else(|| "photocraft-cli".into());
        let args = match (app.control_port, &app.control_token_file) {
            (Some(p), Some(f)) => json!(server_args(p, f)),
            _ => json!(["mcp"]),
        };
        serde_json::to_string_pretty(&json!({"mcpServers": {SERVER: {"command": cli, "args": args}}})).unwrap_or_default()
    }

    fn test(app: &mut PhotocraftApp, s: &mut State) {
        let (tx, rx) = channel();
        s.test = None;
        match app.ensure_control() {
            Ok((port, token)) => {
                std::thread::spawn(move || {
                    let r = mcp_request(port, &token, "tools/list", json!({}), Duration::from_secs(60)).and_then(|v| {
                        let n = v["tools"].as_array().map_or(0, Vec::len);
                        if n > 0 { Ok(format!("MCP OK: {n} tools (this window)")) } else { Err(format!("MCP answered without tools: {v}")) }
                    });
                    let _ = tx.send(r);
                });
                s.test_rx = Some(rx);
            }
            Err(e) => s.test = Some(Err(e)),
        }
    }

    // ---------------------------------------------------------------------------------- Detection

    fn dirs_home() -> Option<PathBuf> {
        std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from)
    }

    fn cli_path() -> Option<PathBuf> {
        let exe = std::env::current_exe().ok()?;
        let name = if cfg!(windows) { "photocraft-cli.exe" } else { "photocraft-cli" };
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
        let claude = find_program(Agent::Claude);
        let claude_mcp = registered(&home.join(".claude.json"));
        out.push(match &claude {
            Some(p) => {
                Conn { name: "Claude Code", status: Status::Connected, detail: format!("CLI: {} · MCP registered: {}", p.display(), yes_no(claude_mcp)) }
            }
            None if claude_mcp.is_some() => Conn {
                name: "Claude Code",
                status: Status::Disconnected,
                detail: "Claude Code is set up, but its CLI wasn't found to send requests from here.".into(),
            },
            None => Conn { name: "Claude Code", status: Status::Unsupported, detail: "Claude Code isn't installed.".into() },
        });
        let cursor_dir = home.join(".cursor");
        let cursor_mcp = registered(&cursor_dir.join("mcp.json"));
        out.push(if let Some(p) = find_program(Agent::Cursor) {
            Conn { name: "Cursor", status: Status::Connected, detail: format!("Cursor Agent: {} · MCP registered: {}", p.display(), yes_no(cursor_mcp)) }
        } else if cursor_dir.is_dir() {
            Conn {
                name: "Cursor",
                status: if cursor_mcp == Some(true) { Status::Connected } else { Status::Disconnected },
                detail: format!(
                    "Cursor's chat reaches PhotoCraft through MCP (registered: {}). Sending requests from this panel needs the Cursor Agent CLI.",
                    yes_no(cursor_mcp)
                ),
            }
        } else {
            Conn { name: "Cursor", status: Status::Unsupported, detail: "Cursor isn't installed.".into() }
        });
        out.push(match find_program(Agent::Codex) {
            Some(p) => Conn { name: "ChatGPT", status: Status::Connected, detail: format!("Codex CLI (signed in with ChatGPT): {}", p.display()) },
            None => Conn {
                name: "ChatGPT",
                status: Status::Unsupported,
                detail: "The ChatGPT app can't reach MCP servers on this computer. Install Codex CLI to use your ChatGPT account here.".into(),
            },
        });
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
            Conn { name: "Claude Desktop", status: Status::Connected, detail: "Chat in the Claude app reaches PhotoCraft through MCP.".into() }
        } else {
            Conn {
                name: "Claude Desktop",
                status: Status::Disconnected,
                detail: "The Claude app is installed without PhotoCraft's MCP (ArtCraft Launcher adds it).".into(),
            }
        });
        out.push(match port {
            Some(p) => Conn { name: "PhotoCraft", status: Status::Connected, detail: format!("Control channel 127.0.0.1:{p}") },
            None => Conn { name: "PhotoCraft", status: Status::Disconnected, detail: "The control channel is off; Send or Turn on opens it.".into() },
        });
        out
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn claude_gets_photocrafts_server_bridged_to_this_window() {
            let token = Path::new("C:/t/photocraft.token");
            let (_, args) = agent_command(Agent::Claude, Path::new("claude.exe"), "Tách nền ảnh này", Path::new("C:/pc/photocraft-cli.exe"), 7971, token, None);
            assert_eq!(args.get(1).map(String::as_str), Some("Tách nền ảnh này"));
            let at = args.iter().position(|a| a == "--mcp-config").unwrap();
            let config: Value = serde_json::from_str(&args[at + 1]).unwrap();
            assert_eq!(
                config["mcpServers"]["photocraft"]["args"],
                json!(["mcp", "--bridge", "127.0.0.1:7971", "--control-token-file", "C:/t/photocraft.token"])
            );
            let allow = args.iter().position(|a| a == "--allowedTools").unwrap();
            assert_eq!(args[allow + 1], "mcp__photocraft");
            let (_, resumed) = agent_command(Agent::Claude, Path::new("claude.exe"), "nữa", Path::new("x"), 7971, token, Some("s1"));
            assert_eq!(resumed[..2], ["--resume".to_string(), "s1".to_string()]);
        }

        #[test]
        fn codex_gets_the_server_as_config_overrides() {
            let (_, args) = agent_command(Agent::Codex, Path::new("codex.cmd"), "hi", Path::new("C:\\pc\\photocraft-cli.exe"), 7971, Path::new("t"), None);
            assert_eq!(args[0], "exec");
            assert!(args.iter().any(|a| a.starts_with("mcp_servers.photocraft.args=[\"mcp\",\"--bridge\",\"127.0.0.1:7971\"")));
        }

        #[test]
        fn agent_output_becomes_transcript_lines() {
            let lines = parse_agent_line(
                Agent::Claude,
                r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Đã tách nền."},{"type":"tool_use","name":"mcp__photocraft__command_run","input":{}}]}}"#,
            );
            assert!(matches!(&lines[0], Line::Ai(t) if t == "Đã tách nền."));
            assert!(matches!(&lines[1], Line::Tool(t) if t == "command_run"));
            let done = parse_agent_line(Agent::Claude, r#"{"type":"result","is_error":false,"duration_ms":5000,"num_turns":2,"total_cost_usd":0.01}"#);
            assert!(matches!(&done[0], Line::Info(t) if t == "✓ · 5 s · 2 ↻ · ≈ $0.010"));
            let err = parse_agent_line(Agent::Claude, r#"{"type":"result","is_error":true,"result":"Not logged in"}"#);
            assert!(matches!(&err[0], Line::Error(t) if t == "Not logged in"));
        }

        #[test]
        fn a_spent_plan_is_told_apart_from_other_errors() {
            assert!(is_usage_limit("Claude AI usage limit reached|1792123200"));
            assert!(is_usage_limit("API Error: 429 rate_limit_error"));
            assert!(!is_usage_limit("Not logged in · Please run /login"));
        }

        #[test]
        fn the_account_is_read_without_any_credential() {
            let a = Account::parse(r#"{"loggedIn": true, "authMethod": "claude.ai", "email": "me@example.com", "subscriptionType": "max"}"#).unwrap();
            assert_eq!(a.label(), "me@example.com · Max");
            assert!(!Account::parse(r#"{"loggedIn": false}"#).unwrap().logged_in);
        }

        #[test]
        fn the_window_menu_item_is_enabled_and_listed() {
            let app = crate::PhotocraftApp::new(photocraft_engine::Session::new(), Default::default());
            assert!(crate::menus::is_enabled(&app, COMMAND), "Window › AI / MCP Command Center must not be greyed out");
            let item = crate::menus::menu_items(&app).into_iter().find(|m| m.id == COMMAND).expect("in the menus");
            assert!(item.enabled);
            assert_eq!(item.path.first().map(String::as_str), Some("Window"));
        }

        #[test]
        fn everyday_requests_run_without_ai() {
            let cmds = vec![
                ("edit.undo".to_string(), vec!["Undo".to_string(), "Hoàn tác".to_string()]),
                ("layer.new".to_string(), vec!["New Layer…".to_string()]),
                ("filter.blur.gaussianBlur".to_string(), vec!["Gaussian Blur…".to_string(), "Blur Gaussian Blur…".to_string()]),
                ("file.exit".to_string(), vec!["Exit".to_string()]),
            ];
            assert_eq!(local_command("Hoàn tác", &cmds, "layer.new"), Some(("edit.undo".into(), json!({}))));
            assert_eq!(local_command("hãy tạo layer mới!", &cmds, "layer.new").map(|c| c.0), Some("layer.new".into()));
            assert_eq!(local_command("gaussian blur", &cmds, "layer.new").map(|c| c.0), Some("filter.blur.gaussianBlur".into()));
            assert_eq!(local_command(r#"/layer.new {"name": "A"}"#, &cmds, "layer.new"), Some(("layer.new".into(), json!({"name": "A"}))));
            assert_eq!(local_command("/nope", &cmds, "layer.new"), None);
            assert_eq!(local_command("exit", &cmds, "layer.new"), None, "a name never closes the app");
            assert_eq!(local_command("Tách nền ảnh này rồi làm mờ phần còn lại cho đẹp hơn nhé", &cmds, "layer.new"), None);
        }

        #[test]
        fn saved_lines_round_trip() {
            for l in
                [Line::User("a".into()), Line::Ai("b".into()), Line::Tool("c".into()), Line::Error("d".into()), Line::Info("e".into()), Line::Limit("f".into())]
            {
                let pair = l.to_pair();
                assert_eq!(Line::from_pair(pair.clone()).to_pair(), pair);
            }
        }

        #[test]
        fn example_args_fill_the_required_fields() {
            let schema = json!({"required": ["id", "params"], "properties": {"id": {"type": "string"}, "params": {"type": "object"}}});
            let v: Value = serde_json::from_str(&example_args(&schema)).unwrap();
            assert_eq!(v, json!({"id": "", "params": {}}));
        }
    }
}
