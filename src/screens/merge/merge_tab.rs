//! The start screen's merge tab: the owner's pull requests (prlist.rs); enter opens 1 as the
//! thread view's merge tab shows it (merge.rs), a prompt input below it. 0 thread here: the
//! prompt (and update / resolve) opens the new-thread view, the task written for the owner to
//! send.

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use ratatui::text::Span;
use ratatui::widgets::{Block, Borders};

use crate::infra::gh::PrRef;
use crate::screens::merge::pr_list::{Pick, PrList};
use crate::screens::merge::{Act, Merge};
use crate::ui::textarea::{Outcome, TextArea};
use crate::ui::theme;

/// What the start screen does.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Out {
    None,
    Toast(String),
    /// The new-thread view with this task.
    Compose(String),
    /// Esc: back from a PR to the list.
    Back,
}

pub(crate) struct MergeTab {
    pub list: PrList,
    detail: Option<Merge>,
    input: TextArea,
    pub(crate) input_focus: bool,
    input_rect: Rect,
}

impl Default for MergeTab {
    fn default() -> Self {
        let mut input = TextArea::new(
            false,
            "tell an agent what to do with this pull request: ctrl+⏎ opens a new thread",
        );

        input.style = Some(theme::text());
        MergeTab {
            list: PrList::default(),
            detail: None,
            input,
            input_focus: false,
            input_rect: Rect::default(),
        }
    }
}

impl MergeTab {
    /// The tab shows: the list read (cache, stale: GitHub).
    pub(crate) fn ensure(&mut self) {
        self.list.ensure();
    }

    /// Show PR (the menu picked 1 without a thread).
    pub(crate) fn open(&mut self, pr: &PrRef) {
        self.detail = Some(Merge::one(pr.clone()));
        self.input_focus = false;
    }

    pub(crate) fn is_detail(&self) -> bool {
        self.detail.is_some()
    }

    /// Keys typed into an input (the start screen's letter keys stay off).
    pub(crate) fn typing(&self) -> bool {
        self.input_focus || self.detail.as_ref().is_some_and(super::Merge::typing)
    }

    /// True: a redraw is due.
    pub(crate) fn poll(&mut self) -> bool {
        let mut dirty = self.list.poll();

        if let Some(detail) = self.detail.as_mut() {
            dirty |= detail.poll(true);
        }

        dirty
    }

    fn task(&self, text: &str) -> String {
        let ctx = self.detail.as_ref().and_then(super::Merge::context).unwrap_or_default();

        if ctx.is_empty() {
            text.to_string()
        } else {
            format!("{text}\n\n{ctx}")
        }
    }

    fn act(act: Act) -> Out {
        match act {
            Act::None => Out::None,
            Act::Toast(toast) => Out::Toast(toast),
            Act::Open(..) => Out::Toast("0 thread: 0 checkout to open".into()),
            Act::Prompt(prompt) => Out::Compose(prompt),
        }
    }

    pub(crate) fn key(&mut self, key: KeyEvent) -> Out {
        dispatch_tab_key(self, key)
    }

    pub(crate) fn mouse(&mut self, event: MouseEvent) -> Out {
        dispatch_tab_mouse(self, event)
    }

    pub(crate) fn hints(&self) -> String {
        match &self.detail {
            None => "←→ state · ↑↓ move · enter: open · r: read again ".into(),
            Some(_) if self.input_focus => {
                "ctrl+⏎: new thread with this task · tab / esc: back to the pull request ".into()
            }
            Some(detail) => format!("{}· esc: the list ", detail.hints()),
        }
    }

    pub(crate) fn draw(&mut self, frame: &mut Frame, area: Rect) {
        let Some(detail) = self.detail.as_mut() else {
            self.list.draw(frame, area, true);
            return;
        };

        let rows = (self.input.rows_for(area.width.saturating_sub(2)) as u16).clamp(1, 6);
        let input_height = rows + 2;
        let top = Rect {
            height: area.height.saturating_sub(input_height + 1),
            ..area
        };

        detail.draw(frame, top, !self.input_focus);
        let input_area = Rect {
            y: top.bottom() + 1,
            height: input_height.min(area.height),
            ..area
        };

        let edge = if self.input_focus {
            theme::accent()
        } else {
            theme::line()
        };

        let block = Block::new()
            .borders(Borders::ALL)
            .border_style(edge)
            .title(Span::styled(" prompt · i ", theme::muted()));
        let inner = block.inner(input_area);

        frame.render_widget(block, input_area);
        self.input.render(frame, inner, self.input_focus);
        self.input_rect = inner;
    }

    /// A note of the PR view (an edit done), once.
    pub(crate) fn take_note(&mut self) -> Option<String> {
        self.detail.as_mut().and_then(|detail| detail.note.take())
    }
}

#[cfg(test)]
#[path = "merge_tab_tests.rs"]
mod tests;

fn dispatch_tab_key(tab: &mut MergeTab, key: KeyEvent) -> Out {
    let Some(detail) = tab.detail.as_mut() else {
        return match tab.list.key(key) {
            Pick::Open(entry) => {
                tab.open(&entry.item.pr);
                Out::None
            }
            Pick::Toast(toast) => Out::Toast(toast),
            Pick::None => Out::None,
        };
    };

    let ctrl = key
        .modifiers
        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER);

    if tab.input_focus {
        return dispatch_prompt_key(tab, key, ctrl);
    }

    if key.code == KeyCode::Esc && !detail.takes_esc() {
        tab.detail = None;
        return Out::Back;
    }

    let act = detail.key(key);

    MergeTab::act(act)
}

fn dispatch_prompt_key(tab: &mut MergeTab, key: KeyEvent, ctrl: bool) -> Out {
    match key.code {
        // esc: back to the pull request (tab: the next region, the start screen's)
        KeyCode::Esc => tab.input_focus = false,
        KeyCode::Enter if ctrl => {
            let text = tab.input.text();

            if text.trim().is_empty() {
                return Out::Toast("0 text".into());
            }

            tab.input.set_text("");
            tab.input_focus = false;
            return Out::Compose(tab.task(text.trim()));
        }
        _ => {
            if let Outcome::Ignored = tab.input.key(key)
                && key.code == KeyCode::Enter
            {
                tab.input.newline();
            }
        }
    }

    Out::None
}

fn dispatch_tab_mouse(tab: &mut MergeTab, event: MouseEvent) -> Out {
    let at = Position {
        x: event.column,
        y: event.row,
    };

    let Some(detail) = tab.detail.as_mut() else {
        return match tab.list.mouse(event) {
            Pick::Open(entry) => {
                tab.open(&entry.item.pr);
                Out::None
            }
            Pick::Toast(toast) => Out::Toast(toast),
            Pick::None => Out::None,
        };
    };

    if tab.input_rect.contains(at) {
        if matches!(event.kind, MouseEventKind::Down(MouseButton::Left)) {
            tab.input_focus = true;
        }

        tab.input.mouse(event);
        return Out::None;
    }

    if matches!(event.kind, MouseEventKind::Down(MouseButton::Left)) {
        tab.input_focus = false;
    }

    let act = detail.mouse(event);

    MergeTab::act(act)
}
