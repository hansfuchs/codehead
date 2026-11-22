//! Settings › mcp servers. codehead's own servers (`mcp.toml`: every harness, the agents that
//! list them) and the servers each harness has on its own (union), grouped by source. A filter
//! on top; a "needs action" box (sign-in, failure, approval) above the groups. Entering the tab
//! starts a refresh in the background: the harness CLIs (`claude mcp list` checks every server,
//! up to 1 min.) and a probe of codehead's servers.

use std::collections::BTreeMap;
use std::sync::mpsc::{self, Receiver, Sender};

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap};

use crate::engine::harnesses;
use crate::engine::roster::{self, Agent};
use crate::engine::servers::{self, Server, Status};
use crate::mcplist::{self, Native};
use crate::textarea::TextArea;
use crate::theme;

/// What a row is.
#[derive(Clone, Debug, PartialEq)]
enum Key {
    Codehead(String),
    Native { harness: String, name: String },
}

#[derive(Clone, Debug)]
struct Item {
    key: Key,
    group: &'static str,
    name: String,
    /// connected, needs-auth, failed, pending, off, unknown
    state: String,
    detail: String,
    /// Why it needs action ("": 0 action).
    action: String,
}

/// Background results.
enum Msg {
    Refreshed(mcplist::Found),
    Probed,
    Done(String, bool),
}

/// The add / edit form of a codehead server.
struct EditForm {
    /// The server's name before the edit (None: a new server).
    old: Option<String>,
    kind: usize,
    name: TextArea,
    target: TextArea,
    secrets: TextArea,
    auto_allow: bool,
    field: usize,
}

const KINDS: [&str; 3] = ["stdio", "http", "sse"];

/// The agents picker: which agents get server NAME, by harness.
struct Pick {
    name: String,
    rows: Vec<(Agent, bool)>,
    cursor: usize,
}

enum Modal {
    Info { title: String, lines: Vec<String>, login: Option<(String, String)> },
    Edit(Box<EditForm>),
    Agents(Pick),
    Confirm { text: String, then: Then },
}

enum Then {
    Delete(String),
    Import { harness: String, name: String, source: String },
}

pub struct McpTab {
    filter: TextArea,
    filtering: bool,
    cursor: usize,
    offset: usize,
    servers: Vec<Server>,
    natives: Vec<Native>,
    statuses: BTreeMap<String, BTreeMap<String, Status>>,
    agents: Vec<Agent>,
    /// claude.ai connectors Claude Code ever connected (`claudeAiMcpEverConnected`).
    ever: std::collections::HashSet<String>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    /// Jobs running: CLI refresh, probe, sign-in.
    refreshing: bool,
    probing: bool,
    busy: Vec<String>,
    cli_errors: Vec<(String, String)>,
    status: String,
    error: bool,
    modal: Option<Modal>,
    /// Click targets of the last draw: (area, index into items()).
    hits: Vec<(Rect, usize)>,
    modal_hits: Vec<(Rect, usize)>,
}

fn icon(state: &str) -> (&'static str, Style) {
    let p = theme::pal();
    match state {
        "connected" => ("✔", Style::default().fg(p.ok)),
        "needs-auth" => ("⚠", Style::default().fg(p.warn)),
        "failed" => ("✘", Style::default().fg(p.danger)),
        "pending" => ("⏸", Style::default().fg(p.warn)),
        "off" => ("○", theme::muted()),
        _ => ("·", theme::muted()),
    }
}

fn harness_label(h: &str) -> &'static str {
    harnesses::get(h).map_or("harness", |x| x.label)
}

impl McpTab {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        let mut filter = TextArea::new(true, "filter servers ( / )");
        filter.style = Some(theme::text());
        let mut t = Self {
            filter,
            filtering: false,
            cursor: 0,
            offset: 0,
            servers: Vec::new(),
            natives: Vec::new(),
            statuses: BTreeMap::new(),
            agents: Vec::new(),
            ever: Default::default(),
            tx,
            rx,
            refreshing: false,
            probing: false,
            busy: Vec::new(),
            cli_errors: Vec::new(),
            status: String::new(),
            error: false,
            modal: None,
            hits: Vec::new(),
            modal_hits: Vec::new(),
        };
        t.load();
        t
    }

    /// codehead's servers, the cached statuses, the agents: from disk.
    fn load(&mut self) {
        match servers::load() {
            Ok(v) => self.servers = v,
            Err(e) => self.say(e, true),
        }
        self.statuses = servers::statuses();
        self.agents = roster::active();
        self.ever = ever_connected();
        // the cache: natives as the last refresh left them
        if self.natives.is_empty() {
            self.natives = self
                .statuses
                .iter()
                .filter(|(h, _)| *h != "codehead")
                .flat_map(|(h, m)| {
                    m.iter().filter(|(_, s)| s.source != "codehead" && !s.source.is_empty()).map(move |(n, s)| Native { harness: h.clone(), name: n.clone(), source: s.source.clone(), target: String::new(), status: s.clone() })
                })
                .collect();
        }
    }

    fn say(&mut self, text: impl Into<String>, error: bool) {
        self.status = text.into();
        self.error = error;
    }

    /// The tab came into view: refresh the harness lists and probe codehead's servers.
    pub fn enter(&mut self) {
        self.load();
        if cfg!(test) {
            return;
        }
        if !self.refreshing {
            self.refreshing = true;
            let tx = self.tx.clone();
            std::thread::spawn(move || {
                let _ = tx.send(Msg::Refreshed(mcplist::refresh()));
            });
        }
        self.probe();
    }

    fn probe(&mut self) {
        if self.probing || self.servers.is_empty() || cfg!(test) {
            return;
        }
        self.probing = true;
        let list = self.servers.clone();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            mcplist::probe_all(&list);
            let _ = tx.send(Msg::Probed);
        });
    }

    /// Background results in. true: something changed (redraw).
    pub fn poll(&mut self) -> bool {
        let mut any = false;
        while let Ok(m) = self.rx.try_recv() {
            any = true;
            match m {
                Msg::Refreshed(found) => {
                    self.refreshing = false;
                    self.natives = found.natives;
                    self.cli_errors = found.errors;
                }
                Msg::Probed => self.probing = false,
                Msg::Done(text, error) => {
                    self.busy.pop();
                    self.say(text, error);
                }
            }
            self.statuses = servers::statuses();
        }
        any
    }

    /// Jobs still running: the draw shows "updating".
    pub fn updating(&self) -> bool {
        self.refreshing || self.probing || !self.busy.is_empty()
    }

    // ---------- rows ----------

    fn codehead_item(&self, s: &Server) -> Item {
        let probe = self.statuses.get("codehead").and_then(|m| m.get(&s.name));
        // a harness that could not run it (a session's report)
        let failed: Vec<(String, String)> = self
            .statuses
            .iter()
            .filter(|(h, _)| *h != "codehead")
            .filter_map(|(h, m)| m.get(&s.name).filter(|st| st.source == "codehead" && !st.error.is_empty()).map(|st| (h.clone(), st.error.clone())))
            .collect();
        let users = self.agents.iter().filter(|a| a.mcp.contains(&s.name)).count();
        let plain = s.plain_secrets();
        let mut state = probe.map_or("unknown".to_string(), |p| p.state.clone());
        let mut action = String::new();
        if let Some((h, e)) = failed.first() {
            action = format!("{}: {e}", harness_label(h));
            if state == "connected" || state == "unknown" {
                state = "failed".into();
            }
        } else if let Some(p) = probe.filter(|p| p.state != "connected") {
            action = p.error.clone();
        }
        if !plain.is_empty() {
            action = format!("plain text secrets: {} (edit to move into the keychain)", plain.join(", "));
        }
        let tools = probe.and_then(|p| p.tools).map(|n| format!("{n} tools · ")).unwrap_or_default();
        let allow = if s.auto_allow { " · auto-allow" } else { "" };
        let who = if users == 0 { "0 agents".to_string() } else { format!("{users} agent{}", if users == 1 { "" } else { "s" }) };
        Item { key: Key::Codehead(s.name.clone()), group: "codehead · every harness", name: s.name.clone(), state, detail: format!("{tools}{who}{allow}"), action }
    }

    fn native_item(&self, n: &Native) -> Item {
        // a connector the account offers but Claude Code never connected: Claude Code's
        // "unused connectors", 0 action needed
        let unused = n.source == "claudeai" && n.status.state == "needs-auth" && !self.ever.contains(&n.name);
        if unused {
            let action = String::new();
            return Item { key: Key::Native { harness: n.harness.clone(), name: n.name.clone() }, group: UNUSED, name: n.name.clone(), state: "off".into(), detail: "not connected · l: connect".into(), action };
        }
        let group = match (n.harness.as_str(), n.source.as_str()) {
            ("claude", "claudeai") => "claude.ai connectors · Claude Code only",
            ("claude", "plugin") => "Claude Code plugins · Claude Code only",
            ("claude", _) => "Claude Code · own config",
            ("codex", _) => "Codex · own config",
            _ => "OpenCode · own config",
        };
        // the tool count: from the last session's init (Claude Code), else the list's state
        let st = self.statuses.get(&n.harness).and_then(|m| m.get(&n.name)).unwrap_or(&n.status);
        let state = n.status.state.clone();
        let detail = match state.as_str() {
            "connected" => st.tools.map(|t| format!("{t} tools")).unwrap_or_else(|| "connected".into()),
            "needs-auth" => "needs sign-in".into(),
            "pending" => "pending approval".into(),
            "off" => "disabled".into(),
            "failed" => "failed".into(),
            _ => String::new(),
        };
        let action = match state.as_str() {
            "needs-auth" => format!("needs sign-in · l: sign in through {}", harness_label(&n.harness)),
            "failed" => if n.status.error.is_empty() { "failed to connect".into() } else { n.status.error.clone() },
            "pending" => "pending approval in the project".into(),
            _ => String::new(),
        };
        Item { key: Key::Native { harness: n.harness.clone(), name: n.name.clone() }, group, name: n.name.clone(), state, detail, action }
    }

    /// Every row the filter keeps: the "needs action" rows first, then all rows by group.
    fn items(&self) -> (Vec<Item>, usize) {
        let q = self.filter.text().trim().to_lowercase();
        let keep = |i: &Item| q.is_empty() || crate::notes::fuzzy(&q, &i.name).is_some() || crate::notes::fuzzy(&q, i.group).is_some();
        let mut all: Vec<Item> = self.servers.iter().map(|s| self.codehead_item(s)).collect();
        let order = ["Claude Code · own config", "claude.ai connectors · Claude Code only", "Claude Code plugins · Claude Code only", "Codex · own config", "OpenCode · own config", UNUSED];
        let mut natives: Vec<Item> = self.natives.iter().map(|n| self.native_item(n)).collect();
        natives.sort_by_key(|i| (order.iter().position(|g| *g == i.group).unwrap_or(9), i.name.to_lowercase()));
        all.extend(natives);
        let all: Vec<Item> = all.into_iter().filter(|i| keep(i)).collect();
        let mut out: Vec<Item> = all.iter().filter(|i| !i.action.is_empty()).cloned().map(|mut i| {
            i.group = "needs action";
            i
        }).collect();
        let n = out.len();
        out.extend(all);
        (out, n)
    }

    fn current(&self) -> Option<Item> {
        self.items().0.get(self.cursor).cloned()
    }

    // ---------- keys ----------

    /// A key. false: the tab did not use it (the parent's: Up at the top).
    pub fn key(&mut self, k: KeyEvent) -> bool {
        if self.modal.is_some() {
            self.modal_key(k);
            return true;
        }
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        if self.filtering {
            match k.code {
                KeyCode::Esc => {
                    self.filter.set_text("");
                    self.filtering = false;
                }
                KeyCode::Enter | KeyCode::Down | KeyCode::Tab => self.filtering = false,
                KeyCode::Up => return false,
                _ => {
                    self.filter.key(k);
                    self.cursor = 0;
                    self.offset = 0;
                }
            }
            return true;
        }
        let n = self.items().0.len();
        match k.code {
            KeyCode::Char('/') => self.filtering = true,
            KeyCode::Char('f') if ctrl => self.filtering = true,
            KeyCode::Up if self.cursor == 0 => {
                self.filtering = true;
            }
            KeyCode::Up => self.cursor -= 1,
            KeyCode::Down => self.cursor = (self.cursor + 1).min(n.saturating_sub(1)),
            KeyCode::PageUp => self.cursor = self.cursor.saturating_sub(10),
            KeyCode::PageDown => self.cursor = (self.cursor + 10).min(n.saturating_sub(1)),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = n.saturating_sub(1),
            KeyCode::Char('a') => self.open_add(),
            KeyCode::Char('r') if ctrl || k.modifiers.is_empty() => {
                self.natives.clear();
                self.enter();
                self.say("updating: harness lists, probe", false);
            }
            KeyCode::Enter => self.open_current(),
            KeyCode::Char('l') => self.login_current(),
            KeyCode::Char('i') => self.import_current(),
            KeyCode::Char('g') => self.agents_current(),
            KeyCode::Char('t') => self.toggle_allow(),
            KeyCode::Char('d') | KeyCode::Delete => {
                if let Some(Item { key: Key::Codehead(name), .. }) = self.current() {
                    self.modal = Some(Modal::Confirm { text: format!("delete server {name}? its keychain secrets go too, its agents lose it"), then: Then::Delete(name) });
                }
            }
            KeyCode::Esc if !self.filter.text().is_empty() => self.filter.set_text(""),
            _ => {}
        }
        true
    }

    fn open_current(&mut self) {
        let Some(it) = self.current() else { return };
        match &it.key {
            Key::Codehead(name) => {
                if let Some(s) = self.servers.iter().find(|s| &s.name == name).cloned() {
                    if !it.action.is_empty() && !it.action.starts_with("plain text") {
                        self.modal = Some(self.info_for(&it));
                    } else {
                        self.modal = Some(Modal::Edit(Box::new(EditForm::of(Some(&s)))));
                    }
                }
            }
            Key::Native { .. } => self.modal = Some(self.info_for(&it)),
        }
    }

    /// What the owner can do about row IT: the error, the steps.
    fn info_for(&self, it: &Item) -> Modal {
        match &it.key {
            Key::Codehead(name) => {
                let s = self.servers.iter().find(|s| &s.name == name);
                let target = s.map(Server::target).unwrap_or_default();
                let mut lines = vec![format!("{name}: {}", it.action), String::new()];
                if it.action.contains("HTTP") || it.action.contains("SSE") {
                    lines.extend([
                        "This harness cannot run HTTP or SSE MCP servers that codehead hands it.".to_string(),
                        "Add the server in the harness itself, then sign in there:".to_string(),
                        format!("  codex mcp add {name} --url {target}"),
                        format!("  codex mcp login {name}"),
                        "Then turn it off for that harness here (g: agents).".to_string(),
                    ]);
                } else if it.state == "needs-auth" {
                    lines.extend([
                        "codehead cannot sign in to an OAuth server for a harness.".to_string(),
                        "Add it in each harness and sign in there:".to_string(),
                        format!("  claude mcp add --transport http --scope user {name} {target}"),
                        format!("  claude mcp login {name}"),
                        "Then delete it here (d): the harness has it on its own.".to_string(),
                    ]);
                } else {
                    lines.extend([
                        "Check the command or URL and its secrets (enter on the row: edit).".to_string(),
                        "Run it by hand in a terminal to see its error, or add it in the harness".to_string(),
                        "and connect it there (claude: /mcp).".to_string(),
                    ]);
                }
                Modal::Info { title: format!("{name} · cannot connect"), lines, login: None }
            }
            Key::Native { harness, name } => {
                let label = harness_label(harness);
                let login = mcplist::login_command(harness, name);
                let mut lines = vec![format!("{name} · {label} · {}", if it.action.is_empty() { it.detail.clone() } else { it.action.clone() }), String::new()];
                match it.state.as_str() {
                    "needs-auth" if login.is_some() => lines.extend([
                        "l: sign in now (the browser opens), or in a terminal:".to_string(),
                        format!("  {}", login.clone().unwrap_or_default().join(" ")),
                    ]),
                    "needs-auth" | "failed" | "pending" => lines.extend([
                        format!("codehead cannot fix this for {label}. Open {label} and connect it there:"),
                        if harness == "claude" { "  claude, then /mcp: pick the server, sign in or see its error".to_string() } else { format!("  {label}: its MCP settings") },
                        "r: refresh this list after".to_string(),
                    ]),
                    _ => lines.push("i: import into codehead (every harness gets it, secrets into the keychain).".into()),
                }
                Modal::Info { title: name.to_string(), lines, login: login.filter(|_| it.state == "needs-auth").map(|_| (harness.clone(), name.clone())) }
            }
        }
    }

    fn login_current(&mut self) {
        let Some(Item { key: Key::Native { harness, name }, .. }) = self.current() else {
            self.say("l: sign in works on a harness's own server", true);
            return;
        };
        self.login(harness, name);
    }

    fn login(&mut self, harness: String, name: String) {
        if mcplist::login_command(&harness, &name).is_none() {
            self.modal = Some(Modal::Info {
                title: format!("{name} · sign in"),
                lines: vec![format!("{} has 0 sign-in command codehead can run.", harness_label(&harness)), format!("Open {} and sign in to {name} there.", harness_label(&harness))],
                login: None,
            });
            return;
        }
        self.busy.push(name.clone());
        self.say(format!("signing in to {name}: the browser opens"), false);
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let r = mcplist::login(&harness, &name);
            let msg = match r {
                Ok(()) => (format!("{name}: signed in · r: refresh"), false),
                Err(e) => (format!("{name}: {e}"), true),
            };
            let _ = tx.send(Msg::Done(msg.0, msg.1));
        });
    }

    fn import_current(&mut self) {
        let Some(it) = self.current() else { return };
        let Key::Native { harness, name } = it.key else {
            self.say("i: import works on a harness's own server", true);
            return;
        };
        let source = self.natives.iter().find(|n| n.harness == harness && n.name == name).map(|n| n.source.clone()).unwrap_or_default();
        if harness == "claude" && (source == "claudeai" || source == "plugin") {
            self.say("connectors and plugins stay with Claude Code", true);
            return;
        }
        self.modal = Some(Modal::Confirm {
            text: format!("import {name} from {}? codehead runs it for every harness, its secrets go into the keychain, it leaves {}'s config", harness_label(&harness), harness_label(&harness)),
            then: Then::Import { harness, name, source },
        });
    }

    fn agents_current(&mut self) {
        if let Some(Item { key: Key::Codehead(name), .. }) = self.current() {
            self.open_agents(&name);
        } else {
            self.say("g: agents work on a codehead server", true);
        }
    }

    fn open_agents(&mut self, name: &str) {
        self.agents = roster::active();
        let mut rows: Vec<(Agent, bool)> = self.agents.iter().map(|a| (a.clone(), a.mcp.iter().any(|m| m == name))).collect();
        rows.sort_by(|a, b| (harness_order(&a.0.harness), &a.0.name).cmp(&(harness_order(&b.0.harness), &b.0.name)));
        self.modal = Some(Modal::Agents(Pick { name: name.to_string(), rows, cursor: 0 }));
    }

    fn toggle_allow(&mut self) {
        let Some(Item { key: Key::Codehead(name), .. }) = self.current() else { return };
        let mut all = self.servers.clone();
        if let Some(s) = all.iter_mut().find(|s| s.name == name) {
            s.auto_allow = !s.auto_allow;
            let on = s.auto_allow;
            match servers::save(&all) {
                Ok(()) => {
                    self.servers = all;
                    self.say(format!("{name}: {}", if on { "auto-allow on: its tools run without an ask" } else { "auto-allow off: the agent's mode decides" }), false);
                }
                Err(e) => self.say(e, true),
            }
        }
    }

    fn open_add(&mut self) {
        self.modal = Some(Modal::Edit(Box::new(EditForm::of(None))));
    }

    fn modal_key(&mut self, k: KeyEvent) {
        let Some(m) = self.modal.take() else { return };
        self.modal = match m {
            Modal::Info { title, lines, login } => match k.code {
                KeyCode::Char('l') if login.is_some() => {
                    let (h, n) = login.unwrap_or_default();
                    self.login(h, n);
                    None
                }
                KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => None,
                _ => Some(Modal::Info { title, lines, login }),
            },
            Modal::Confirm { text, then } => match k.code {
                KeyCode::Char('y') | KeyCode::Enter => {
                    self.run(then);
                    self.modal.take()
                }
                KeyCode::Esc | KeyCode::Char('n') => None,
                _ => Some(Modal::Confirm { text, then }),
            },
            Modal::Agents(mut p) => match k.code {
                KeyCode::Esc => None,
                KeyCode::Up => {
                    p.cursor = p.cursor.saturating_sub(1);
                    Some(Modal::Agents(p))
                }
                KeyCode::Down => {
                    p.cursor = (p.cursor + 1).min(p.rows.len().saturating_sub(1));
                    Some(Modal::Agents(p))
                }
                KeyCode::Char(' ') => {
                    if let Some(r) = p.rows.get_mut(p.cursor) {
                        r.1 = !r.1;
                    }
                    Some(Modal::Agents(p))
                }
                // every agent of the harness of the cursor row: on, or off when all are on
                KeyCode::Char('h') => {
                    if let Some(h) = p.rows.get(p.cursor).map(|r| r.0.harness.clone()) {
                        let all = p.rows.iter().filter(|r| r.0.harness == h).all(|r| r.1);
                        for r in p.rows.iter_mut().filter(|r| r.0.harness == h) {
                            r.1 = !all;
                        }
                    }
                    Some(Modal::Agents(p))
                }
                KeyCode::Enter => {
                    self.save_agents(&p);
                    None
                }
                _ => Some(Modal::Agents(p)),
            },
            Modal::Edit(mut e) => {
                let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
                match k.code {
                    KeyCode::Esc => None,
                    KeyCode::Char('s') if ctrl => self.save_edit(e),
                    KeyCode::Enter if e.field == 4 => self.save_edit(e),
                    KeyCode::Tab | KeyCode::Down if !(e.field == 3 && k.code == KeyCode::Down && !e.secrets_at_end()) => {
                        e.field = (e.field + 1).min(4);
                        Some(Modal::Edit(e))
                    }
                    KeyCode::BackTab | KeyCode::Up if !(e.field == 3 && k.code == KeyCode::Up && !e.secrets_at_start()) => {
                        e.field = e.field.saturating_sub(1);
                        Some(Modal::Edit(e))
                    }
                    KeyCode::Left | KeyCode::Right if e.field == 1 => {
                        e.kind = if k.code == KeyCode::Right { (e.kind + 1) % 3 } else { (e.kind + 2) % 3 };
                        e.target.placeholder = EditForm::target_hint(e.kind).into();
                        Some(Modal::Edit(e))
                    }
                    KeyCode::Char(' ') if e.field == 4 => {
                        e.auto_allow = !e.auto_allow;
                        Some(Modal::Edit(e))
                    }
                    KeyCode::Enter if e.field < 3 => {
                        e.field += 1;
                        Some(Modal::Edit(e))
                    }
                    _ => {
                        match e.field {
                            0 => {
                                e.name.key(k);
                            }
                            2 => {
                                e.target.key(k);
                            }
                            3 => {
                                e.secrets.key(k);
                            }
                            _ => {}
                        }
                        Some(Modal::Edit(e))
                    }
                }
            }
        };
    }

    fn run(&mut self, then: Then) {
        match then {
            Then::Delete(name) => {
                let mut all = self.servers.clone();
                if let Some(s) = all.iter().find(|s| s.name == name) {
                    for v in s.env.values().chain(s.headers.values()) {
                        servers::forget_secret(v);
                    }
                }
                all.retain(|s| s.name != name);
                if let Err(e) = servers::save(&all) {
                    return self.say(e, true);
                }
                for mut a in roster::active().into_iter().filter(|a| a.mcp.contains(&name)) {
                    a.mcp.retain(|m| *m != name);
                    let _ = roster::save(&a);
                }
                self.servers = all;
                self.agents = roster::active();
                self.say(format!("{name}: deleted"), false);
            }
            Then::Import { harness, name, source } => {
                let mut s = match mcplist::import(&harness, &name) {
                    Ok(s) => s,
                    Err(e) => return self.say(e, true),
                };
                // a name codehead cannot use as a server name: its safe form
                if !servers::valid_name(&s.name) {
                    s.name = s.name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' }).collect();
                }
                if self.servers.iter().any(|x| x.name == s.name) {
                    return self.say(format!("{}: codehead has a server with this name", s.name), true);
                }
                if let Err(e) = secure(&mut s, None) {
                    return self.say(e, true);
                }
                let mut all = self.servers.clone();
                all.push(s.clone());
                if let Err(e) = servers::save(&all) {
                    return self.say(e, true);
                }
                self.servers = all;
                let gone = mcplist::remove(&harness, &name, &source);
                self.natives.retain(|n| !(n.harness == harness && n.name == name));
                self.say(
                    match gone {
                        Ok(()) => format!("{}: imported, gone from {}", s.name, harness_label(&harness)),
                        Err(e) => format!("{}: imported; {} still has it ({e})", s.name, harness_label(&harness)),
                    },
                    false,
                );
                self.probe();
                self.open_agents(&s.name);
            }
        }
    }

    /// The edit form saved: the server written (secrets into the keychain), then the agents
    /// picker for a new server. Err: the form stays, the error shows.
    fn save_edit(&mut self, e: Box<EditForm>) -> Option<Modal> {
        let old = e.old.as_ref().and_then(|n| self.servers.iter().find(|s| &s.name == n)).cloned();
        let mut s = match e.server() {
            Ok(s) => s,
            Err(why) => {
                self.say(why, true);
                return Some(Modal::Edit(e));
            }
        };
        if let Err(why) = s.check() {
            self.say(why, true);
            return Some(Modal::Edit(e));
        }
        if e.old.as_deref() != Some(s.name.as_str()) && self.servers.iter().any(|x| x.name == s.name) {
            self.say(format!("{}: codehead has a server with this name", s.name), true);
            return Some(Modal::Edit(e));
        }
        if let Err(why) = secure(&mut s, old.as_ref()) {
            self.say(why, true);
            return Some(Modal::Edit(e));
        }
        let mut all: Vec<Server> = self.servers.iter().filter(|x| Some(&x.name) != e.old.as_ref()).cloned().collect();
        all.push(s.clone());
        all.sort_by(|a, b| a.name.cmp(&b.name));
        if let Err(why) = servers::save(&all) {
            self.say(why, true);
            return Some(Modal::Edit(e));
        }
        // a rename: the agents follow
        if let Some(o) = &e.old
            && *o != s.name
        {
            for mut a in roster::active().into_iter().filter(|a| a.mcp.contains(o)) {
                for m in a.mcp.iter_mut().filter(|m| *m == o) {
                    *m = s.name.clone();
                }
                let _ = roster::save(&a);
            }
        }
        self.servers = all;
        self.say(format!("{}: saved · agents start it at their next message", s.name), false);
        self.probe();
        if e.old.is_none() {
            self.open_agents(&s.name);
            return self.modal.take();
        }
        None
    }

    fn save_agents(&mut self, p: &Pick) {
        let mut n = 0;
        for (a, on) in &p.rows {
            let Some(mut cur) = roster::get(&a.name) else { continue };
            let has = cur.mcp.contains(&p.name);
            if has == *on {
                n += usize::from(*on);
                continue;
            }
            if *on {
                cur.mcp.push(p.name.clone());
                n += 1;
            } else {
                cur.mcp.retain(|m| *m != p.name);
            }
            if let Err(e) = roster::save(&cur) {
                return self.say(e, true);
            }
        }
        self.agents = roster::active();
        self.say(format!("{}: {n} agent{} · they start it at their next message", p.name, if n == 1 { "" } else { "s" }), false);
    }

    // ---------- mouse ----------

    pub fn mouse(&mut self, m: MouseEvent) {
        let at = Position { x: m.column, y: m.row };
        // the edit form: a click on a text field focuses it; the field takes its mouse
        // (cursor, selection + copy)
        if let Some(Modal::Edit(e)) = &mut self.modal {
            let down = matches!(m.kind, MouseEventKind::Down(_));
            for (i, ta) in [(0, &mut e.name), (2, &mut e.target), (3, &mut e.secrets)] {
                if (e.field == i || (down && ta.area().contains(at))) && ta.mouse(m) {
                    ta.copied = None;
                    e.field = i;
                    return;
                }
            }
            return;
        }
        // the filter: its mouse while it has the keys or a drag runs in it
        if self.modal.is_none() && self.filtering && !matches!(m.kind, MouseEventKind::Down(_)) && self.filter.mouse(m) {
            self.filter.copied = None;
            return;
        }
        match m.kind {
            MouseEventKind::ScrollUp => self.cursor = self.cursor.saturating_sub(3),
            MouseEventKind::ScrollDown => self.cursor = (self.cursor + 3).min(self.items().0.len().saturating_sub(1)),
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(Modal::Agents(p)) = &mut self.modal {
                    if let Some(&(_, i)) = self.modal_hits.iter().find(|(r, _)| r.contains(at)) {
                        p.cursor = i;
                        if let Some(r) = p.rows.get_mut(i) {
                            r.1 = !r.1;
                        }
                    }
                    return;
                }
                if self.modal.is_some() {
                    return;
                }
                if self.filter.area().contains(at) {
                    self.filtering = true;
                    self.filter.mouse(m);
                    return;
                }
                if let Some(&(_, i)) = self.hits.iter().find(|(r, _)| r.contains(at)) {
                    self.filtering = false;
                    if self.cursor == i {
                        self.open_current();
                    }
                    self.cursor = i;
                }
            }
            _ => {}
        }
    }

    pub fn paste(&mut self, text: &str) {
        match &mut self.modal {
            Some(Modal::Edit(e)) => match e.field {
                0 => e.name.insert(&text.replace(['\n', '\r'], "")),
                2 => e.target.insert(&text.replace(['\n', '\r'], " ")),
                3 => e.secrets.insert(text),
                _ => {}
            },
            None if self.filtering => self.filter.insert(&text.replace(['\n', '\r'], "")),
            _ => {}
        }
    }

    pub fn help(&self) -> &'static [(&'static str, &'static str)] {
        match &self.modal {
            Some(Modal::Edit(_)) => &[("tab ↑↓", "field"), ("←→", "type"), ("space", "auto-allow"), ("ctrl+s", "save"), ("esc", "cancel")],
            Some(Modal::Agents(_)) => &[("↑↓", "move"), ("space", "agent"), ("h", "whole harness"), ("enter", "save"), ("esc", "cancel")],
            Some(Modal::Confirm { .. }) => &[("y enter", "yes"), ("n esc", "no")],
            Some(Modal::Info { .. }) => &[("l", "sign in"), ("enter esc", "close")],
            None if self.filtering => &[("type", "filter"), ("enter ↓", "list"), ("esc", "clear")],
            None => &[("/", "filter"), ("a", "add"), ("enter", "edit / details"), ("g", "agents"), ("l", "sign in"), ("i", "import"), ("t", "auto-allow"), ("d", "delete"), ("r", "refresh")],
        }
    }

    // ---------- draw ----------

    pub fn draw(&mut self, f: &mut Frame, area: Rect, active: bool) {
        self.hits.clear();
        self.modal_hits.clear();
        self.filter.hide();
        if area.height < 6 || area.width < 20 {
            return;
        }
        // the filter row
        let fr = Rect { height: 1, ..area };
        f.render_widget(Paragraph::new("").style(theme::panel()), fr);
        self.filter.render(f, Rect { x: fr.x + 1, width: fr.width.saturating_sub(2), ..fr }, active && self.filtering && self.modal.is_none());
        let (items, actions) = self.items();
        if self.cursor >= items.len() {
            self.cursor = items.len().saturating_sub(1);
        }
        // the state line: updating, counts, the CLI errors
        let mut head = vec![];
        if self.updating() {
            head.push(Span::styled(format!("updating{} ", if self.refreshing { ": claude mcp list checks every server (up to 1 min.)" } else { "" }), theme::text().fg(theme::pal().warn)));
        }
        head.push(Span::styled(format!("{} codehead · {} harness servers", self.servers.len(), self.natives.len()), theme::muted()));
        for (h, e) in &self.cli_errors {
            head.push(Span::styled(format!(" · {h}: {e}"), theme::text().fg(theme::pal().danger)));
        }
        f.render_widget(Paragraph::new(Line::from(head)), Rect { y: area.y + 1, height: 1, ..area });
        let body = Rect { y: area.y + 3, height: area.height.saturating_sub(4), ..area };
        // the needs-action box: max. 6 rows
        let mut y = body.y;
        let label_w = (body.width / 2).min(44) as usize;
        if actions > 0 {
            let shown = actions.min(6);
            let h = shown as u16 + 2;
            let r = Rect { y, height: h.min(body.height), ..body };
            let block = Block::new()
                .borders(Borders::ALL)
                .border_type(BorderType::Plain)
                .border_style(Style::default().fg(theme::pal().warn))
                .title(Span::styled(format!(" needs action · {actions} "), theme::text().fg(theme::pal().warn).add_modifier(Modifier::BOLD)));
            let inner = block.inner(r);
            f.render_widget(block, r);
            let off = if self.cursor < actions { self.cursor.saturating_sub(shown - 1) } else { 0 };
            for (j, it) in items.iter().enumerate().take(actions).skip(off).take(shown) {
                let row = Rect { y: inner.y + (j - off) as u16, height: 1, ..inner };
                self.row(f, row, it, &it.action, j == self.cursor && active && !self.filtering, label_w);
                self.hits.push((row, j));
            }
            y += h + 1;
        }
        // the groups
        let list = Rect { y, height: body.bottom().saturating_sub(y), ..body };
        let mut lines: Vec<Option<usize>> = Vec::new();
        let mut heads: Vec<&str> = Vec::new();
        let mut last = "";
        for (j, it) in items.iter().enumerate().skip(actions) {
            if it.group != last {
                if !lines.is_empty() {
                    lines.push(None);
                    heads.push("");
                }
                lines.push(None);
                heads.push(it.group);
                last = it.group;
            }
            lines.push(Some(j));
            heads.push("");
        }
        if items.len() == actions {
            let empty = if self.filter.text().trim().is_empty() { "0 servers · a: add one · r: refresh the harness lists" } else { "0 servers match the filter" };
            f.render_widget(Paragraph::new(Line::styled(empty, theme::muted())), Rect { height: 1, ..list });
        }
        let view = list.height as usize;
        if let Some(at) = lines.iter().position(|l| *l == Some(self.cursor)) {
            if at < self.offset {
                self.offset = at.saturating_sub(1);
            }
            if at >= self.offset + view {
                self.offset = at + 1 - view;
            }
        }
        self.offset = self.offset.min(lines.len().saturating_sub(view));
        for (k, line) in lines.iter().enumerate().skip(self.offset).take(view) {
            let row = Rect { y: list.y + (k - self.offset) as u16, height: 1, ..list };
            match line {
                None if !heads[k].is_empty() => {
                    let title = heads[k];
                    let rule = (row.width as usize).saturating_sub(title.chars().count() + 1);
                    f.render_widget(Paragraph::new(Line::from(vec![Span::styled(title.to_string(), theme::accent_bold()), Span::styled(format!(" {}", "─".repeat(rule)), theme::line())])), row);
                }
                None => {}
                Some(j) => {
                    let it = &items[*j];
                    let r = Rect { x: row.x + 2, width: row.width.saturating_sub(2), ..row };
                    self.row(f, r, it, &it.detail, *j == self.cursor && active && !self.filtering, label_w);
                    self.hits.push((r, *j));
                }
            }
        }
        f.render_widget(
            Paragraph::new(Line::styled(self.status.clone(), if self.error { theme::text().fg(theme::pal().danger) } else { theme::muted() })),
            Rect { y: area.bottom() - 1, height: 1, ..area },
        );
        if self.modal.is_some() {
            self.draw_modal(f, area);
        }
    }

    fn row(&self, f: &mut Frame, r: Rect, it: &Item, detail: &str, on: bool, label_w: usize) {
        let (mark, st) = icon(&it.state);
        let base = if on { theme::pill() } else { theme::text() };
        let name: String = if it.name.chars().count() > label_w { it.name.chars().take(label_w - 1).chain(['…']).collect() } else { it.name.clone() };
        let pad = label_w.saturating_sub(name.chars().count());
        let room = (r.width as usize).saturating_sub(label_w + 5);
        let detail: String = detail.chars().take(room).collect();
        let line = Line::from(vec![
            Span::styled(format!(" {mark} "), if on { base } else { st }),
            Span::styled(format!("{name}{} ", " ".repeat(pad)), base),
            Span::styled(detail, if on { base } else { theme::muted() }),
        ]);
        f.render_widget(Paragraph::new(line).style(base), r);
    }

    fn draw_modal(&mut self, f: &mut Frame, area: Rect) {
        let w = area.width.saturating_sub(8).min(90);
        let place = |h: u16| Rect { x: area.x + (area.width - w) / 2, y: area.y + 2, width: w, height: h.min(area.height.saturating_sub(2)) };
        let boxed = |f: &mut Frame, r: Rect, title: &str| -> Rect {
            f.render_widget(Clear, r);
            let block = Block::new()
                .borders(Borders::ALL)
                .border_type(BorderType::Plain)
                .border_style(theme::accent())
                .title(Span::styled(format!(" {title} "), theme::accent_bold()))
                .style(Style::default().bg(theme::pal().on_accent).fg(theme::pal().text));
            let inner = block.inner(r);
            f.render_widget(block, r);
            Rect { x: inner.x + 1, width: inner.width.saturating_sub(2), ..inner }
        };
        match self.modal.as_mut() {
            Some(Modal::Info { title, lines, login }) => {
                let r = place(lines.len() as u16 + 4);
                let inner = boxed(f, r, title);
                let mut text: Vec<Line> = lines.iter().map(|l| if l.starts_with("  ") { Line::styled(l.clone(), theme::accent()) } else { Line::raw(l.clone()) }).collect();
                text.push(Line::raw(""));
                text.push(Line::styled(if login.is_some() { "l: sign in · enter / esc: close" } else { "enter / esc: close" }, theme::muted()));
                f.render_widget(Paragraph::new(text).wrap(Wrap { trim: false }), inner);
            }
            Some(Modal::Confirm { text, .. }) => {
                let r = place(6);
                let inner = boxed(f, r, "confirm");
                f.render_widget(Paragraph::new(vec![Line::raw(text.clone()), Line::raw(""), Line::styled("y / enter: yes · n / esc: no", theme::muted())]).wrap(Wrap { trim: false }), inner);
            }
            Some(Modal::Agents(p)) => {
                let n = p.rows.len();
                let r = place(n as u16 + 6);
                let inner = boxed(f, r, &format!("{} · which agents get it", p.name));
                let mut y = inner.y;
                let mut last = "";
                let view = inner.height.saturating_sub(1) as usize;
                let off = p.cursor.saturating_sub(view.saturating_sub(4));
                let mut hits = Vec::new();
                for (i, (a, on)) in p.rows.iter().enumerate().skip(off) {
                    if y + 1 >= inner.bottom() {
                        break;
                    }
                    if a.harness != last {
                        let all = p.rows.iter().filter(|r| r.0.harness == a.harness).all(|r| r.1);
                        f.render_widget(
                            Paragraph::new(Line::from(vec![Span::styled(harness_label(&a.harness), theme::accent_bold()), Span::styled(if all { " · all on · h: all off" } else { " · h: all on" }, theme::muted())])),
                            Rect { y, height: 1, ..inner },
                        );
                        y += 1;
                        last = &a.harness;
                    }
                    let row = Rect { y, height: 1, x: inner.x + 2, width: inner.width.saturating_sub(2) };
                    let hl = i == p.cursor;
                    let st = if hl { theme::pill() } else { theme::text() };
                    f.render_widget(Paragraph::new(Line::styled(format!("{} {}", if *on { "☑" } else { "☐" }, a.name), st)).style(st), row);
                    hits.push((row, i));
                    y += 1;
                }
                if n == 0 {
                    f.render_widget(Paragraph::new(Line::styled("0 agents · make one in the agents tab", theme::muted())), Rect { height: 1, ..inner });
                }
                f.render_widget(Paragraph::new(Line::styled("space: agent · h: harness · enter: save · esc: cancel", theme::muted())), Rect { y: inner.bottom() - 1, height: 1, ..inner });
                self.modal_hits = hits;
            }
            Some(Modal::Edit(e)) => {
                let r = place(16);
                let title = if e.old.is_some() { "edit MCP server" } else { "add MCP server" };
                let inner = boxed(f, r, title);
                let lw = 10u16;
                let rows = [("name", 1u16), ("type", 1), (if e.kind == 0 { "command" } else { "URL" }, 1), (if e.kind == 0 { "env" } else { "headers" }, 5), ("auto-allow", 1)];
                let mut y = inner.y;
                for (i, (label, h)) in rows.iter().enumerate() {
                    let focused = e.field == i;
                    let lr = Rect { y, height: 1, width: lw, ..inner };
                    f.render_widget(Paragraph::new(Line::styled(*label, if focused { theme::accent_bold() } else { theme::muted() })), lr);
                    let ir = Rect { x: inner.x + lw, y, width: inner.width.saturating_sub(lw), height: *h };
                    match i {
                        0 => {
                            f.render_widget(Paragraph::new("").style(theme::panel()), ir);
                            e.name.render(f, ir, focused);
                        }
                        1 => {
                            let spans: Vec<Span> = KINDS.iter().enumerate().map(|(k, n)| Span::styled(format!(" {n} "), if k == e.kind { if focused { theme::pill() } else { theme::accent_bold() } } else { theme::muted() })).collect();
                            f.render_widget(Paragraph::new(Line::from(spans)), ir);
                        }
                        2 => {
                            f.render_widget(Paragraph::new("").style(theme::panel()), ir);
                            e.target.render(f, ir, focused);
                        }
                        3 => {
                            f.render_widget(Paragraph::new("").style(theme::panel()), ir);
                            e.secrets.render(f, ir, focused);
                        }
                        _ => {
                            let st = if focused { theme::pill() } else { theme::text() };
                            f.render_widget(Paragraph::new(Line::styled(format!(" {} its tools run without an ask ", if e.auto_allow { "☑" } else { "☐" }), st)), ir);
                        }
                    }
                    y += h + 1;
                }
                let hint = "secrets: 1 KEY=value per line → the OS keychain on save; KEY=${VAR}: an env var of codehead";
                f.render_widget(Paragraph::new(Line::styled(hint, theme::muted())).wrap(Wrap { trim: false }), Rect { y: inner.bottom().saturating_sub(2), height: 2, ..inner });
            }
            None => {}
        }
    }
}

impl Default for McpTab {
    fn default() -> Self {
        Self::new()
    }
}

/// The group of connectors Claude Code never connected (its "unused connectors").
const UNUSED: &str = "claude.ai connectors · unused";

/// `claudeAiMcpEverConnected` of ~/.claude.json: connectors that were connected once.
fn ever_connected() -> std::collections::HashSet<String> {
    if cfg!(test) {
        return Default::default();
    }
    read_ever(&crate::config::home().join(".claude.json"))
}

fn read_ever(path: &std::path::Path) -> std::collections::HashSet<String> {
    let v: serde_json::Value = std::fs::read(path).ok().and_then(|t| serde_json::from_slice(&t).ok()).unwrap_or_default();
    v["claudeAiMcpEverConnected"].as_array().into_iter().flatten().filter_map(|s| s.as_str().map(String::from)).collect()
}

fn harness_order(h: &str) -> usize {
    harnesses::ALL.iter().position(|x| x.name == h).unwrap_or(99)
}

/// The env / header values of S in the keychain: plain values move in (OLD's entries a new value
/// replaces go). 0 keychain: Err, the owner writes `${VAR}` instead.
fn secure(s: &mut Server, old: Option<&Server>) -> Result<(), String> {
    let name = s.name.clone();
    let olds: Vec<String> = old.map(|o| o.env.values().chain(o.headers.values()).cloned().collect()).unwrap_or_default();
    for map in [&mut s.env, &mut s.headers] {
        for (k, v) in map.iter_mut() {
            if servers::is_ref(v) {
                continue;
            }
            *v = servers::store_secret(&name, k, v).map_err(|e| format!("{e} · write {k}=${{VAR}} and set VAR in codehead's environment"))?;
        }
    }
    let now: Vec<&String> = s.env.values().chain(s.headers.values()).collect();
    for o in olds.iter().filter(|o| !now.contains(o)) {
        servers::forget_secret(o);
    }
    Ok(())
}

impl EditForm {
    fn target_hint(kind: usize) -> &'static str {
        if kind == 0 { "npx -y @modelcontextprotocol/server-filesystem ~/x" } else { "https://mcp.example.com/mcp" }
    }

    fn of(s: Option<&Server>) -> EditForm {
        let input = |text: &str, hint: &str, single: bool| {
            let mut t = TextArea::new(single, hint);
            t.set_text(text);
            t.style = Some(theme::text());
            t
        };
        let kind = s.map_or(0, |s| KINDS.iter().position(|k| *k == s.kind).unwrap_or(0));
        let target = s.map(Server::target).unwrap_or_default();
        let pairs = s.map(|s| if s.kind == "stdio" { &s.env } else { &s.headers }).map(|m| m.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("\n")).unwrap_or_default();
        EditForm {
            old: s.map(|s| s.name.clone()),
            kind,
            name: input(s.map_or("", |s| s.name.as_str()), "linear", true),
            target: input(&target, Self::target_hint(kind), true),
            secrets: input(&pairs, "API_KEY=…", false),
            auto_allow: s.is_none_or(|s| s.auto_allow),
            field: 0,
        }
    }

    fn secrets_at_end(&self) -> bool {
        let (row, _) = self.secrets.cursor();
        row + 1 >= self.secrets.text().lines().count().max(1)
    }

    fn secrets_at_start(&self) -> bool {
        self.secrets.cursor().0 == 0
    }

    /// The form as a server (secrets as typed: `secure` moves them into the keychain).
    fn server(&self) -> Result<Server, String> {
        let kind = KINDS[self.kind].to_string();
        let target = self.target.text().trim().to_string();
        let mut map = BTreeMap::new();
        for l in self.secrets.text().lines().map(str::trim).filter(|l| !l.is_empty()) {
            let (k, v) = l.split_once('=').ok_or_else(|| format!("{l:?}: KEY=value"))?;
            map.insert(k.trim().to_string(), v.trim().to_string());
        }
        let mut s = Server { name: self.name.text().trim().to_string(), kind, auto_allow: self.auto_allow, ..Default::default() };
        if s.kind == "stdio" {
            let mut parts = harnesses::split(&target).into_iter();
            s.command = parts.next();
            s.args = parts.collect();
            s.env = map;
        } else {
            s.url = (!target.is_empty()).then_some(target);
            s.headers = map;
        }
        Ok(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab(servers: Vec<Server>, natives: Vec<Native>) -> McpTab {
        let mut t = McpTab::new();
        t.servers = servers;
        t.natives = natives;
        t.statuses = BTreeMap::new();
        t.agents = Vec::new();
        t
    }

    fn native(h: &str, name: &str, source: &str, state: &str) -> Native {
        Native { harness: h.into(), name: name.into(), source: source.into(), target: String::new(), status: Status { state: state.into(), ..Default::default() } }
    }

    #[test]
    fn rows_group_by_source_with_needs_action_first() {
        let s = Server { name: "files".into(), kind: "stdio".into(), command: Some("npx".into()), auto_allow: true, ..Default::default() };
        let t = tab(vec![s], vec![native("claude", "claude.ai Linear", "claudeai", "connected"), native("claude", "plugin:general:figma", "plugin", "needs-auth"), native("claude", "local-db", "user", "failed")]);
        let (items, actions) = t.items();
        assert_eq!(actions, 2, "{items:#?}");
        assert_eq!(items[0].group, "needs action");
        let groups: Vec<&str> = items[actions..].iter().map(|i| i.group).collect();
        assert_eq!(groups, ["codehead · every harness", "Claude Code · own config", "claude.ai connectors · Claude Code only", "Claude Code plugins · Claude Code only"]);
        assert!(items[1].action.contains("sign") || items[0].action.contains("sign"));
    }

    #[test]
    fn connectors_never_connected_go_to_unused_not_needs_action() {
        let mut t = tab(Vec::new(), vec![native("claude", "claude.ai Linear", "claudeai", "needs-auth"), native("claude", "claude.ai Box", "claudeai", "needs-auth")]);
        t.ever.insert("claude.ai Linear".into());
        let (items, actions) = t.items();
        assert_eq!(actions, 1, "{items:#?}");
        assert_eq!(items[0].name, "claude.ai Linear");
        let box_row = items.iter().find(|i| i.name == "claude.ai Box").unwrap();
        assert_eq!((box_row.group, box_row.action.as_str()), (UNUSED, ""));
        assert_eq!(items.last().unwrap().group, UNUSED);
    }

    #[test]
    fn the_filter_keeps_fuzzy_matches_of_names_and_groups() {
        let mut t = tab(Vec::new(), vec![native("claude", "claude.ai Linear", "claudeai", "connected"), native("claude", "claude.ai Slack", "claudeai", "connected"), native("codex", "docs", "config", "unknown")]);
        t.filter.set_text("slk");
        assert_eq!(t.items().0.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(), ["claude.ai Slack"]);
        t.filter.set_text("codex");
        assert_eq!(t.items().0.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(), ["docs"]);
    }

    #[test]
    fn a_harness_failure_marks_a_codehead_server() {
        let s = Server { name: "web".into(), kind: "http".into(), url: Some("https://w/mcp".into()), auto_allow: true, ..Default::default() };
        let mut t = tab(vec![s], Vec::new());
        let mut m = BTreeMap::new();
        m.insert("web".to_string(), Status { state: "failed".into(), error: "harness: 0 HTTP MCP support (ACP mcpCapabilities.http)".into(), source: "codehead".into(), ..Default::default() });
        t.statuses.insert("codex".into(), m);
        let (items, actions) = t.items();
        assert_eq!(actions, 1);
        assert!(items[0].action.starts_with("Codex: harness: 0 HTTP"), "{}", items[0].action);
        let Modal::Info { lines, .. } = t.info_for(&items[0]) else { panic!() };
        assert!(lines.iter().any(|l| l.contains("codex mcp add web --url https://w/mcp")), "{lines:#?}");
    }

    #[test]
    fn the_edit_form_builds_stdio_and_http_servers() {
        let mut e = EditForm::of(None);
        e.name.set_text("fs");
        e.target.set_text("npx -y '@m/server fs' /tmp");
        e.secrets.set_text("TOKEN=abc\n\nOTHER=${X}\n");
        let s = e.server().unwrap();
        assert_eq!((s.command.as_deref(), s.args.clone()), (Some("npx"), vec!["-y".to_string(), "@m/server fs".into(), "/tmp".into()]));
        assert_eq!(s.env.len(), 2);
        assert!(s.check().is_ok());
        e.kind = 1;
        e.target.set_text("https://x/mcp");
        e.secrets.set_text("Authorization=${TOK}");
        let h = e.server().unwrap();
        assert_eq!((h.kind.as_str(), h.url.as_deref(), h.headers.len()), ("http", Some("https://x/mcp"), 1));
        e.secrets.set_text("no-equals");
        assert!(e.server().is_err());
        // an edit shows references, never resolved values
        let back = EditForm::of(Some(&h));
        assert_eq!(back.secrets.text(), "Authorization=${TOK}");
    }

    /// The tab with this machine's harness lists: `cargo test live_lists -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn live_lists_render() {
        let found = mcplist::refresh();
        let mut t = tab(Vec::new(), found.natives);
        t.cli_errors = found.errors;
        t.ever = read_ever(&crate::config::home().join(".claude.json"));
        let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 50)).unwrap();
        term.draw(|f| t.draw(f, f.area(), true)).unwrap();
        let b = term.backend().buffer().clone();
        let screen: Vec<String> = (0..50).map(|y| (0..120).map(|x| b[(x, y)].symbol()).collect::<String>().trim_end().to_string()).collect();
        println!("{}", screen.join("\n"));
        assert!(screen.iter().any(|l| l.contains("needs action")));
    }

    #[test]
    fn env_references_need_no_keychain() {
        let mut s = Server { name: "x".into(), kind: "stdio".into(), command: Some("a".into()), auto_allow: true, ..Default::default() };
        s.env.insert("K".into(), "${VAR}".into());
        assert!(secure(&mut s, None).is_ok());
        assert_eq!(s.env["K"], "${VAR}");
    }
}
