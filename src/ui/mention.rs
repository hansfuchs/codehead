//! `@` in a prompt input: a dropdown of agent cards (0 border) above the `@` (below: 0 room above),
//! grouped by harness like every other screen, filtered as the name is typed. ↑↓ move, tab / enter
//! insert `@name `, esc closes it (until the `@` is gone). With `replaceable` agents (thread view):
//! ←→ switch add / replace. Replace opens a 2nd dropdown of the thread's agents: ↑↓ pick 1, enter
//! inserts `@name (replaces @old) ` and closes both, ← (or esc) goes back to add.

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use crate::domain::agents::Agent;
use crate::ui::cards;
use crate::ui::dropdown;
use crate::ui::textarea::{Pos, TextArea};
use crate::ui::theme;

/// Rows of 1 card in the dropdown: the sprite (0 blank rows between cards).
const CARD_ROWS: u16 = crate::ui::sprites::HEIGHT as u16;

/// What a click hit.
#[derive(Clone, Debug, PartialEq)]
enum Hit {
    Card(String),
    Target(String),
}

#[derive(Default)]
pub(crate) struct Mention {
    /// Index of the current card in the filtered order (groups A→Z, agents A→Z).
    cursor: usize,
    /// 1st virtual row in view.
    offset: usize,
    /// esc on the dropdown of the `@` at this position: closed until that `@` is gone.
    closed: Option<Pos>,
    /// Rows of the last draw (mouse).
    rows: Vec<(Rect, Hit)>,
    /// Agents of the thread a pick can replace. Empty: add only.
    pub replaceable: Vec<String>,
    /// Replace mode: the cursor in the 2nd dropdown (the agents the current card can replace).
    replace: Option<usize>,
    /// The last pick: name + the agent it replaces (None: add).
    pub picked: Option<(String, Option<String>)>,
    /// ↑↓: the current agent's `@name` in the text (the typed query: the filter).
    preview: crate::ui::dropdown::Preview,
}

/// Text after `@new` for a replace pick: ` (replaces @old)`.
pub(crate) fn replaces_note(old: &str) -> String {
    format!(" (replaces @{old})")
}

/// TEXT without its `(replaces @name)` notes (each with the space before it).
pub(crate) fn strip_replaces(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;

    while let Some(i) = rest.find(" (replaces @") {
        let after = &rest[i + " (replaces @".len()..];
        let name_end = after
            .find(|character: char| !(character.is_alphanumeric() || matches!(character, '-' | '_' | '.')))
            .unwrap_or(after.len());

        if after[name_end..].starts_with(')') {
            out.push_str(&rest[..i]);
            rest = &after[name_end + 1..];
        } else {
            out.push_str(&rest[..=i]);
            rest = &rest[i + 1..];
        }
    }

    out.push_str(rest);
    out
}

/// Agents (indexes of AGENTS) whose name starts with Q (0 such: contains Q), in dropdown order:
/// by harness, A→Z.
fn visible(agents: &[Agent], query: &str) -> Vec<(String, Vec<usize>)> {
    let query = query.to_lowercase();
    let prefix = agents.iter().any(|agent| agent.name.to_lowercase().starts_with(&query));

    cards::groups(agents, |agent| {
        if prefix {
            agent.name.to_lowercase().starts_with(&query)
        } else {
            agent.name.to_lowercase().contains(&query)
        }
    })
}

fn order(groups: &[(String, Vec<usize>)]) -> Vec<usize> {
    groups.iter().flat_map(|(_, group)| group.iter().copied()).collect()
}

impl Mention {
    /// Agents NAME can replace (every agent of the thread but NAME).
    fn targets(&self, name: &str) -> Vec<String> {
        self.replaceable
            .iter()
            .filter(|target| *target != name)
            .cloned()
            .collect()
    }

    fn pick(&mut self, ta: &mut TextArea, at: Pos, name: &str, old: Option<String>) {
        let text = match &old {
            Some(old) => format!("@{name}{} ", replaces_note(old)),
            None => format!("@{name} "),
        };

        ta.replace_to_cursor(at, &text);
        self.picked = Some((name.to_string(), old));
        self.cursor = 0;
        self.offset = 0;
        self.replace = None;
    }

    /// The `@` of TA, and its esc state: a closed `@` that is gone opens again on the next `@`.
    fn sync(&mut self, ta: &TextArea) {
        if self.closed.is_some() && ta.mention().map(|found| found.0) != self.closed {
            self.closed = None;
        }
    }

    /// The `@` of TA + its query. A live preview in the text: the query typed before it filters.
    /// None: 0 `@`, or closed.
    fn query(&self, ta: &TextArea) -> Option<(Pos, String)> {
        let (at, query) = ta.mention()?;

        if self.closed == Some(at) {
            return None;
        }

        let query = match self.preview.typed(ta) {
            Some((pos, typed)) if pos == at => typed.chars().skip(1).collect(),
            _ => query,
        };

        Some((at, query))
    }

    /// The open dropdown: `@` position + the matching agents (in order). None: closed.
    fn open(&self, ta: &TextArea, agents: &[Agent]) -> Option<(Pos, Vec<usize>)> {
        let (at, query) = self.query(ta)?;
        let shown = order(&visible(agents, &query));

        (!shown.is_empty()).then_some((at, shown))
    }

    pub(crate) fn is_open(&self, ta: &TextArea, agents: &[Agent]) -> bool {
        self.open(ta, agents).is_some()
    }

    /// Key for the dropdown. true: used (the input must not see it).
    pub(crate) fn key(&mut self, key: KeyEvent, ta: &mut TextArea, agents: &[Agent]) -> bool {
        self.sync(ta);

        let Some((at, shown)) = self.open(ta, agents) else {
            self.cursor = 0;
            self.replace = None;
            return false;
        };

        let plain = key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT;
        let count = shown.len();

        self.cursor = self.cursor.min(count - 1);
        let name = agents[shown[self.cursor]].name.clone();
        let targets = self.targets(&name);

        if let Some(character) = self.replace {
            return replace_key(key, ta, at, &name, &targets, character, self);
        }

        key_add(
            key,
            ta,
            agents,
            self,
            MentionSelection {
                at,
                shown,
                plain,
                count,
                name,
                targets,
            },
        )
    }

    /// Row of the last draw at P (hover).
    pub(crate) fn row_at(&self, at: Position) -> Option<Rect> {
        self.rows.iter().map(|(rect, _)| *rect).find(|rect| rect.contains(at))
    }

    /// Click on a card: insert it (add); on an agent of the replace dropdown: replace it. true: used.
    pub(crate) fn mouse(&mut self, ev: MouseEvent, ta: &mut TextArea, agents: &[Agent]) -> bool {
        handle_click(self, ev, ta, agents)
    }
}

#[cfg(test)]
#[path = "mention_tests.rs"]
mod tests;

fn handle_click(mention: &mut Mention, ev: MouseEvent, ta: &mut TextArea, agents: &[Agent]) -> bool {
    if !matches!(ev.kind, MouseEventKind::Down(MouseButton::Left)) {
        return false;
    }

    let at = Position {
        x: ev.column,
        y: ev.row,
    };

    let Some(hit) = mention
        .rows
        .iter()
        .find(|(rect, _)| rect.contains(at))
        .map(|(_, hit)| hit.clone())
    else {
        return false;
    };

    if let Some((pos, shown)) = mention.open(ta, agents) {
        match hit {
            Hit::Card(name) => mention.pick(ta, pos, &name, None),
            Hit::Target(old) => {
                let name = agents[shown[mention.cursor.min(shown.len() - 1)]].name.clone();

                mention.pick(ta, pos, &name, Some(old));
            }
        }
    }

    true
}

fn key_add(
    key: KeyEvent,
    ta: &mut TextArea,
    agents: &[Agent],
    mention: &mut Mention,
    settings: MentionSelection,
) -> bool {
    let MentionSelection {
        at,
        shown,
        plain,
        count,
        name,
        targets,
    } = settings;

    match key.code {
        KeyCode::Up | KeyCode::Down if plain => {
            mention.cursor = if key.code == KeyCode::Up {
                (mention.cursor + count - 1) % count
            } else {
                (mention.cursor + 1) % count
            };

            // the agent's `@name` in the text at once
            let typed = match mention.preview.typed(ta) {
                Some((_, typed)) => typed,
                None => ta.mention().map(|(_, query)| format!("@{query}")).unwrap_or_default(),
            };

            let name = agents[shown[mention.cursor]].name.clone();

            mention.preview.show(ta, at, &typed, &format!("@{name}"));
        }
        KeyCode::Right if key.modifiers.is_empty() && !targets.is_empty() => mention.replace = Some(0),
        KeyCode::Left if key.modifiers.is_empty() && !targets.is_empty() => {}
        KeyCode::Tab | KeyCode::Enter if key.modifiers.is_empty() => mention.pick(ta, at, &name, None),
        KeyCode::Esc if key.modifiers.is_empty() => {
            mention.closed = Some(at);
            mention.cursor = 0;
        }
        // a name char after a preview: the typed query again, the char after it
        KeyCode::Char(ch)
            if plain && (ch.is_alphanumeric() || matches!(ch, '-' | '_' | '.')) && mention.preview.revert(ta) =>
        {
            mention.cursor = 0;
            return false;
        }
        _ => {
            // typing goes on: the 1st match is current again
            if matches!(key.code, KeyCode::Char(_) | KeyCode::Backspace) {
                mention.cursor = 0;
            }

            return false;
        }
    }

    true
}

#[path = "mention_render.rs"]
mod render;

struct MentionSelection {
    at: Pos,
    shown: Vec<usize>,
    plain: bool,
    count: usize,
    name: String,
    targets: Vec<String>,
}

fn replace_key(
    key: KeyEvent,
    ta: &mut TextArea,
    at: Pos,
    name: &str,
    targets: &[String],
    target: usize,
    mention: &mut Mention,
) -> bool {
    let plain = key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT;
    let targets_len = targets.len().max(1);

    match key.code {
        KeyCode::Up if plain => mention.replace = Some((target + targets_len - 1) % targets_len),
        KeyCode::Down if plain => mention.replace = Some((target + 1) % targets_len),
        KeyCode::Left | KeyCode::Esc if key.modifiers.is_empty() => mention.replace = None,
        KeyCode::Right if key.modifiers.is_empty() => {}
        KeyCode::Tab | KeyCode::Enter if key.modifiers.is_empty() => {
            let old = targets.get(target.min(targets_len - 1)).cloned();

            mention.pick(ta, at, name, old);
        }
        _ => {
            // typing goes on: back to add, the 1st match current again
            mention.replace = None;
            mention.cursor = 0;
            return false;
        }
    }

    true
}
