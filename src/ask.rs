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

use crate::stream::Ask;

/// What a key did.
#[derive(Debug, PartialEq)]
pub enum Act {
    /// Not the picker's key: the view handles it.
    Pass,
    /// The picker changed (redraw).
    Changed,
    /// The answer for the host (`op: answer`).
    Answer(Value),
}

/// The picker of 1 ask (`sync` resets it when the 1st open ask changes).
#[derive(Debug, Default)]
pub struct Picker {
    id: String,
    /// The ask has the keys (else the prompt).
    pub focus: bool,
    /// The answer went to the host: 0 keys until the ask closes (or the send failed).
    pub sent: bool,
    /// Question shown (questions only).
    pub q: usize,
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
pub struct Row {
    pub label: String,
    pub desc: String,
    pub cursor: bool,
    pub picked: bool,
    /// The "Other" row: LABEL is its text so far.
    pub other: bool,
}

fn questions(a: &Ask) -> Vec<(String, String, Vec<(String, String)>, bool)> {
    a.questions()
}

/// K edits a text field with a modifier: ⌫ / Del, ctrl or ⌘ + a letter of the edit keys
/// (u w h k a e d x z y), alt + a letter (alt+b/f/d), alt / ctrl / ⌘ + ←→ Home End.
fn text_edit(k: &KeyEvent) -> bool {
    let m = k.modifiers;
    let cmd = m.intersects(KeyModifiers::CONTROL | KeyModifiers::SUPER);
    match k.code {
        KeyCode::Backspace | KeyCode::Delete => true,
        KeyCode::Char(c) if cmd && !m.contains(KeyModifiers::ALT) => "uwhkaedxzy".contains(c.to_ascii_lowercase()),
        KeyCode::Char(c) if m.contains(KeyModifiers::ALT) => c.is_ascii_alphabetic(),
        KeyCode::Left | KeyCode::Right | KeyCode::Home | KeyCode::End => true,
        _ => false,
    }
}

impl Picker {
    /// Bind to ASK (the 1st open ask; None: 0 asks). A new ask: a fresh picker with the focus.
    pub fn sync(&mut self, ask: Option<&Ask>) {
        self.sync_with(ask, true);
    }

    /// As `sync`; FOCUS: a new ask takes the focus. False (the owner types in the prompt): the
    /// ask waits for tab, 0 typed key lands in the ask by surprise.
    pub fn sync_with(&mut self, ask: Option<&Ask>, focus: bool) {
        let Some(a) = ask else {
            *self = Picker::default();
            return;
        };
        if self.id == a.id {
            return;
        }
        let n = questions(a).len().max(1);
        *self = Picker { id: a.id.clone(), focus, picks: vec![BTreeSet::new(); n], others: vec![String::new(); n], answers: vec![None; n], ..Default::default() };
    }

    /// Answers so far: (question, answer) of the answered questions before the shown one.
    pub fn answered(&self, a: &Ask) -> Vec<(String, String)> {
        questions(a).iter().zip(&self.answers).filter_map(|((q, ..), ans)| ans.clone().map(|x| (q.clone(), x))).collect()
    }

    /// Row count of the shown question (options + Other), else of the choices.
    fn rows_n(&self, a: &Ask) -> usize {
        let qs = questions(a);
        match qs.get(self.q) {
            Some((_, _, opts, _)) => opts.len() + 1,
            None if !a.options().is_empty() => a.options().len(),
            None => 2,
        }
    }

    fn multi(&self, a: &Ask) -> bool {
        questions(a).get(self.q).is_some_and(|q| q.3)
    }

    /// The "Other" row of the shown question (None: not a question).
    fn other_row(&self, a: &Ask) -> Option<usize> {
        questions(a).get(self.q).map(|q| q.2.len())
    }

    /// The rows to draw.
    pub fn rows(&self, a: &Ask) -> Vec<Row> {
        let qs = questions(a);
        let row = |i: usize, label: String, desc: String, other: bool| Row { label, desc, cursor: i == self.at, picked: self.picks.get(self.q).is_some_and(|p| p.contains(&i)), other };
        match qs.get(self.q) {
            Some((_, _, opts, _)) => {
                let mut v: Vec<Row> = opts.iter().enumerate().map(|(i, (l, d))| row(i, l.clone(), d.clone(), false)).collect();
                v.push(row(opts.len(), self.others.get(self.q).cloned().unwrap_or_default(), String::new(), true));
                v
            }
            None => {
                let own = a.options();
                let labels: Vec<String> = if own.is_empty() { vec!["allow".into(), "deny".into()] } else { own.into_iter().map(|o| o.1).collect() };
                labels.into_iter().enumerate().map(|(i, l)| row(i, l, String::new(), false)).collect()
            }
        }
    }

    /// The hint row under the choices.
    pub fn hint(&self, a: &Ask) -> String {
        if self.sent {
            return "answer sent…".into();
        }
        if !self.focus {
            return "tab: answer the question".into();
        }
        let qs = questions(a);
        let mut h: Vec<&str> = vec!["↑↓ move"];
        if qs.is_empty() {
            h.push("enter / 1-9: pick");
            // a flow's wait: the prompt's text goes along
            if a.tool == crate::flowbar::WAIT_TOOL {
                h.push("text in the prompt: your note");
            }
        } else {
            let last = (0..qs.len()).filter(|&j| j != self.q).all(|j| self.answers.get(j).is_some_and(Option::is_some));
            if self.multi(a) {
                h.push("space: pick");
                h.push(if last { "enter: answer" } else { "enter: next" });
            } else {
                h.push(if last { "space / enter: answer" } else { "space / enter: next" });
            }
            h.push("type: other");
            if qs.len() > 1 {
                h.push("←→ questions");
            }
        }
        h.push("tab: prompt");
        h.join(" · ")
    }

    /// Key K for ask A. Tab moves the focus between the ask and the prompt; with the focus, the
    /// ask takes every plain key but esc, page up / down and F-keys (0 key edits the hidden prompt).
    pub fn key(&mut self, a: &Ask, k: KeyEvent) -> Act {
        // a picker of another ask (0 sync yet): a fresh one for A, the focus stays in the prompt
        self.sync_with(Some(a), false);
        if self.sent {
            return Act::Pass;
        }
        if k.code == KeyCode::Tab && k.modifiers.is_empty() {
            self.focus = !self.focus;
            return Act::Changed;
        }
        let none = k.modifiers.is_empty();
        let typed = none || k.modifiers == KeyModifiers::SHIFT;
        let other = self.other_row(a);
        let on_other = other == Some(self.at);

        // with the focus, a text edit key with a modifier (ctrl+w, alt+⌫, ctrl+u …): the Other
        // text's, else 0 effect (never the hidden prompt's)
        if self.focus && !typed && text_edit(&k) {
            if on_other && crate::textarea::delete_key(&mut self.others[self.q], &k) && self.others[self.q].is_empty() {
                self.picks[self.q].remove(&self.at);
            }
            return Act::Changed;
        }

        if !self.focus || !typed || matches!(k.code, KeyCode::Esc | KeyCode::PageUp | KeyCode::PageDown | KeyCode::F(_)) {
            return Act::Pass;
        }
        let n = self.rows_n(a);
        let qn = questions(a).len();
        match k.code {
            // shift+↑↓: the view scrolls
            KeyCode::Up | KeyCode::Down if !none => return Act::Pass,
            KeyCode::Up => self.at = (self.at + n - 1) % n,
            KeyCode::Down => self.at = (self.at + 1) % n,
            KeyCode::Left if self.q > 0 => self.show(a, self.q - 1),
            KeyCode::Right if self.q + 1 < qn && self.answers[self.q].is_some() => self.show(a, self.q + 1),
            KeyCode::Enter => return self.enter(a, self.at),
            KeyCode::Backspace | KeyCode::Char('u' | 'w' | 'h') if on_other && crate::textarea::delete_key(&mut self.others[self.q], &k) => {
                if self.others[self.q].is_empty() {
                    self.picks[self.q].remove(&self.at);
                }
            }
            // space: in "Other" text a space; multi: toggle; single / choices: pick
            KeyCode::Char(' ') if on_other && !self.others[self.q].is_empty() => self.others[self.q].push(' '),
            KeyCode::Char(' ') if self.multi(a) => self.toggle(self.at),
            KeyCode::Char(' ') => return self.enter(a, self.at),
            KeyCode::Char(c @ '1'..='9') if !on_other => {
                let i = c as usize - '1' as usize;
                if i < n {
                    self.at = i;
                    if Some(i) != other {
                        if self.multi(a) {
                            self.toggle(i);
                        } else {
                            return self.enter(a, i);
                        }
                    }
                }
            }
            // letters: into "Other" (permissions: 0)
            KeyCode::Char(c) => {
                if let Some(o) = other {
                    self.at = o;
                    self.others[self.q].push(c);
                    if self.multi(a) {
                        self.picks[self.q].insert(o);
                    }
                }
            }
            _ => {}
        }
        Act::Changed
    }

    /// A click on row I: multi toggles it, the Other row takes the cursor, else the pick.
    pub fn click(&mut self, a: &Ask, i: usize) -> Act {
        self.sync(Some(a));
        self.focus = true;
        self.at = i;
        if self.other_row(a) == Some(i) {
            return Act::Changed;
        }
        if self.multi(a) {
            self.toggle(i);
            return Act::Changed;
        }
        self.enter(a, i)
    }

    fn show(&mut self, a: &Ask, q: usize) {
        self.q = q;
        // the cursor on the 1st pick of that question, else its 1st row
        self.at = self.picks[q].iter().next().copied().unwrap_or(0).min(self.rows_n(a) - 1);
    }

    fn toggle(&mut self, i: usize) {
        let p = &mut self.picks[self.q];
        if !p.remove(&i) {
            p.insert(i);
        }
    }

    /// Enter / a pick on row I: answer the shown question, show the next open one, or send.
    fn enter(&mut self, a: &Ask, i: usize) -> Act {
        let qs = questions(a);
        let Some((_, _, opts, multi)) = qs.get(self.q) else {
            return self.choice(a, i);
        };
        let other = self.others[self.q].trim().to_string();
        let label = |k: usize| if k == opts.len() { other.clone() } else { opts[k].0.clone() };
        let rows: Vec<usize> = if *multi {
            let p: Vec<usize> = self.picks[self.q].iter().copied().filter(|&k| k < opts.len() || !other.is_empty()).collect();
            // 0 picks: the cursor row
            if p.is_empty() { vec![i] } else { p }
        } else {
            vec![i]
        };
        if rows.iter().any(|&k| k == opts.len() && other.is_empty()) {
            // "Other" with 0 text: 0 answer
            return Act::Changed;
        }
        if !*multi {
            self.picks[self.q] = rows.iter().copied().collect();
        }
        self.answers[self.q] = Some(rows.into_iter().map(label).collect::<Vec<_>>().join(", "));
        // the next open question (after this one first), else send
        if let Some(next) = (self.q + 1..qs.len()).chain(0..self.q).find(|&j| self.answers[j].is_none()) {
            self.show(a, next);
            return Act::Changed;
        }
        let answers: serde_json::Map<String, Value> = qs.iter().zip(&self.answers).map(|((q, ..), x)| (q.clone(), Value::String(x.clone().unwrap_or_default()))).collect();
        let label = self.answers.iter().flatten().cloned().collect::<Vec<_>>().join(" · ");
        Act::Answer(json!({"op": "answer", "id": a.id, "allow": true, "answers": answers, "label": label}))
    }

    /// A permission / ACP choice I.
    fn choice(&self, a: &Ask, i: usize) -> Act {
        let own = a.options();
        if own.is_empty() {
            let allow = i == 0;
            return Act::Answer(json!({"op": "answer", "id": a.id, "allow": allow, "label": if allow { "allow" } else { "deny" }}));
        }
        let Some((id, name, kind)) = own.get(i).cloned() else { return Act::Changed };
        Act::Answer(json!({"op": "answer", "id": a.id, "allow": kind.starts_with("allow"), "option": id, "label": name}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(question: &str, header: &str, multi: bool, opts: &[&str]) -> Value {
        json!({"question": question, "header": header, "multiSelect": multi, "options": opts.iter().map(|o| json!({"label": o, "description": format!("{o} desc")})).collect::<Vec<_>>()})
    }

    fn questions(id: &str, qs: Vec<Value>) -> Ask {
        Ask { id: id.into(), agent: "ada".into(), tool: "AskUserQuestion".into(), input: json!({"questions": qs}), ts: 1 }
    }

    fn permission() -> Ask {
        Ask { id: "p1".into(), agent: "ada".into(), tool: "Bash".into(), input: json!({"command": "ls"}), ts: 1 }
    }

    fn picker(a: &Ask) -> Picker {
        let mut p = Picker::default();
        p.sync(Some(a));
        p
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    /// Keys of a space-separated list: ↑ ↓ ← → ⏎ ␣ ⌫ ⇥, else each char.
    fn press(p: &mut Picker, a: &Ask, keys: &str) -> Act {
        let mut last = Act::Pass;
        for t in keys.split(' ').filter(|t| !t.is_empty()) {
            let codes: Vec<KeyCode> = match t {
                "↑" => vec![KeyCode::Up],
                "↓" => vec![KeyCode::Down],
                "←" => vec![KeyCode::Left],
                "→" => vec![KeyCode::Right],
                "⏎" => vec![KeyCode::Enter],
                "␣" => vec![KeyCode::Char(' ')],
                "⌫" => vec![KeyCode::Backspace],
                "⇥" => vec![KeyCode::Tab],
                s => s.chars().map(KeyCode::Char).collect(),
            };
            for c in codes {
                last = p.key(a, key(c));
            }
        }
        last
    }

    fn answer(act: Act) -> Value {
        match act {
            Act::Answer(v) => v,
            other => panic!("0 answer: {other:?}"),
        }
    }

    #[test]
    fn single_question_arrows_go_round_and_enter_answers_the_cursor_row() {
        let a = questions("q1", vec![q("Color?", "Color", false, &["red", "green", "blue"])]);
        let mut p = picker(&a);
        assert!(p.focus);
        // 3 options + Other: 4 rows, round
        press(&mut p, &a, "↑");
        assert_eq!(p.at, 3);
        press(&mut p, &a, "↓ ↓ ↓");
        assert_eq!(p.at, 2);
        let v = answer(press(&mut p, &a, "⏎"));
        assert_eq!(v["answers"]["Color?"], "blue");
        assert_eq!((v["id"].as_str(), v["allow"].as_bool(), v["label"].as_str()), (Some("q1"), Some(true), Some("blue")));
    }

    #[test]
    fn single_question_space_and_digits_answer_at_once() {
        let a = questions("q1", vec![q("Color?", "", false, &["red", "green"])]);
        let mut p = picker(&a);
        assert_eq!(answer(press(&mut p, &a, "↓ ␣"))["answers"]["Color?"], "green");
        let mut p = picker(&a);
        assert_eq!(answer(press(&mut p, &a, "1"))["answers"]["Color?"], "red");
        // a digit past the rows: 0 change; the Other row's digit: the cursor only
        let mut p = picker(&a);
        assert_eq!(press(&mut p, &a, "9"), Act::Changed);
        assert_eq!(p.at, 0);
        assert_eq!(press(&mut p, &a, "3"), Act::Changed);
        assert_eq!(p.at, 2);
    }

    #[test]
    fn multi_question_space_toggles_and_enter_sends_the_picks_in_option_order() {
        let a = questions("q1", vec![q("Which?", "Keys", true, &["a", "b", "c", "d"])]);
        let mut p = picker(&a);
        press(&mut p, &a, "↓ ↓ ␣ ↑ ↑ ␣ ↓ ␣ ␣");
        let rows = p.rows(&a);
        let picked: Vec<bool> = rows.iter().map(|r| r.picked).collect();
        assert_eq!(picked, vec![true, false, true, false, false]);
        assert_eq!(answer(press(&mut p, &a, "⏎"))["answers"]["Which?"], "a, c");
        // digits toggle too, enter with 0 picks: the cursor row
        let mut p = picker(&a);
        press(&mut p, &a, "2 4 2");
        assert_eq!(answer(press(&mut p, &a, "⏎"))["answers"]["Which?"], "d");
        let mut p = picker(&a);
        press(&mut p, &a, "↓ ↓");
        assert_eq!(answer(press(&mut p, &a, "⏎"))["answers"]["Which?"], "c");
    }

    #[test]
    fn letters_type_into_other_and_enter_sends_the_text() {
        let a = questions("q1", vec![q("Name?", "", false, &["ada", "bob"])]);
        let mut p = picker(&a);
        press(&mut p, &a, "Cl");
        assert_eq!(p.at, 2);
        press(&mut p, &a, "x ⌫ a u d e ␣ 5");
        assert_eq!(p.rows(&a)[2].label, "Claude 5");
        assert_eq!(answer(press(&mut p, &a, "⏎"))["answers"]["Name?"], "Claude 5");
        // Other with 0 text: 0 answer; space on it: 0 text
        let mut p = picker(&a);
        press(&mut p, &a, "↑");
        assert_eq!(press(&mut p, &a, "⏎"), Act::Changed);
        assert_eq!(press(&mut p, &a, "␣"), Act::Changed);
        assert!(p.rows(&a)[2].label.is_empty());
    }

    #[test]
    fn multi_other_text_is_picked_and_comes_last() {
        let a = questions("q1", vec![q("Which?", "", true, &["a", "b"])]);
        let mut p = picker(&a);
        press(&mut p, &a, "␣ zed");
        assert!(p.rows(&a)[2].picked);
        assert_eq!(answer(press(&mut p, &a, "⏎"))["answers"]["Which?"], "a, zed");
        // backspace to 0 text: Other unpicked
        let mut p = picker(&a);
        press(&mut p, &a, "z ⌫");
        assert!(!p.rows(&a)[2].picked);
        press(&mut p, &a, "↑ ↑ ␣");
        assert_eq!(answer(press(&mut p, &a, "⏎"))["answers"]["Which?"], "a");
    }

    #[test]
    fn several_questions_go_in_order_back_and_forth_then_send_all() {
        let a = questions("q1", vec![q("One?", "First", false, &["x", "y"]), q("Two?", "Second", true, &["m", "n", "o"]), q("Three?", "Third", false, &["p", "q"])]);
        let mut p = picker(&a);
        // → before an answer: 0 move
        press(&mut p, &a, "→");
        assert_eq!(p.q, 0);
        assert_eq!(press(&mut p, &a, "↓ ⏎"), Act::Changed);
        assert_eq!((p.q, p.at), (1, 0));
        press(&mut p, &a, "␣ ↓ ↓ ␣ ⏎");
        assert_eq!(p.q, 2);
        // back to the 1st: the cursor on its answer; another answer, then the open 3rd
        press(&mut p, &a, "← ←");
        assert_eq!((p.q, p.at), (0, 1));
        assert_eq!(p.answered(&a), vec![("One?".into(), "y".into()), ("Two?".into(), "m, o".into())]);
        press(&mut p, &a, "↑ ⏎");
        assert_eq!(p.q, 2);
        let v = answer(press(&mut p, &a, "2"));
        assert_eq!(v["answers"], json!({"One?": "x", "Two?": "m, o", "Three?": "q"}));
        assert_eq!(v["label"], "x · m, o · q");
    }

    #[test]
    fn going_back_to_an_answered_question_and_enter_sends_when_all_are_answered() {
        let a = questions("q1", vec![q("One?", "", false, &["x", "y"]), q("Two?", "", false, &["m", "n"])]);
        let mut p = picker(&a);
        press(&mut p, &a, "⏎");
        press(&mut p, &a, "←");
        assert_eq!(p.q, 0);
        // the 2nd still open: enter goes there
        assert_eq!(press(&mut p, &a, "↓ ⏎"), Act::Changed);
        assert_eq!(p.q, 1);
        let v = answer(press(&mut p, &a, "⏎"));
        assert_eq!(v["answers"], json!({"One?": "y", "Two?": "m"}));
    }

    #[test]
    fn tab_moves_the_focus_and_the_prompt_gets_every_key_then() {
        let a = questions("q1", vec![q("Color?", "", true, &["red", "green"])]);
        let mut p = picker(&a);
        assert_eq!(press(&mut p, &a, "⇥"), Act::Changed);
        assert!(!p.focus);
        for c in [KeyCode::Down, KeyCode::Char(' '), KeyCode::Enter, KeyCode::Char('1'), KeyCode::Char('x')] {
            assert_eq!(p.key(&a, key(c)), Act::Pass, "{c:?}");
        }
        assert!(p.rows(&a).iter().all(|r| !r.picked));
        assert_eq!(p.hint(&a), "tab: answer the question");
        press(&mut p, &a, "⇥");
        assert!(p.focus);
    }

    #[test]
    fn with_the_focus_the_view_keeps_esc_scroll_menu_and_ctrl_keys() {
        let a = questions("q1", vec![q("Color?", "", false, &["red", "green"])]);
        let mut p = picker(&a);
        for k in [key(KeyCode::Esc), key(KeyCode::PageUp), key(KeyCode::PageDown), key(KeyCode::F(2)), KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL), KeyEvent::new(KeyCode::Up, KeyModifiers::SHIFT), KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL)] {
            assert_eq!(p.key(&a, k), Act::Pass, "{k:?}");
        }
        // plain keys never reach the hidden prompt
        for c in [KeyCode::Left, KeyCode::Right, KeyCode::Backspace, KeyCode::Delete, KeyCode::Home] {
            assert_eq!(p.key(&a, key(c)), Act::Changed, "{c:?}");
        }
        // shift + letter: a capital in Other
        assert_eq!(p.key(&a, KeyEvent::new(KeyCode::Char('Z'), KeyModifiers::SHIFT)), Act::Changed);
        assert_eq!(p.rows(&a)[2].label, "Z");
    }

    #[test]
    fn permission_requests_answer_allow_or_deny() {
        let a = permission();
        let mut p = picker(&a);
        assert_eq!(p.rows(&a).len(), 2);
        let v = answer(press(&mut p, &a, "⏎"));
        assert_eq!((v["allow"].as_bool(), v["label"].as_str()), (Some(true), Some("allow")));
        let mut p = picker(&a);
        assert_eq!(answer(press(&mut p, &a, "↓ ⏎"))["allow"], false);
        let mut p = picker(&a);
        assert_eq!(answer(press(&mut p, &a, "2"))["label"], "deny");
        // letters: 0 Other row, 0 answer
        let mut p = picker(&a);
        assert_eq!(press(&mut p, &a, "y"), Act::Changed);
        let mut p = picker(&a);
        assert_eq!(answer(press(&mut p, &a, "↓ ␣"))["allow"], false);
    }

    #[test]
    fn acp_options_answer_with_their_option_id() {
        let a = Ask { id: "o1".into(), agent: "ada".into(), tool: "Edit".into(), input: json!({"_options": [{"id": "ok1", "name": "Allow once", "kind": "allow_once"}, {"id": "ok2", "name": "Allow always", "kind": "allow_always"}, {"id": "no", "name": "Reject", "kind": "reject_once"}]}), ts: 1 };
        let mut p = picker(&a);
        assert_eq!(p.rows(&a).iter().map(|r| r.label.as_str()).collect::<Vec<_>>(), vec!["Allow once", "Allow always", "Reject"]);
        let v = answer(press(&mut p, &a, "↑ ⏎"));
        assert_eq!((v["option"].as_str(), v["allow"].as_bool()), (Some("no"), Some(false)));
        let mut p = picker(&a);
        let v = answer(press(&mut p, &a, "2"));
        assert_eq!((v["option"].as_str(), v["allow"].as_bool()), (Some("ok2"), Some(true)));
    }

    #[test]
    fn sync_keeps_the_state_of_the_same_ask_and_resets_on_a_new_one() {
        let a = questions("q1", vec![q("Which?", "", true, &["a", "b"])]);
        let mut p = picker(&a);
        press(&mut p, &a, "↓ ␣ ⇥");
        p.sync(Some(&a));
        assert_eq!((p.at, p.focus), (1, false));
        assert!(p.rows(&a)[1].picked);
        let b = questions("q2", vec![q("Next?", "", false, &["x"])]);
        p.sync(Some(&b));
        assert_eq!((p.at, p.focus, p.q), (0, true, 0));
        assert!(p.rows(&b).iter().all(|r| !r.picked));
        p.sync(None);
        assert_eq!(p.rows_n(&b), 2);
        assert!(!p.focus);
    }

    #[test]
    fn a_sent_answer_takes_0_keys() {
        let a = questions("q1", vec![q("Color?", "", false, &["red"])]);
        let mut p = picker(&a);
        assert!(matches!(press(&mut p, &a, "⏎"), Act::Answer(_)));
        p.sent = true;
        assert_eq!(press(&mut p, &a, "↓"), Act::Pass);
        assert_eq!(p.hint(&a), "answer sent…");
    }

    #[test]
    fn clicks_toggle_multi_rows_answer_single_ones_and_focus_the_ask() {
        let a = questions("q1", vec![q("Which?", "", true, &["a", "b"])]);
        let mut p = picker(&a);
        p.focus = false;
        assert_eq!(p.click(&a, 1), Act::Changed);
        assert!(p.focus && p.rows(&a)[1].picked);
        assert_eq!(p.click(&a, 2), Act::Changed);
        assert_eq!(p.at, 2);
        let s = questions("q2", vec![q("One?", "", false, &["x", "y"])]);
        let mut p = picker(&s);
        assert_eq!(answer(p.click(&s, 1))["answers"]["One?"], "y");
    }
}
