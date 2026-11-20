//! The start screen's merge tab: the owner's pull requests (prlist.rs); enter opens 1 as the
//! thread view's merge tab shows it (merge.rs), a prompt input below it. 0 thread here: the
//! prompt (and update / resolve) opens the new-thread view, the task written for the owner to
//! send.

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use ratatui::text::Span;
use ratatui::widgets::{Block, Borders};

use crate::gh::PrRef;
use crate::merge::{Act, Merge};
use crate::prlist::{Pick, PrList};
use crate::textarea::{Outcome, TextArea};
use crate::theme;

/// What the start screen does.
#[derive(Clone, Debug, PartialEq)]
pub enum Out {
    None,
    Toast(String),
    /// The new-thread view with this task.
    Compose(String),
    /// Esc: back from a PR to the list.
    Back,
}

pub struct MergeTab {
    pub list: PrList,
    detail: Option<Merge>,
    input: TextArea,
    input_focus: bool,
    input_rect: Rect,
}

impl Default for MergeTab {
    fn default() -> Self {
        let mut input = TextArea::new(false, "tell an agent what to do with this pull request: ctrl+⏎ opens a new thread");
        input.style = Some(theme::text());
        MergeTab { list: PrList::default(), detail: None, input, input_focus: false, input_rect: Rect::default() }
    }
}

impl MergeTab {
    /// The tab shows: the list read (cache, stale: GitHub).
    pub fn ensure(&mut self) {
        self.list.ensure();
    }

    /// Show PR (the menu picked 1 without a thread).
    pub fn open(&mut self, pr: &PrRef) {
        self.detail = Some(Merge::one(pr.clone()));
        self.input_focus = false;
    }

    pub fn is_detail(&self) -> bool {
        self.detail.is_some()
    }

    /// Keys typed into an input (the start screen's letter keys stay off).
    pub fn typing(&self) -> bool {
        self.input_focus || self.detail.as_ref().is_some_and(|m| m.typing())
    }

    /// True: a redraw is due.
    pub fn poll(&mut self) -> bool {
        let mut d = self.list.poll();
        if let Some(m) = self.detail.as_mut() {
            d |= m.poll(true);
        }
        d
    }

    fn task(&self, text: &str) -> String {
        let ctx = self.detail.as_ref().and_then(|m| m.context()).unwrap_or_default();
        if ctx.is_empty() { text.to_string() } else { format!("{text}\n\n{ctx}") }
    }

    fn act(&mut self, a: Act) -> Out {
        match a {
            Act::None => Out::None,
            Act::Toast(t) => Out::Toast(t),
            Act::Open(..) => Out::Toast("0 thread: 0 checkout to open".into()),
            Act::Prompt(t) => Out::Compose(t),
        }
    }

    pub fn key(&mut self, k: KeyEvent) -> Out {
        let Some(m) = self.detail.as_mut() else {
            return match self.list.key(k) {
                Pick::Open(e) => {
                    self.open(&e.item.pr);
                    Out::None
                }
                Pick::Toast(t) => Out::Toast(t),
                Pick::None => Out::None,
            };
        };
        let ctrl = k.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER);
        if self.input_focus {
            match k.code {
                KeyCode::Esc => self.input_focus = false,
                KeyCode::Enter if ctrl => {
                    let text = self.input.text();
                    if text.trim().is_empty() {
                        return Out::Toast("0 text".into());
                    }
                    self.input.set_text("");
                    self.input_focus = false;
                    return Out::Compose(self.task(text.trim()));
                }
                _ => {
                    if let Outcome::Ignored = self.input.key(k)
                        && k.code == KeyCode::Enter
                    {
                        self.input.newline();
                    }
                }
            }
            return Out::None;
        }
        if !m.typing() && k.code == KeyCode::Char('i') && !ctrl {
            self.input_focus = true;
            return Out::None;
        }
        if k.code == KeyCode::Esc && !m.takes_esc() {
            self.detail = None;
            return Out::Back;
        }
        let a = m.key(k);
        self.act(a)
    }

    pub fn mouse(&mut self, m: MouseEvent) -> Out {
        let at = Position { x: m.column, y: m.row };
        let Some(d) = self.detail.as_mut() else {
            return match self.list.mouse(m) {
                Pick::Open(e) => {
                    self.open(&e.item.pr);
                    Out::None
                }
                Pick::Toast(t) => Out::Toast(t),
                Pick::None => Out::None,
            };
        };
        if self.input_rect.contains(at) {
            if matches!(m.kind, MouseEventKind::Down(MouseButton::Left)) {
                self.input_focus = true;
            }
            self.input.mouse(m);
            return Out::None;
        }
        if matches!(m.kind, MouseEventKind::Down(MouseButton::Left)) {
            self.input_focus = false;
        }
        let a = d.mouse(m);
        self.act(a)
    }

    pub fn hints(&self) -> String {
        match &self.detail {
            None => "↑↓ move · enter: open · s: state · r: read again ".into(),
            Some(_) if self.input_focus => "ctrl+⏎: new thread with this task · esc: back to the pull request ".into(),
            Some(m) => format!("{}· esc: the list ", m.hints()),
        }
    }

    pub fn draw(&mut self, f: &mut Frame, area: Rect) {
        let Some(m) = self.detail.as_mut() else {
            self.list.draw(f, area, true);
            return;
        };
        let rows = (self.input.rows_for(area.width.saturating_sub(2)) as u16).clamp(1, 6);
        let input_h = rows + 2;
        let top = Rect { height: area.height.saturating_sub(input_h + 1), ..area };
        m.draw(f, top, !self.input_focus);
        let r = Rect { y: top.bottom() + 1, height: input_h.min(area.height), ..area };
        let edge = if self.input_focus { theme::accent() } else { theme::line() };
        let b = Block::new().borders(Borders::ALL).border_style(edge).title(Span::styled(" prompt · i ", theme::muted()));
        let inner = b.inner(r);
        f.render_widget(b, r);
        self.input.render(f, inner, self.input_focus);
        self.input_rect = inner;
    }

    /// A note of the PR view (an edit done), once.
    pub fn take_note(&mut self) -> Option<String> {
        self.detail.as_mut().and_then(|m| m.note.take())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_prompt_and_update_open_a_new_thread() {
        let mut t = MergeTab { detail: Some(crate::merge::tests::with(crate::merge::tests::pr())), ..Default::default() };
        t.key(KeyEvent::from(KeyCode::Char('i')));
        for c in "fix it".chars() {
            t.key(KeyEvent::from(KeyCode::Char(c)));
        }
        let o = t.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));
        assert!(matches!(o, Out::Compose(ref s) if s.starts_with("fix it\n\n[codehead: context from the merge tab]")), "{o:?}");
        // esc: back to the list
        assert_eq!(t.key(KeyEvent::from(KeyCode::Esc)), Out::Back);
        assert!(!t.is_detail());
    }
}
