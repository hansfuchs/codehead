//! Screens: new (compose, the start) | threads | agents; notes = sub-screen of task. Modals: F2 menu, y/n confirm, info.

pub(crate) mod start;

pub(crate) mod repo_pick;

pub(crate) mod drafts;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap};
use unicode_width::UnicodeWidthStr;

use crate::domain::agents::{self, Agent, Use};
use crate::domain::notes::{Notes, Row, slug};
use crate::domain::threads::{self, Worktree};
use crate::features::prompt_input::attach::{Pasted, Paster};
use crate::features::prompt_input::command::Command;
use crate::infra::background::{self, Reads};
use crate::infra::claude::{self, Session};
use crate::infra::styles::{self, Style as OutStyle};
use crate::screens::start_screen::drafts::Draft;
use crate::ui::dropdown;
use crate::ui::mention::Mention;
use crate::ui::preview::Previews;
use crate::ui::sprites::{self, Sprite};
use crate::ui::textarea::{Outcome as Input, TextArea};
use crate::ui::theme;

/// Agent card rows: the sprite + 2 border rows.
use crate::ui::cards::CARD_H;

/// Below this width the preview goes under the list (1/3 list, 2/3 preview).
const THIN: u16 = 110;

/// The main tabs in order (F1-F7, alt+1-7).
const MAIN_TABS: [Tab; 7] = [
    Tab::New,
    Tab::Threads,
    Tab::Projects,
    Tab::Merge,
    Tab::Agents,
    Tab::Flows,
    Tab::Settings,
];

/// Choices of REPO (CWD: the dir codehead runs in): current branch, new branch, new branch +
/// worktree, default branch (main, else master), the other branches (newest commit first).
fn where_choices(repo: &Path, cwd: &Path) -> Choices {
    // 2 git runs at once (~15 ms each): off the UI thread (`quick_choices` until then)
    let (bs, checkouts) = std::thread::scope(|text| {
        let checkouts = text.spawn(|| threads::checkouts(repo));

        (threads::branches(repo), checkouts.join().unwrap_or_default())
    });

    choices_of(repo, cwd, bs, checkouts)
}

/// The where choices by the file system only (0 git runs): current branch, new branch, new
/// branch + worktree. The branches come with `where_choices`.
fn quick_choices(repo: &Path, cwd: &Path) -> Choices {
    choices_of(repo, cwd, Vec::new(), HashMap::new())
}

fn choices_of(repo: &Path, cwd: &Path, bs: Vec<String>, checkouts: HashMap<String, String>) -> Choices {
    let here_branch = threads::current_branch(cwd);
    let repo_branch = threads::current_branch(repo);
    let here_dir = threads::toplevel(cwd)
        .unwrap_or_else(|| repo.to_path_buf())
        .display()
        .to_string();
    let default = ["main", "master"]
        .into_iter()
        .find(|directory| bs.iter().any(|bytes| bytes == directory))
        .map(String::from);
    let mut wheres = vec![Where::Current, Where::NewBranch, Where::NewWorktree];

    if let Some(directory_2) = default
        .as_ref()
        .filter(|directory_value| **directory_value != here_branch)
    {
        wheres.push(Where::Branch(directory_2.clone()));
    }

    wheres.extend(
        bs.into_iter()
            .filter(|bytes_value| *bytes_value != here_branch && Some(bytes_value) != default.as_ref())
            .map(Where::Branch),
    );

    Choices {
        wheres,
        checkouts,
        repo_branch,
        here_branch,
        here_dir,
    }
}

const F_NAME: usize = 0;
const F_HARNESS: usize = 1;
const F_MODEL: usize = 2;
const F_EFFORT: usize = 3;
const F_COLOR: usize = 4;
const F_MODE: usize = 5;
const F_STYLE: usize = 6;
const F_MCP: usize = 7;
const F_SET: usize = 8;
const F_SPRITE: usize = 9;
const F_VOICE: usize = 10;
const F_PROMPT: usize = 11;
const F_BUTTONS: usize = 12;
const FORM_FIELDS: usize = 13;

type AgentSnapshot = (Vec<Agent>, String, HashMap<String, Vec<Use>>);

/// 2 esc presses within this time: quit on the task tab, close local controls elsewhere.
const DOUBLE_ESC: Duration = Duration::from_millis(500);

/// Toast tail for a session restart after an agent edit.
fn restart_note(result: &agents::Restart) -> String {
    let mut parts = Vec::new();

    if result.resumed > 0 {
        parts.push(format!("{} sessions restarted (conversation kept)", result.resumed));
    }

    if result.stopped > 0 {
        parts.push(format!(
            "{} sessions stopped (new harness: next message starts fresh)",
            result.stopped
        ));
    }

    if !result.busy.is_empty() {
        parts.push(format!(
            "{} busy: old profile until stopped ({})",
            result.busy.len(),
            result.busy.join(", ")
        ));
    }

    if !result.failed.is_empty() {
        parts.push(format!("failed: {}", result.failed.join("; ")));
    }

    parts.iter().fold(String::new(), |mut output, part| {
        output.push_str(" · ");
        output.push_str(part);
        output
    })
}

/// Area of the (area, index) row at AT.
fn find_rows(area: &[(Rect, usize)], at: Position) -> Option<Rect> {
    area.iter()
        .map(|(result, _)| *result)
        .find(|result_value| result_value.contains(at))
}

/// Filter row ROW: the input cells after the `filter › ` label, before the count (COUNT cells).
fn filter_input(row: Rect, count: usize) -> Rect {
    let width = row.width.saturating_sub(9 + count as u16 + 1).max(1);

    Rect {
        x: row.x + 9,
        y: row.y,
        width,
        height: 1,
    }
}

/// Quick tab id of "keys of this screen".
const KEYS_ITEM: usize = 1000;

/// Threads tab row of a thread card (the menu opens it like the threads tab does).
fn worktree_of(character: &crate::ui::thread_card::Card) -> Worktree {
    let path = character
        .dir
        .as_ref()
        .map(|directory| directory.display().to_string())
        .unwrap_or_default();
    let thread = crate::domain::threads::Thread {
        id: character.id.clone(),
        title: character.title.clone(),
        state: character.state.clone(),
        live: character.agents.live,
        updated: character.activity / 1000,
        worktree: None,
        branch: (!character.branch.is_empty()).then(|| character.branch.clone()),
        dir: Some(path.clone()),
        busy: character.agents.busy,
        needs: character.agents.blocked.then(|| "question".to_string()),
        ..Default::default()
    };

    Worktree {
        path,
        branch: if character.branch.is_empty() {
            "in place".into()
        } else {
            character.branch.clone()
        },
        thread: Some(thread),
        unpushed: 0,
        when: String::new(),
        linked: character.worktree,
        repo: character.repo.clone(),
    }
}

fn hint(key: &str, label: &str) -> Vec<Span<'static>> {
    vec![
        Span::styled(key.to_string(), theme::text()),
        Span::styled(format!(" {label}  "), theme::muted()),
    ]
}

fn trash(path: &std::path::Path) -> Result<&'static str, String> {
    let ok = |character: &mut std::process::Command| {
        character
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|text| text.success())
    };

    if ok(std::process::Command::new("gio").arg("trash").arg(path)) {
        return Ok("moved to trash");
    }

    if ok(std::process::Command::new("trash-put").arg(path)) {
        return Ok("moved to trash");
    }

    let result = if path.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    };

    result.map(|()| "deleted").map_err(|error| error.to_string())
}

/// App name pill at the left of the header and the waiting screen.
const BRAND: &str = " codehead ";

/// Waiting screen while a thread starts.
pub(crate) fn draw_status(frame: &mut Frame, text: &str, tick: usize) {
    let area = frame.area();

    frame.render_widget(Paragraph::new("").style(theme::text()), area);
    let spin = ["◐", "◓", "◑", "◒"][tick % 4];
    let line = Line::from(vec![
        Span::styled(BRAND, theme::accent_bold()),
        Span::styled(format!(" {spin} "), theme::accent()),
        Span::styled(text.to_string(), theme::muted()),
    ]);

    frame.render_widget(Paragraph::new(line), Rect { height: 1, ..area });
}

/// The flow a new task of repo KEY starts with: the last one picked there.
fn flow_defaults_path() -> PathBuf {
    crate::engine::data_dir().join("flow-defaults.json")
}

fn flow_default_get(key: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_slice(&std::fs::read(flow_defaults_path()).ok()?).ok()?;

    value[key].as_str().map(String::from)
}

fn flow_default_set(key: &str, slug: &str) {
    let path = flow_defaults_path();
    let mut value: serde_json::Value = std::fs::read(&path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_else(|| serde_json::json!({}));

    value[key] = serde_json::Value::String(slug.to_string());
    let _ = crate::engine::write_atomic(&path, value.to_string().as_bytes());
}

mod actions;
mod agent_form;
mod agent_navigation;
mod agents_draw;
mod app_state;
mod commands;
mod compose_draw;
mod compose_input;
mod draw;
mod form_draw;
mod form_models;
mod initialize;
mod input;
mod list_input;
mod modal_draw;
mod modal_input;
mod mouse;
mod navigation;
mod notes_draw;
mod paste;
mod project_menu;
mod refresh;
mod repository;
mod selection;
mod state_initialization;
mod tabs;
mod task_context;
mod task_drafts;
mod task_flows;
mod task_message;
mod threads_draw;
mod where_menu;

use agent_form::{AgentForm, FormPick};
pub(crate) use app_state::App;

#[cfg(test)]
#[path = "start_screen_tests.rs"]
mod start_screen_tests;

#[cfg(test)]
#[path = "start_screen_navigation_keys_tests.rs"]
mod navigation_keys;

#[cfg(test)]
#[path = "start_screen_preview_selection_tests.rs"]
mod preview_selection_tests;

#[cfg(test)]
#[path = "start_screen_flow_menu_tests.rs"]
mod flow_menu_tests;

#[cfg(test)]
#[path = "start_screen_keys_tests.rs"]
mod key_fix_tests;

mod controls;
use controls::{
    Act, Choices, Cmd, ConfirmAction, Field, FlowMenu, Job, Modal, Region, Regions, Screen, StartCtx, Tab, Where,
    WhereMenu,
};
pub(crate) use controls::{Config, Outcome, Suspend, Target};

mod task_layout;

mod flow_preview;

mod form_input;

mod preview_selection;

use preview_selection::draw_preview_selection;

mod header_draw;

mod screen_layout;

mod preview_draw;

use preview_draw::draw_preview;

mod filter_draw;

use filter_draw::{draw_agent_filter, draw_note_filter};

mod mouse_lists;

use mouse_lists::{mouse_lists, mouse_thread_list};

mod mouse_compose;

use mouse_compose::mouse_compose;

mod mouse_form;

use mouse_form::mouse_agent_form;

#[cfg(test)]
mod start_screen_mouse_tests;

pub(crate) use crate::domain::fuzzy::fuzzy;
pub(crate) use crate::infra::paths::tilde;
pub(crate) use crate::ui::keys::key_name;
