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

fn questions(ask: &Ask) -> Vec<Question> {
    ask.questions()
}

/// K edits a text field with a modifier: ⌫ / Del, ctrl or ⌘ + a letter of the edit keys
/// (u w h k a e d x z y), alt + a letter (alt+b/f/d), alt / ctrl / ⌘ + ←→ Home End.
fn text_edit(key: &KeyEvent) -> bool {
    let modifiers = key.modifiers;
    let cmd = modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::SUPER);

    match key.code {
        KeyCode::Backspace | KeyCode::Delete | KeyCode::Left | KeyCode::Right | KeyCode::Home | KeyCode::End => true,
        KeyCode::Char(letter) if cmd && !modifiers.contains(KeyModifiers::ALT) => {
            "uwhkaedxzy".contains(letter.to_ascii_lowercase())
        }
        KeyCode::Char(letter) if modifiers.contains(KeyModifiers::ALT) => letter.is_ascii_alphabetic(),
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
        sync_picker(self, ask, focus);
    }

    /// Answers so far: (question, answer) of the answered questions before the shown one.
    pub(crate) fn answered(&self, ask: &Ask) -> Vec<(String, String)> {
        questions(ask)
            .iter()
            .zip(&self.answers)
            .filter_map(|((question, ..), answer)| answer.clone().map(|answer| (question.clone(), answer)))
            .collect()
    }

    /// Row count of the shown question (options + Other), else of the choices.
    fn rows_n(&self, ask: &Ask) -> usize {
        let qs = questions(ask);

        match qs.get(self.question) {
            Some((_, _, opts, _)) => opts.len() + 1,
            None if !ask.options().is_empty() => ask.options().len(),
            None => 2,
        }
    }

    /// The rows to draw.
    pub(crate) fn rows(&self, ask: &Ask) -> Vec<Row> {
        picker_rows(self, ask)
    }

    /// The hint row under the choices.
    pub(crate) fn hint(&self, ask: &Ask) -> String {
        picker_hint(self, ask)
    }

    /// Key K for ask A. Tab moves the focus between the ask and the prompt; with the focus, the
    /// ask takes every plain key but esc, page up / down and F-keys (0 key edits the hidden prompt).
    pub(crate) fn key(&mut self, ask: &Ask, key: KeyEvent) -> Act {
        picker_key(self, ask, key)
    }

    /// A click on row I: multi toggles it, the Other row takes the cursor, else the pick.
    pub(crate) fn click(&mut self, ask: &Ask, i: usize) -> Act {
        self.sync(Some(ask));
        self.focus = true;
        self.at = i;

        if other_row(ask, self) == Some(i) {
            return Act::Changed;
        }

        if multi(ask, self) {
            toggle(i, self);
            return Act::Changed;
        }

        enter(ask, i, self)
    }
}

fn push_choice_hints(ask: &Ask, hints: &mut Vec<&str>) {
    hints.push("enter / 1-9: pick");

    // a flow's wait: the prompt's text goes along
    if ask.tool == crate::engine::flows::run::WAIT_TOOL {
        hints.push("text in the prompt: your note");
    }
}

fn choice_rows(ask: &Ask, row: impl Fn(usize, String, String, bool) -> Row) -> Vec<Row> {
    let own = ask.options();

    let labels: Vec<String> = if own.is_empty() {
        vec!["allow".into(), "deny".into()]
    } else {
        own.into_iter().map(|option| option.1).collect()
    };

    labels
        .into_iter()
        .enumerate()
        .map(|(i, label)| row(i, label, String::new(), false))
        .collect()
}

#[cfg(test)]
#[path = "chat_ask_tests.rs"]
mod tests;

fn multi(ask: &Ask, picker: &Picker) -> bool {
    questions(ask).get(picker.question).is_some_and(|question| question.3)
}

/// The "Other" row of the shown question (None: not a question).
fn other_row(ask: &Ask, picker: &Picker) -> Option<usize> {
    questions(ask).get(picker.question).map(|question| question.2.len())
}

fn unpick_empty_other(picker: &mut Picker) {
    if picker.others[picker.question].is_empty() {
        picker.picks[picker.question].remove(&picker.at);
    }
}

fn type_other(ask: &Ask, other: Option<usize>, letter: char, picker: &mut Picker) {
    if let Some(row) = other {
        picker.at = row;
        picker.others[picker.question].push(letter);

        if multi(ask, picker) {
            picker.picks[picker.question].insert(row);
        }
    }
}

fn show(ask: &Ask, question: usize, picker: &mut Picker) {
    picker.question = question;

    // the cursor on the 1st pick of that question, else its 1st row
    picker.at = picker.picks[question]
        .iter()
        .next()
        .copied()
        .unwrap_or(0)
        .min(picker.rows_n(ask) - 1);
}

fn toggle(i: usize, picker: &mut Picker) {
    let picks = &mut picker.picks[picker.question];

    if !picks.remove(&i) {
        picks.insert(i);
    }
}

/// Enter / a pick on row I: answer the shown question, show the next open one, or send.
fn enter(ask: &Ask, i: usize, picker: &mut Picker) -> Act {
    let qs = questions(ask);

    let Some((_, _, opts, multi)) = qs.get(picker.question) else {
        return choice(ask, i, picker);
    };

    let other = picker.others[picker.question].trim().to_string();

    let label = |row: usize| {
        if row == opts.len() {
            other.clone()
        } else {
            opts[row].0.clone()
        }
    };

    let rows: Vec<usize> = if *multi {
        let chosen: Vec<usize> = picker.picks[picker.question]
            .iter()
            .copied()
            .filter(|&row| row < opts.len() || !other.is_empty())
            .collect();

        // 0 picks: the cursor row
        if chosen.is_empty() { vec![i] } else { chosen }
    } else {
        vec![i]
    };

    if rows.iter().any(|&row| row == opts.len() && other.is_empty()) {
        // "Other" with 0 text: 0 answer
        return Act::Changed;
    }

    if !*multi {
        picker.picks[picker.question] = rows.iter().copied().collect();
    }

    picker.answers[picker.question] = Some(rows.into_iter().map(label).collect::<Vec<_>>().join(", "));

    // the next open question (after this one first), else send
    if let Some(next) = (picker.question + 1..qs.len())
        .chain(0..picker.question)
        .find(|&j| picker.answers[j].is_none())
    {
        show(ask, next, picker);
        return Act::Changed;
    }

    let answers: serde_json::Map<String, Value> = qs
        .iter()
        .zip(&picker.answers)
        .map(|((question, ..), answer)| (question.clone(), Value::String(answer.clone().unwrap_or_default())))
        .collect();
    let label = picker.answers.iter().flatten().cloned().collect::<Vec<_>>().join(" · ");

    Act::Answer(json!({"op": "answer", "id": ask.id, "allow": true, "answers": answers, "label": label}))
}

/// A permission / ACP choice I.
fn choice(ask: &Ask, i: usize, _picker: &Picker) -> Act {
    let own = ask.options();

    if own.is_empty() {
        let allow = i == 0;

        return Act::Answer(
            json!({"op": "answer", "id": ask.id, "allow": allow, "label": if allow { "allow" } else { "deny" }}),
        );
    }

    let Some((id, name, kind)) = own.get(i).cloned() else {
        return Act::Changed;
    };

    Act::Answer(json!({"op": "answer", "id": ask.id, "allow": kind.starts_with("allow"), "option": id, "label": name}))
}

fn push_question_hints(ask: &Ask, qs: &[Question], hints: &mut Vec<&str>, picker: &Picker) {
    let last = (0..qs.len())
        .filter(|&j| j != picker.question)
        .all(|j| picker.answers.get(j).is_some_and(Option::is_some));

    if multi(ask, picker) {
        hints.push("space: pick");
        hints.push(if last { "enter: answer" } else { "enter: next" });
    } else {
        hints.push(if last {
            "space / enter: answer"
        } else {
            "space / enter: next"
        });
    }

    hints.push("type: other");

    if qs.len() > 1 {
        hints.push("←→ questions");
    }
}

fn sync_picker(picker: &mut Picker, ask: Option<&Ask>, focus: bool) {
    let Some(ask) = ask else {
        *picker = Picker::default();
        return;
    };

    if picker.id == ask.id {
        return;
    }

    let count = questions(ask).len().max(1);

    *picker = Picker {
        id: ask.id.clone(),
        focus,
        picks: vec![BTreeSet::new(); count],
        others: vec![String::new(); count],
        answers: vec![None; count],
        ..Default::default()
    };
}

fn picker_rows(picker: &Picker, ask: &Ask) -> Vec<Row> {
    let qs = questions(ask);
    let row = |i: usize, label: String, desc: String, other: bool| Row {
        label,
        desc,
        cursor: i == picker.at,
        picked: picker
            .picks
            .get(picker.question)
            .is_some_and(|picks| picks.contains(&i)),
        other,
    };

    if let Some((_, _, opts, _)) = qs.get(picker.question) {
        let mut rows: Vec<Row> = opts
            .iter()
            .enumerate()
            .map(|(i, (label, desc))| row(i, label.clone(), desc.clone(), false))
            .collect();

        rows.push(row(
            opts.len(),
            picker.others.get(picker.question).cloned().unwrap_or_default(),
            String::new(),
            true,
        ));
        rows
    } else {
        choice_rows(ask, row)
    }
}

fn picker_hint(picker: &Picker, ask: &Ask) -> String {
    if picker.sent {
        return "answer sent…".into();
    }

    if !picker.focus && ask.tool == crate::engine::flows::run::WAIT_TOOL {
        return "ctrl+enter: your message (the wait stays) · tab: the choices".into();
    }

    if !picker.focus {
        return "tab: answer the question".into();
    }

    let qs = questions(ask);
    let mut hints: Vec<&str> = vec!["↑↓ move"];

    if qs.is_empty() {
        push_choice_hints(ask, &mut hints);
    } else {
        push_question_hints(ask, &qs, &mut hints, picker);
    }

    hints.push("tab: prompt");

    hints.join(" · ")
}

fn picker_key(picker: &mut Picker, ask: &Ask, key: KeyEvent) -> Act {
    // a picker of another ask (0 sync yet): a fresh one for A, the focus stays in the prompt
    picker.sync_with(Some(ask), false);

    if picker.sent {
        return Act::Pass;
    }

    let plain = key.modifiers.is_empty();
    let typed = plain || key.modifiers == KeyModifiers::SHIFT;
    let other = other_row(ask, picker);
    let on_other = other == Some(picker.at);

    // with the focus, a text edit key with a modifier (ctrl+w, alt+⌫, ctrl+u …): the Other
    // text's, else 0 effect (never the hidden prompt's)
    if picker.focus && !typed && text_edit(&key) {
        if on_other
            && crate::ui::textarea::delete_key(&mut picker.others[picker.question], &key)
            && picker.others[picker.question].is_empty()
        {
            picker.picks[picker.question].remove(&picker.at);
        }

        return Act::Changed;
    }

    if !picker.focus
        || !typed
        || matches!(
            key.code,
            KeyCode::Esc | KeyCode::PageUp | KeyCode::PageDown | KeyCode::F(_)
        )
    {
        return Act::Pass;
    }

    let count = picker.rows_n(ask);
    let qn = questions(ask).len();

    match key.code {
        // shift+↑↓: the view scrolls
        KeyCode::Up | KeyCode::Down if !plain => return Act::Pass,
        KeyCode::Up => picker.at = (picker.at + count - 1) % count,
        KeyCode::Down => picker.at = (picker.at + 1) % count,
        KeyCode::Left if picker.question > 0 => show(ask, picker.question - 1, picker),
        KeyCode::Right if picker.question + 1 < qn && picker.answers[picker.question].is_some() => {
            show(ask, picker.question + 1, picker);
        }
        KeyCode::Backspace | KeyCode::Char('u' | 'w' | 'h')
            if on_other && crate::ui::textarea::delete_key(&mut picker.others[picker.question], &key) =>
        {
            unpick_empty_other(picker);
        }
        // space: in "Other" text a space; multi: toggle; single / choices: pick
        KeyCode::Char(' ') if on_other && !picker.others[picker.question].is_empty() => {
            picker.others[picker.question].push(' ');
        }
        KeyCode::Char(' ') if multi(ask, picker) => toggle(picker.at, picker),
        KeyCode::Enter | KeyCode::Char(' ') => return enter(ask, picker.at, picker),
        KeyCode::Char(digit @ '1'..='9') if !on_other => {
            return number_key(ask, digit as usize - '1' as usize, count, other, picker);
        }
        // letters: into "Other" (permissions: 0)
        KeyCode::Char(letter) => {
            type_other(ask, other, letter, picker);
        }
        _ => {}
    }

    Act::Changed
}

fn number_key(ask: &Ask, index: usize, count: usize, other: Option<usize>, picker: &mut Picker) -> Act {
    if index >= count {
        return Act::Changed;
    }

    picker.at = index;

    if Some(index) == other {
        return Act::Changed;
    }

    if multi(ask, picker) {
        toggle(index, picker);
        return Act::Changed;
    }

    enter(ask, index, picker)
}
