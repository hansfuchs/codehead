//! Settings › mcp servers. codehead's own servers (`mcp.toml`: every harness, the agents that
//! list them) and the servers each harness has on its own (union), grouped by source. A filter
//! on top; a "needs action" box (sign-in, failure, approval) above the groups. Entering the tab
//! starts a refresh in the background: the harness CLIs (`claude mcp list` checks every server,
//! up to 1 min.) and a probe of codehead's servers.

use std::collections::BTreeMap;
use std::collections::HashSet;
use std::sync::mpsc::Receiver;

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap};

use crate::engine::harnesses;
use crate::engine::roster::{self, Agent};
use crate::engine::servers::{self, Server, Status};
use crate::infra::mcp_list::{self, Native};
use crate::ui::textarea::TextArea;
use crate::ui::theme;

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
    Refreshed(mcp_list::Found),
    Probed,
    Done(String, bool),
    /// An import of a harness's server ended: (harness, name, the server + its toast).
    Imported {
        harness: String,
        name: String,
        result: Result<(Server, String), String>,
    },
    /// The edit form's save ended: (the server, every server).
    Saved(Result<(Server, Vec<Server>), String>),
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
    Info {
        title: String,
        lines: Vec<String>,
        login: Option<(String, String)>,
    },
    Edit(Box<EditForm>),
    Agents(Pick),
    Confirm {
        text: String,
        then: Then,
    },
}

enum Then {
    Delete(String),
    Import {
        harness: String,
        name: String,
        source: String,
    },
}

struct McpJobs {
    tx: crate::infra::bus::Tx<Msg>,
    rx: Receiver<Msg>,
    /// Jobs running: CLI refresh, probe, sign-in.
    refreshing: bool,
    probing: bool,
    busy: Vec<String>,
    cli_errors: Vec<(String, String)>,
}

pub(crate) struct McpTab {
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
    jobs: McpJobs,
    status: String,
    error: bool,
    modal: Option<Modal>,
    /// Click targets of the last draw: (area, index into `items()`).
    hits: Vec<(Rect, usize)>,
    modal_hits: Vec<(Rect, usize)>,
    /// The last esc in the edit form: a 2nd within 2 s drops its edits.
    esc_at: Option<std::time::Instant>,
    /// The edit form whose save (keychain, file) runs off the UI thread: back on an error.
    saving: Option<Box<EditForm>>,
}

impl McpTab {
    pub(crate) fn new() -> Self {
        let (sender, receiver) = crate::infra::bus::channel();
        let mut filter = TextArea::new(true, "filter servers ( / )");

        filter.style = Some(theme::text());
        let mut thread = Self {
            filter,
            filtering: false,
            cursor: 0,
            offset: 0,
            servers: Vec::new(),
            natives: Vec::new(),
            statuses: BTreeMap::new(),
            agents: Vec::new(),
            ever: HashSet::default(),
            jobs: crate::features::settings::mcp_tab::McpJobs {
                tx: sender,
                rx: receiver,
                refreshing: false,
                probing: false,
                busy: Vec::new(),
                cli_errors: Vec::new(),
            },

            status: String::new(),
            error: false,
            modal: None,
            hits: Vec::new(),
            modal_hits: Vec::new(),
            esc_at: None,
            saving: None,
        };

        thread.load();

        thread
    }

    /// codehead's servers, the cached statuses, the agents: from disk.
    fn load(&mut self) {
        load_mcp_tab(self);
    }

    /// The tab came into view: refresh the harness lists and probe codehead's servers.
    pub(crate) fn enter(&mut self) {
        self.load();

        if cfg!(test) {
            return;
        }

        start_refresh(self);

        self::probe(self);
    }

    /// Jobs still running: the draw shows "updating".
    pub(crate) fn updating(&self) -> bool {
        self.jobs.refreshing || self.jobs.probing || !self.jobs.busy.is_empty()
    }

    /// Every row the filter keeps: the "needs action" rows first, then all rows by group.
    fn items(&self) -> (Vec<Item>, usize) {
        items_mcp_tab(self)
    }

    fn current(&self) -> Option<Item> {
        self.items().0.get(self.cursor).cloned()
    }

    fn login(&mut self, harness: String, name: String) {
        login_mcp_tab(self, harness, name);
    }

    fn run(&mut self, then: Then) {
        run_mcp_tab(self, then);
    }

    pub(crate) fn paste(&mut self, text: &str) {
        match &mut self.modal {
            Some(Modal::Edit(error)) => match error.field {
                0 => error.name.insert(&text.replace(['\n', '\r'], "")),
                2 => error.target.insert(&text.replace(['\n', '\r'], " ")),
                3 => error.secrets.insert(text),
                _ => {}
            },
            None if self.filtering => self.filter.insert(&text.replace(['\n', '\r'], "")),
            _ => {}
        }
    }

    pub(crate) fn help(&self) -> &'static [(&'static str, &'static str)] {
        help_mcp_tab(self)
    }
}

fn forget_secrets(refs: Vec<String>) {
    if !refs.is_empty() {
        std::thread::spawn(move || refs.iter().for_each(|reference| servers::forget_secret(reference)));
    }
}

impl Default for McpTab {
    fn default() -> Self {
        Self::new()
    }
}

/// The group of connectors Claude Code never connected (its "unused connectors").
const UNUSED: &str = "claude.ai connectors · unused";

impl EditForm {
    fn target_hint(kind: usize) -> &'static str {
        if kind == 0 {
            "npx -y @modelcontextprotocol/server-filesystem ~/x"
        } else {
            "https://mcp.example.com/mcp"
        }
    }

    fn of(server: Option<&Server>) -> EditForm {
        let input = |text: &str, hint: &str, single: bool| {
            let mut thread = TextArea::new(single, hint);

            thread.set_text(text);
            thread.style = Some(theme::text());
            thread
        };

        let kind = server.map_or(0, |server_value| {
            KINDS.iter().position(|key| *key == server_value.kind).unwrap_or(0)
        });

        let target = server.map(Server::target).unwrap_or_default();

        let pairs = server
            .map(server_secrets)
            .map(|message| {
                message
                    .iter()
                    .map(|(key_value, value)| format!("{key_value}={value}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();

        EditForm {
            old: server.map(|server_2| server_2.name.clone()),
            kind,
            name: input(server.map_or("", |server_3| server_3.name.as_str()), "linear", true),
            target: input(&target, Self::target_hint(kind), true),
            secrets: input(&pairs, "API_KEY=…", false),
            auto_allow: server.is_none_or(|server_4| server_4.auto_allow),
            field: 0,
        }
    }
}

fn server_secrets(server: &Server) -> &BTreeMap<String, String> {
    if server.kind == "stdio" {
        &server.env
    } else {
        &server.headers
    }
}

#[cfg(test)]
#[path = "settings_mcp_tab_tests.rs"]
mod tests;

#[path = "mcp_tab_draw.rs"]
mod draw;
#[path = "mcp_tab_draw_helpers.rs"]
mod draw_helpers;
#[path = "mcp_tab_key.rs"]
mod key;
#[path = "mcp_tab_key_helpers.rs"]
mod key_helpers;
#[path = "mcp_tab_mouse.rs"]
mod mouse;
#[path = "mcp_tab_poll.rs"]
mod poll;

#[path = "mcp_tab_catalog.rs"]
mod catalog;
use catalog::{codehead_item, ever_connected, harness_label, harness_order, icon, info_for, native_item};

#[path = "mcp_tab_edit.rs"]
mod edit;
use edit::{import, save_edit, secrets_at_end, secrets_at_start};

#[path = "mcp_tab_actions.rs"]
mod actions;
use actions::{
    agents_current, import_current, imported, login_current, open_add, open_agents, open_current, probe, save_agents,
    say, toggle_allow,
};

fn row(frame: &mut Frame, area: Rect, item: &Item, detail: &str, selected: bool, label_width: usize) {
    let (mark, style_value) = icon(&item.state);
    let base = if selected { theme::pill() } else { theme::text() };

    let name: String = if item.name.chars().count() > label_width {
        item.name.chars().take(label_width - 1).chain(['…']).collect()
    } else {
        item.name.clone()
    };

    let pad = label_width.saturating_sub(name.chars().count());
    let room = (area.width as usize).saturating_sub(label_width + 5);
    let detail: String = detail.chars().take(room).collect();
    let line = Line::from(vec![
        Span::styled(format!(" {mark} "), if selected { base } else { style_value }),
        Span::styled(format!("{name}{} ", " ".repeat(pad)), base),
        Span::styled(detail, if selected { base } else { theme::muted() }),
    ]);

    frame.render_widget(Paragraph::new(line).style(base), area);
}

#[cfg(test)]
use catalog::read_ever;
#[cfg(test)]
use edit::{secure, server};

fn load_mcp_tab(mcp_tab: &mut McpTab) {
    match servers::load() {
        Ok(value) => mcp_tab.servers = value,
        Err(error) => self::say(error, true, mcp_tab),
    }

    mcp_tab.statuses = servers::statuses();
    mcp_tab.agents = roster::active();
    mcp_tab.ever = ever_connected();

    // the cache: natives as the last refresh left them
    if mcp_tab.natives.is_empty() {
        mcp_tab.natives = mcp_tab
            .statuses
            .iter()
            .filter(|(harness_value, _)| *harness_value != "codehead")
            .flat_map(|(harness_2, statuses_value)| {
                statuses_value
                    .iter()
                    .filter(|(_, status_value)| status_value.source != "codehead" && !status_value.source.is_empty())
                    .map(move |(name_value, status_2)| Native {
                        harness: harness_2.clone(),
                        name: name_value.clone(),
                        source: status_2.source.clone(),
                        target: String::new(),
                        status: status_2.clone(),
                    })
            })
            .collect();
    }
}

fn items_mcp_tab(mcp_tab: &McpTab) -> (Vec<Item>, usize) {
    let query = mcp_tab.filter.text().trim().to_lowercase();

    let keep = |i: &Item| {
        query.is_empty()
            || crate::domain::notes::fuzzy(&query, &i.name).is_some()
            || crate::domain::notes::fuzzy(&query, i.group).is_some()
    };

    let mut all: Vec<Item> = mcp_tab
        .servers
        .iter()
        .map(|server| codehead_item(server, mcp_tab))
        .collect();
    let order = [
        "Claude Code · own config",
        "claude.ai connectors · Claude Code only",
        "Claude Code plugins · Claude Code only",
        "Codex · own config",
        "OpenCode · own config",
        UNUSED,
    ];
    let mut natives: Vec<Item> = mcp_tab
        .natives
        .iter()
        .map(|native| native_item(native, mcp_tab))
        .collect();

    natives.sort_by_key(|i| {
        (
            order
                .iter()
                .position(|group_value| *group_value == i.group)
                .unwrap_or(9),
            i.name.to_lowercase(),
        )
    });

    all.extend(natives);
    let all: Vec<Item> = all.into_iter().filter(|i| keep(i)).collect();

    let mut out: Vec<Item> = all
        .iter()
        .filter(|i| !i.action.is_empty())
        .cloned()
        .map(|mut i| {
            i.group = "needs action";
            i
        })
        .collect();

    let action_count = out.len();

    out.extend(all);

    (out, action_count)
}

fn login_mcp_tab(mcp_tab: &mut McpTab, harness: String, name: String) {
    if mcp_list::login_command(&harness, &name).is_none() {
        mcp_tab.modal = Some(Modal::Info {
            title: format!("{name} · sign in"),
            lines: vec![
                format!("{} has 0 sign-in command codehead can run.", harness_label(&harness)),
                format!("Open {} and sign in to {name} there.", harness_label(&harness)),
            ],
            login: None,
        });
        return;
    }

    mcp_tab.jobs.busy.push(name.clone());
    self::say(format!("signing in to {name}: the browser opens"), false, mcp_tab);
    let sender = mcp_tab.jobs.tx.clone();

    std::thread::spawn(move || {
        let result = mcp_list::login(&harness, &name);

        let message = match result {
            Ok(()) => (format!("{name}: signed in · r: refresh"), false),
            Err(error) => (format!("{name}: {error}"), true),
        };

        let _ = sender.send(Msg::Done(message.0, message.1));
    });
}

fn run_mcp_tab(mcp_tab: &mut McpTab, then: Then) {
    match then {
        Then::Delete(name) => {
            let mut all = mcp_tab.servers.clone();

            // its keychain entries go off the UI thread (tens of ms each)
            let refs: Vec<String> = all
                .iter()
                .filter(|text| text.name == name)
                .flat_map(|text_value| text_value.env.values().chain(text_value.headers.values()).cloned())
                .filter(|value| value.starts_with("keychain:"))
                .collect();

            forget_secrets(refs);

            all.retain(|text_2| text_2.name != name);

            if let Err(error) = servers::save(&all) {
                return self::say(error, true, mcp_tab);
            }

            for mut agent_value in roster::active().into_iter().filter(|agent| agent.mcp.contains(&name)) {
                agent_value.mcp.retain(|message| *message != name);
                let _ = roster::save(&agent_value);
            }

            mcp_tab.servers = all;
            mcp_tab.agents = roster::active();
            self::say(format!("{name}: deleted"), false, mcp_tab);
        }
        // the harness CLI (`codex mcp list`, `claude mcp remove`), the keychain: seconds, off
        // the UI thread (tests: at once)
        Then::Import { harness, name, source } => {
            let servers = mcp_tab.servers.clone();
            let (height, count) = (harness.clone(), name.clone());
            let run = move || import(&height, &count, &source, servers);

            if cfg!(test) {
                let result = run();

                return self::imported(&harness, &name, result, mcp_tab);
            }

            mcp_tab.jobs.busy.push(name.clone());
            self::say(format!("importing {name}…"), false, mcp_tab);
            let sender = mcp_tab.jobs.tx.clone();

            std::thread::spawn(move || {
                let _ = sender.send(Msg::Imported {
                    harness,
                    name,
                    result: run(),
                });
            });
        }
    }
}

fn help_mcp_tab(mcp_tab: &McpTab) -> &'static [(&'static str, &'static str)] {
    match &mcp_tab.modal {
        Some(Modal::Edit(_)) => &[
            ("tab ↑↓", "field"),
            ("←→", "type"),
            ("space", "auto-allow"),
            ("ctrl+s", "save"),
            ("esc", "cancel"),
        ],
        Some(Modal::Agents(_)) => &[
            ("↑↓", "move"),
            ("space", "agent"),
            ("h", "whole harness"),
            ("enter", "save"),
            ("esc", "cancel"),
        ],
        Some(Modal::Confirm { .. }) => &[("y enter", "yes"), ("n esc", "no")],
        Some(Modal::Info { .. }) => &[("l", "sign in"), ("enter esc", "close")],
        None if mcp_tab.filtering => &[("type", "filter"), ("enter ↓", "list"), ("esc", "clear")],
        None => &[
            ("ctrl+f", "filter"),
            ("a", "add"),
            ("enter", "edit / details"),
            ("g", "agents"),
            ("l", "sign in"),
            ("i", "import"),
            ("t", "auto-allow"),
            ("d", "delete"),
            ("r", "refresh"),
        ],
    }
}

fn start_refresh(mcp_tab: &mut McpTab) {
    if !mcp_tab.jobs.refreshing {
        mcp_tab.jobs.refreshing = true;
        let sender = mcp_tab.jobs.tx.clone();

        std::thread::spawn(move || {
            let _ = sender.send(Msg::Refreshed(mcp_list::refresh()));
        });
    }
}
