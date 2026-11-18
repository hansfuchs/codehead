//! `@` in a prompt input: a dropdown of the agents above the `@` (below: 0 room above), filtered
//! as the name is typed. Max. 10 rows in view, ▲ / ▼ on the right border: more rows that way. ↑↓ move, tab / enter insert `@name `, esc closes it (until the next `@`). With `replaceable`
//! agents (thread view): ←→ pick the action of the row: + add, or ⇄ replace an agent of the thread.

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use crate::dropdown;
use crate::textarea::{Pos, TextArea};
use crate::theme;

#[derive(Default)]
pub struct Mention {
    cursor: usize,
    /// 1st row in view.
    offset: usize,
    /// esc on the dropdown of the `@` at this position: closed until another `@`.
    closed: Option<Pos>,
    /// Rows of the last draw (mouse).
    rows: Vec<(Rect, String)>,
    /// Agents of the thread a pick can replace. Empty: add only.
    pub replaceable: Vec<String>,
    /// Action of the current row: 0 add, n: replace the nth other agent of `replaceable`.
    action: usize,
    /// The last pick: name + the agent it replaces (None: add).
    pub picked: Option<(String, Option<String>)>,
}

/// Agent names matching Q: prefix matches first, then names that contain Q.
pub fn filter<'a>(names: &'a [(String, Color)], q: &str) -> Vec<&'a (String, Color)> {
    let q = q.to_lowercase();
    let mut out: Vec<&(String, Color)> = names.iter().filter(|(n, _)| n.to_lowercase().starts_with(&q)).collect();
    out.extend(names.iter().filter(|(n, _)| !n.to_lowercase().starts_with(&q) && n.to_lowercase().contains(&q)));
    out
}

impl Mention {
    /// Agents NAME can replace (every agent of the thread but NAME).
    fn targets(&self, name: &str) -> Vec<String> {
        self.replaceable.iter().filter(|r| *r != name).cloned().collect()
    }

    /// Label of action A for NAME: `+ add`, `⇄ replace amon`.
    fn action_label(&self, name: &str, a: usize) -> String {
        match a.checked_sub(1).and_then(|i| self.targets(name).get(i).cloned()) {
            Some(old) => format!("⇄ replace {old}"),
            None => "+ add".into(),
        }
    }

    fn pick(&mut self, ta: &mut TextArea, at: Pos, name: &str) {
        let replace = self.action.checked_sub(1).and_then(|i| self.targets(name).get(i).cloned());
        ta.replace_to_cursor(at, &format!("@{name} "));
        self.picked = Some((name.to_string(), replace));
        self.cursor = 0;
        self.action = 0;
    }

    /// The open dropdown: `@` position + matching names. None: closed.
    fn open<'a>(&self, ta: &TextArea, names: &'a [(String, Color)]) -> Option<(Pos, Vec<&'a (String, Color)>)> {
        let (at, q) = ta.mention()?;
        if self.closed == Some(at) {
            return None;
        }
        let m = filter(names, &q);
        (!m.is_empty()).then_some((at, m))
    }

    pub fn is_open(&self, ta: &TextArea, names: &[(String, Color)]) -> bool {
        self.open(ta, names).is_some()
    }

    /// Key for the dropdown. true: used (the input must not see it).
    pub fn key(&mut self, k: KeyEvent, ta: &mut TextArea, names: &[(String, Color)]) -> bool {
        let Some((at, m)) = self.open(ta, names) else {
            self.cursor = 0;
            return false;
        };
        let plain = k.modifiers.is_empty() || k.modifiers == KeyModifiers::SHIFT;
        let n = m.len();
        self.cursor = self.cursor.min(n - 1);
        let actions = 1 + self.targets(&m[self.cursor].0).len();
        match k.code {
            KeyCode::Up if plain => {
                self.cursor = (self.cursor + n - 1) % n;
                self.action = 0;
            }
            KeyCode::Down if plain => {
                self.cursor = (self.cursor + 1) % n;
                self.action = 0;
            }
            KeyCode::Left if k.modifiers.is_empty() && actions > 1 => self.action = (self.action + actions - 1) % actions,
            KeyCode::Right if k.modifiers.is_empty() && actions > 1 => self.action = (self.action + 1) % actions,
            KeyCode::Tab | KeyCode::Enter if k.modifiers.is_empty() => {
                let name = m[self.cursor].0.clone();
                self.pick(ta, at, &name);
            }
            KeyCode::Esc if k.modifiers.is_empty() => {
                self.closed = Some(at);
                self.cursor = 0;
            }
            _ => {
                // typing goes on: the 1st match is current again
                if matches!(k.code, KeyCode::Char(_) | KeyCode::Backspace) {
                    self.cursor = 0;
                    self.action = 0;
                }
                return false;
            }
        }
        true
    }


    /// Row of the last draw at P (hover).
    pub fn row_at(&self, p: Position) -> Option<Rect> {
        self.rows.iter().map(|(r, _)| *r).find(|r| r.contains(p))
    }

    /// Click on a name: insert it. true: used.
    pub fn mouse(&mut self, m: MouseEvent, ta: &mut TextArea, names: &[(String, Color)]) -> bool {
        if !matches!(m.kind, MouseEventKind::Down(MouseButton::Left)) {
            return false;
        }
        let at = Position { x: m.column, y: m.row };
        let Some(name) = self.rows.iter().find(|(r, _)| r.contains(at)).map(|(_, n)| n.clone()) else { return false };
        if let Some((pos, m)) = self.open(ta, names) {
            // a click on the current row keeps its action, other rows: add
            if m.get(self.cursor).is_none_or(|c| c.0 != name) {
                self.action = 0;
            }
            self.pick(ta, pos, &name);
        }
        true
    }

    /// Dropdown right above the `@` of TA (TA rendered this frame), inside BOUNDS. Below it when 0 room above.
    pub fn render(&mut self, f: &mut Frame, ta: &TextArea, names: &[(String, Color)], bounds: Rect) {
        self.rows.clear();
        let Some((at, m)) = self.open(ta, names) else { return };
        let Some(p) = ta.screen_pos(at) else { return };
        let n = m.len();
        self.cursor = self.cursor.min(n - 1);
        // room for the action of the current row: `‹ ⇄ replace amon ›`
        let act_w = if self.replaceable.is_empty() { 0 } else { m.iter().map(|(name, _)| (0..=self.targets(name).len()).map(|a| self.action_label(name, a).width()).max().unwrap_or(0)).max().unwrap_or(0) + 6 };
        let w = (m.iter().map(|(name, _)| name.width()).max().unwrap_or(0) + 6 + act_w).max(16) as u16;
        let Some(r) = dropdown::place(Rect { x: p.x, y: p.y, width: 1, height: 1 }, w, n, bounds, true) else { return };
        let shown = (r.height as usize).saturating_sub(2);
        self.offset = dropdown::fit(self.offset, self.cursor, n, shown);
        let off = self.offset;
        let inner = dropdown::frame(f, r, "agents", "", off > 0, off + shown < n, true);
        for (i, (name, color)) in m.iter().enumerate().skip(off).take(shown) {
            let row = Rect { y: inner.y + (i - off) as u16, height: 1, ..inner };
            let hl = i == self.cursor;
            let st = if hl { theme::pill().add_modifier(Modifier::BOLD) } else { Style::default().fg(theme::pal().text) };
            let dot = if hl { Style::default().fg(theme::pal().on_accent) } else { Style::default().fg(*color) };
            let mut spans = vec![Span::styled(" ● ", dot), Span::styled(name.clone(), st)];
            if hl && !self.replaceable.is_empty() {
                let many = !self.targets(name).is_empty();
                let act = self.action_label(name, self.action);
                let room = (row.width as usize).saturating_sub(name.width() + 3 + act.width() + 5);
                spans.push(Span::styled(" ".repeat(room), st));
                spans.push(Span::styled(if many { "‹ " } else { "  " }, st));
                spans.push(Span::styled(act, st));
                spans.push(Span::styled(if many { " › " } else { "   " }, st));
            }
            f.render_widget(Paragraph::new(Line::from(spans)).style(st), row);
            self.rows.push((row, name.clone()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dropdown_filters_and_inserts() {
        let names: Vec<(String, Color)> = ["amon", "nova", "ramona"].iter().map(|n| (n.to_string(), Color::Red)).collect();
        assert_eq!(filter(&names, "mo").iter().map(|x| x.0.as_str()).collect::<Vec<_>>(), ["amon", "ramona"]);
        assert_eq!(filter(&names, "ra").iter().map(|x| x.0.as_str()).collect::<Vec<_>>(), ["ramona"]);
        let mut ta = TextArea::new(false, "");
        ta.set_text("ask @");
        ta.move_end();
        let mut m = Mention::default();
        assert!(m.is_open(&ta, &names));
        assert!(m.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE), &mut ta, &names));
        assert!(m.key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE), &mut ta, &names));
        assert_eq!(ta.text(), "ask @nova ");
        assert!(!m.is_open(&ta, &names));
        // esc closes it for this @ only
        ta.set_text("@a");
        ta.move_end();
        assert!(m.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), &mut ta, &names));
        assert!(!m.is_open(&ta, &names));
        ta.insert(" @");
        assert!(m.is_open(&ta, &names));
        // thread view: ←→ pick replace
        m.replaceable = vec!["amon".into()];
        ta.set_text("x @no");
        ta.move_end();
        assert!(m.key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE), &mut ta, &names));
        assert!(m.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &mut ta, &names));
        assert_eq!(m.picked.take(), Some(("nova".into(), Some("amon".into()))));
        // amon can't replace amon: add only, ←→ pass to the input
        ta.set_text("x @am");
        ta.move_end();
        assert!(!m.key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE), &mut ta, &names));
        // 0 matches: closed, keys pass
        ta.insert("zz");
        assert!(!m.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &mut ta, &names));
    }
}
