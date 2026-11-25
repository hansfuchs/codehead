//! The open ask of a thread view as a picker: its rows, the cursor, the picks, the "Other"
//! text, and the keys. The view gives it the 1st open ask and sends the answer it returns.
//!
//! Kinds of asks:
//! - questions (`AskUserQuestion`): 1-4 questions, each with options + an "Other" row (free
//!   text), single or multi pick; answered 1 by 1, ←→ go between them, the last one sends all.
//! - an ACP agent's own options: 1 pick each.
//! - a permission request: allow / deny.
//!
//! Keys while the ask has the focus (the default; tab gives it to the prompt and back):
//! ↑↓ move (round), space picks (multi: toggles), enter answers, 1-9 pick a row (single: answer),
//! ←→ previous / next question, letters type into "Other". Other keys: the view's.

use std::collections::BTreeSet;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::{Value, json};

use crate::domain::chat::Ask;

/// What a key did.
#[derive(Debug, PartialEq)]
pub(crate) enum Act {
    /// Not the picker's key: the view handles it.
    Pass,
    /// The picker changed (redraw).
    Changed,
    /// The answer for the host (`op: answer`).
    Answer(Value),
}

/// The picker of 1 ask (`sync` resets it when the 1st open ask changes).
#[derive(Debug, Default)]
pub(crate) struct Picker {
    id: String,
    /// The ask has the keys (else the prompt).
    pub focus: bool,
    /// The answer went to the host: 0 keys until the ask closes (or the send failed).
    pub sent: bool,
    /// Question shown (questions only).
    pub question: usize,
    /// Cursor row.
    pub at: usize,
    /// Per question: picked rows (single: 0 or 1 row).
    picks: Vec<BTreeSet<usize>>,
    /// Per question: the "Other" text.
    others: Vec<String>,
    /// Per question: its answer, once given.
    answers: Vec<Option<String>>,
}

/// A row of the picker as the stream draws it.
#[derive(Debug, PartialEq)]
pub(crate) struct Row {
    pub label: String,
    pub desc: String,
    pub cursor: bool,
    pub picked: bool,
    /// The "Other" row: LABEL is its text so far.
    pub other: bool,
}

pub(super) type Question = (String, String, Vec<(String, String)>, bool);

fn questions(agent: &Ask) -> Vec<Question> {
    agent.questions()
}

/// K edits a text field with a modifier: ⌫ / Del, ctrl or ⌘ + a letter of the edit keys
/// (u w h k a e d x z y), alt + a letter (alt+b/f/d), alt / ctrl / ⌘ + ←→ Home End.
fn text_edit(key: &KeyEvent) -> bool {
    let message = key.modifiers;
    let cmd = message.intersects(KeyModifiers::CONTROL | KeyModifiers::SUPER);

    match key.code {
        KeyCode::Backspace | KeyCode::Delete | KeyCode::Left | KeyCode::Right | KeyCode::Home | KeyCode::End => true,
        KeyCode::Char(character) if cmd && !message.contains(KeyModifiers::ALT) => {
            "uwhkaedxzy".contains(character.to_ascii_lowercase())
        }
        KeyCode::Char(character_value) if message.contains(KeyModifiers::ALT) => character_value.is_ascii_alphabetic(),
        _ => false,
    }
}

impl Picker {
    /// Bind to ASK (the 1st open ask; None: 0 asks). A new ask: a fresh picker with the focus.
    pub(crate) fn sync(&mut self, ask: Option<&Ask>) {
        self.sync_with(ask, true);
    }

    /// As `sync`; FOCUS: a new ask takes the focus. False (the owner types in the prompt): the
    /// ask waits for tab, 0 typed key lands in the ask by surprise.
    pub(crate) fn sync_with(&mut self, ask: Option<&Ask>, focus: bool) {
        sync_with_picker(self, ask, focus);
    }

    /// Answers so far: (question, answer) of the answered questions before the shown one.
    pub(crate) fn answered(&self, agent: &Ask) -> Vec<(String, String)> {
        questions(agent)
            .iter()
            .zip(&self.answers)
            .filter_map(|((query, ..), ans)| ans.clone().map(|x| (query.clone(), x)))
            .collect()
    }

    /// Row count of the shown question (options + Other), else of the choices.
    fn rows_n(&self, agent: &Ask) -> usize {
        let qs = questions(agent);

        match qs.get(self.question) {
            Some((_, _, opts, _)) => opts.len() + 1,
            None if !agent.options().is_empty() => agent.options().len(),
            None => 2,
        }
    }

    /// The rows to draw.
    pub(crate) fn rows(&self, agent: &Ask) -> Vec<Row> {
        rows_picker(self, agent)
    }

    /// The hint row under the choices.
    pub(crate) fn hint(&self, agent: &Ask) -> String {
        hint_picker(self, agent)
    }

    /// Key K for ask A. Tab moves the focus between the ask and the prompt; with the focus, the
    /// ask takes every plain key but esc, page up / down and F-keys (0 key edits the hidden prompt).
    pub(crate) fn key(&mut self, agent: &Ask, key_value: KeyEvent) -> Act {
        key_picker(self, agent, key_value)
    }

    /// A click on row I: multi toggles it, the Other row takes the cursor, else the pick.
    pub(crate) fn click(&mut self, agent: &Ask, i: usize) -> Act {
        self.sync(Some(agent));
        self.focus = true;
        self.at = i;

        if other_row(agent, self) == Some(i) {
            return Act::Changed;
        }

        if multi(agent, self) {
            toggle(i, self);
            return Act::Changed;
        }

        enter(agent, i, self)
    }
}

fn hint_body(agent: &Ask, height: &mut Vec<&str>) {
    height.push("enter / 1-9: pick");

    // a flow's wait: the prompt's text goes along
    if agent.tool == crate::engine::flows::run::WAIT_TOOL {
        height.push("text in the prompt: your note");
    }
}

fn rows_own(agent: &Ask, row: impl Fn(usize, String, String, bool) -> Row) -> Vec<Row> {
    let own = agent.options();

    let labels: Vec<String> = if own.is_empty() {
        vec!["allow".into(), "deny".into()]
    } else {
        own.into_iter().map(|output| output.1).collect()
    };

    labels
        .into_iter()
        .enumerate()
        .map(|(i, line_value)| row(i, line_value, String::new(), false))
        .collect()
}

#[cfg(test)]
#[path = "chat_ask_tests.rs"]
mod tests;

fn multi(agent: &Ask, view_state: &Picker) -> bool {
    questions(agent).get(view_state.question).is_some_and(|query| query.3)
}

/// The "Other" row of the shown question (None: not a question).
fn other_row(agent: &Ask, view_state: &Picker) -> Option<usize> {
    questions(agent).get(view_state.question).map(|query| query.2.len())
}

fn key_keycode_backspace_keycode_char(view_state: &mut Picker) {
    if view_state.others[view_state.question].is_empty() {
        view_state.picks[view_state.question].remove(&view_state.at);
    }
}

fn key_keycode_char_character_value(agent: &Ask, other: Option<usize>, character_value: char, view_state: &mut Picker) {
    if let Some(output) = other {
        view_state.at = output;
        view_state.others[view_state.question].push(character_value);

        if multi(agent, view_state) {
            view_state.picks[view_state.question].insert(output);
        }
    }
}

fn show(agent: &Ask, query: usize, view_state: &mut Picker) {
    view_state.question = query;

    // the cursor on the 1st pick of that question, else its 1st row
    view_state.at = view_state.picks[query]
        .iter()
        .next()
        .copied()
        .unwrap_or(0)
        .min(view_state.rows_n(agent) - 1);
}

fn toggle(i: usize, view_state: &mut Picker) {
    let path = &mut view_state.picks[view_state.question];

    if !path.remove(&i) {
        path.insert(i);
    }
}

/// Enter / a pick on row I: answer the shown question, show the next open one, or send.
fn enter(agent: &Ask, i: usize, view_state: &mut Picker) -> Act {
    let qs = questions(agent);

    let Some((_, _, opts, multi)) = qs.get(view_state.question) else {
        return choice(agent, i, view_state);
    };

    let other = view_state.others[view_state.question].trim().to_string();

    let label = |key: usize| {
        if key == opts.len() {
            other.clone()
        } else {
            opts[key].0.clone()
        }
    };

    let rows: Vec<usize> = if *multi {
        let path: Vec<usize> = view_state.picks[view_state.question]
            .iter()
            .copied()
            .filter(|&key_value| key_value < opts.len() || !other.is_empty())
            .collect();

        // 0 picks: the cursor row
        if path.is_empty() { vec![i] } else { path }
    } else {
        vec![i]
    };

    if rows.iter().any(|&key_2| key_2 == opts.len() && other.is_empty()) {
        // "Other" with 0 text: 0 answer
        return Act::Changed;
    }

    if !*multi {
        view_state.picks[view_state.question] = rows.iter().copied().collect();
    }

    view_state.answers[view_state.question] = Some(rows.into_iter().map(label).collect::<Vec<_>>().join(", "));

    // the next open question (after this one first), else send
    if let Some(next) = (view_state.question + 1..qs.len())
        .chain(0..view_state.question)
        .find(|&j| view_state.answers[j].is_none())
    {
        show(agent, next, view_state);
        return Act::Changed;
    }

    let answers: serde_json::Map<String, Value> = qs
        .iter()
        .zip(&view_state.answers)
        .map(|((query, ..), x)| (query.clone(), Value::String(x.clone().unwrap_or_default())))
        .collect();
    let label = view_state
        .answers
        .iter()
        .flatten()
        .cloned()
        .collect::<Vec<_>>()
        .join(" · ");

    Act::Answer(json!({"op": "answer", "id": agent.id, "allow": true, "answers": answers, "label": label}))
}

/// A permission / ACP choice I.
fn choice(agent: &Ask, i: usize, _view_state: &Picker) -> Act {
    let own = agent.options();

    if own.is_empty() {
        let allow = i == 0;

        return Act::Answer(
            json!({"op": "answer", "id": agent.id, "allow": allow, "label": if allow { "allow" } else { "deny" }}),
        );
    }

    let Some((id, name, kind)) = own.get(i).cloned() else {
        return Act::Changed;
    };

    Act::Answer(
        json!({"op": "answer", "id": agent.id, "allow": kind.starts_with("allow"), "option": id, "label": name}),
    )
}

fn hint_last(agent: &Ask, qs: &[Question], height: &mut Vec<&str>, view_state: &Picker) {
    let last = (0..qs.len())
        .filter(|&j| j != view_state.question)
        .all(|j| view_state.answers.get(j).is_some_and(Option::is_some));

    if multi(agent, view_state) {
        height.push("space: pick");
        height.push(if last { "enter: answer" } else { "enter: next" });
    } else {
        height.push(if last {
            "space / enter: answer"
        } else {
            "space / enter: next"
        });
    }

    height.push("type: other");

    if qs.len() > 1 {
        height.push("←→ questions");
    }
}

fn sync_with_picker(state: &mut Picker, ask: Option<&Ask>, focus: bool) {
    let Some(agent) = ask else {
        *state = Picker::default();
        return;
    };

    if state.id == agent.id {
        return;
    }

    let count = questions(agent).len().max(1);

    *state = Picker {
        id: agent.id.clone(),
        focus,
        picks: vec![BTreeSet::new(); count],
        others: vec![String::new(); count],
        answers: vec![None; count],
        ..Default::default()
    };
}

fn rows_picker(state: &Picker, agent: &Ask) -> Vec<Row> {
    let qs = questions(agent);
    let row = |i: usize, label: String, desc: String, other: bool| Row {
        label,
        desc,
        cursor: i == state.at,
        picked: state.picks.get(state.question).is_some_and(|path| path.contains(&i)),
        other,
    };

    if let Some((_, _, opts, _)) = qs.get(state.question) {
        let mut value: Vec<Row> = opts
            .iter()
            .enumerate()
            .map(|(i, (line, directory))| row(i, line.clone(), directory.clone(), false))
            .collect();

        value.push(row(
            opts.len(),
            state.others.get(state.question).cloned().unwrap_or_default(),
            String::new(),
            true,
        ));
        value
    } else {
        rows_own(agent, row)
    }
}

fn hint_picker(state: &Picker, agent: &Ask) -> String {
    if state.sent {
        return "answer sent…".into();
    }

    if !state.focus && agent.tool == crate::engine::flows::run::WAIT_TOOL {
        return "ctrl+enter: your message (the wait stays) · tab: the choices".into();
    }

    if !state.focus {
        return "tab: answer the question".into();
    }

    let qs = questions(agent);
    let mut height: Vec<&str> = vec!["↑↓ move"];

    if qs.is_empty() {
        hint_body(agent, &mut height);
    } else {
        hint_last(agent, &qs, &mut height, state);
    }

    height.push("tab: prompt");

    height.join(" · ")
}

fn key_picker(state: &mut Picker, agent: &Ask, key_value: KeyEvent) -> Act {
    // a picker of another ask (0 sync yet): a fresh one for A, the focus stays in the prompt
    state.sync_with(Some(agent), false);

    if state.sent {
        return Act::Pass;
    }

    let none = key_value.modifiers.is_empty();
    let typed = none || key_value.modifiers == KeyModifiers::SHIFT;
    let other = other_row(agent, state);
    let on_other = other == Some(state.at);

    // with the focus, a text edit key with a modifier (ctrl+w, alt+⌫, ctrl+u …): the Other
    // text's, else 0 effect (never the hidden prompt's)
    if state.focus && !typed && text_edit(&key_value) {
        if on_other
            && crate::ui::textarea::delete_key(&mut state.others[state.question], &key_value)
            && state.others[state.question].is_empty()
        {
            state.picks[state.question].remove(&state.at);
        }

        return Act::Changed;
    }

    if !state.focus
        || !typed
        || matches!(
            key_value.code,
            KeyCode::Esc | KeyCode::PageUp | KeyCode::PageDown | KeyCode::F(_)
        )
    {
        return Act::Pass;
    }

    let count = state.rows_n(agent);
    let qn = questions(agent).len();

    match key_value.code {
        // shift+↑↓: the view scrolls
        KeyCode::Up | KeyCode::Down if !none => return Act::Pass,
        KeyCode::Up => state.at = (state.at + count - 1) % count,
        KeyCode::Down => state.at = (state.at + 1) % count,
        KeyCode::Left if state.question > 0 => show(agent, state.question - 1, state),
        KeyCode::Right if state.question + 1 < qn && state.answers[state.question].is_some() => {
            show(agent, state.question + 1, state);
        }
        KeyCode::Backspace | KeyCode::Char('u' | 'w' | 'h')
            if on_other && crate::ui::textarea::delete_key(&mut state.others[state.question], &key_value) =>
        {
            key_keycode_backspace_keycode_char(state);
        }
        // space: in "Other" text a space; multi: toggle; single / choices: pick
        KeyCode::Char(' ') if on_other && !state.others[state.question].is_empty() => {
            state.others[state.question].push(' ');
        }
        KeyCode::Char(' ') if multi(agent, state) => toggle(state.at, state),
        KeyCode::Enter | KeyCode::Char(' ') => return enter(agent, state.at, state),
        KeyCode::Char(character @ '1'..='9') if !on_other => {
            return number_key(agent, character as usize - '1' as usize, count, other, state);
        }
        // letters: into "Other" (permissions: 0)
        KeyCode::Char(character_value) => {
            key_keycode_char_character_value(agent, other, character_value, state);
        }
        _ => {}
    }

    Act::Changed
}

fn number_key(agent: &Ask, index: usize, count: usize, other: Option<usize>, state: &mut Picker) -> Act {
    if index >= count {
        return Act::Changed;
    }

    state.at = index;

    if Some(index) == other {
        return Act::Changed;
    }

    if multi(agent, state) {
        toggle(index, state);
        return Act::Changed;
    }

    enter(agent, index, state)
}
