//! The context menu (ctrl+., shift+F10): the actions of the thing in focus (an agent, a file, a
//! line, a comment, a commit, a PR stop, a flow step, a card). 1 list in sections (a rule and a
//! subheading between 2, e.g. the panel's "ship" steps): ↑↓ / j k move, enter runs (an outward
//! row: enter enter), 1-9 and the hint key of a row run it too, esc or the key again closes. The
//! host builds the rows per focus; a row's action is the host's (mostly the key the row stands
//! for, replayed). It opens beside or above / below the thing in focus, never over it.

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};
use unicode_width::UnicodeWidthStr;

use crate::ui::theme;

/// The context menu key: ctrl+. (kitty keyboard protocol), ⌘., shift+F10 and the menu key (any
/// terminal: shift+F10).
pub(crate) fn is_key(key: &KeyEvent) -> bool {
    match key.code {
        KeyCode::Char('.') => {
            key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::SUPER)
                && !key.modifiers.contains(KeyModifiers::ALT)
        }
        KeyCode::F(10) => key.modifiers.contains(KeyModifiers::SHIFT),
        KeyCode::Menu => true,
        _ => false,
    }
}

/// 1 row: LABEL, the key that does the same outside the menu (HINT, may be empty), ACT. A row
/// of a later SECTION sits under a rule; `ship/git`: the subheading `ship` (a rule alone between
/// 2 groups of 1 subheading). STATE: the row's state right of it (in
/// place of the hint); AGENT: an agent does it (✦); OFF: grayed, 0 run; ASKS: enter enter.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Item<A> {
    pub label: String,
    pub hint: String,
    pub act: A,
    pub section: String,
    pub state: String,
    pub agent: bool,
    pub off: bool,
    pub asks: bool,
}

impl<A> Item<A> {
    pub(crate) fn new(label: impl Into<String>, hint: impl Into<String>, act: A) -> Item<A> {
        Item {
            label: label.into(),
            hint: hint.into(),
            act,
            section: String::new(),
            state: String::new(),
            agent: false,
            off: false,
            asks: false,
        }
    }

    /// The row under the subheading S.
    pub(crate) fn in_section(mut self, text: impl Into<String>) -> Item<A> {
        self.section = text.into();
        self
    }

    pub(crate) fn map<B>(self, file: impl FnOnce(A) -> B) -> Item<B> {
        Item {
            label: self.label,
            hint: self.hint,
            act: file(self.act),
            section: self.section,
            state: self.state,
            agent: self.agent,
            off: self.off,
            asks: self.asks,
        }
    }
}

/// A row's action in a view: a key the view knows, a command with 0 key of its own, a step of
/// the view's own list (the panel's ship steps: their index).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Act {
    Key(KeyEvent),
    Cmd(&'static str),
    Step(usize),
}

/// The plain key C as a row's action.
pub(crate) fn key(character: char) -> Act {
    Act::Key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE))
}

/// Where the menu opens: never over the thing in focus.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Place {
    /// Above the row ANCHOR (below it when there is more room there).
    At(Rect),
    /// Right of PARENT (a side panel), its top at row Y; 0 room right: over the right edge.
    Beside(Rect, u16),
}

/// What a key / click did.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Out<A> {
    /// The menu stays open.
    None,
    Close,
    Run(A),
}

pub(crate) struct Menu<A> {
    pub title: String,
    /// A pill on the 1st row (the open PR: `codehead#1234`).
    pub head: Option<String>,
    pub items: Vec<Item<A>>,
    cur: usize,
    /// The 1st enter on an outward row: "enter again".
    armed: bool,
    top: usize,
    rows: Vec<(Rect, usize)>,
    area: Rect,
}

impl<A: Clone> Menu<A> {
    /// None: 0 items (the host says so in a toast).
    pub(crate) fn new(title: impl Into<String>, items: Vec<Item<A>>) -> Option<Menu<A>> {
        if items.is_empty() {
            return None;
        }

        Some(Menu {
            title: title.into(),
            head: None,
            items,
            cur: 0,
            armed: false,
            top: 0,
            rows: Vec::new(),
            area: Rect::default(),
        })
    }

    /// The rows again (a push started, a commit came): the cursor stays on its label.
    pub(crate) fn set_items(&mut self, items: Vec<Item<A>>) {
        if items.is_empty() {
            return;
        }

        let label = self.items.get(self.cur).map(|i| (i.section.clone(), i.label.clone()));

        self.cur = label
            .and_then(|(text, line)| items.iter().position(|i| i.section == text && i.label == line))
            .unwrap_or(self.cur.min(items.len() - 1));
        self.items = items;
    }

    /// Row I runs (an outward row: armed first).
    fn run(&mut self, i: usize) -> Out<A> {
        let Some(it) = self.items.get(i) else { return Out::None };

        if it.off {
            return Out::None;
        }

        if it.asks && !(self.armed && self.cur == i) {
            self.cur = i;
            self.armed = true;
            return Out::None;
        }

        Out::Run(it.act.clone())
    }

    pub(crate) fn key(&mut self, key_value: KeyEvent) -> Out<A> {
        let count = self.items.len();

        if is_key(&key_value) {
            return Out::Close;
        }

        let armed = std::mem::take(&mut self.armed);
        let ctrl = key_value.modifiers.contains(KeyModifiers::CONTROL);

        match key_value.code {
            KeyCode::Esc => Out::Close,
            KeyCode::Up | KeyCode::Char('k') if !ctrl => {
                self.cur = (self.cur + count - 1) % count;
                Out::None
            }
            KeyCode::Down | KeyCode::Char('j') if !ctrl => {
                self.cur = (self.cur + 1) % count;
                Out::None
            }
            KeyCode::Home => {
                self.cur = 0;
                Out::None
            }
            KeyCode::End => {
                self.cur = count - 1;
                Out::None
            }
            KeyCode::Enter => {
                self.armed = armed;
                self.run(self.cur)
            }
            KeyCode::Char(character @ '1'..='9')
                if key_value.modifiers.is_empty() && usize::from(character as u8 - b'1') < count =>
            {
                let i = usize::from(character as u8 - b'1');

                self.armed = armed && self.cur == i;
                self.run(i)
            }
            // the hint key of a row (`e`, `d d` → its 1st key): that row
            _ => self.key_name(key_value, armed),
        }
    }

    fn key_name(&mut self, key_value: KeyEvent, armed: bool) -> Out<A> {
        let name = crate::ui::keys::key_name(&key_value);

        match self.items.iter().position(|it| {
            !it.hint.is_empty()
                && it
                    .hint
                    .split(" · ")
                    .any(|height| height.split(' ').next() == Some(name.as_str()))
        }) {
            Some(i) => {
                self.armed = armed && self.cur == i;
                self.run(i)
            }
            None => Out::None,
        }
    }

    pub(crate) fn mouse(&mut self, event: MouseEvent) -> Out<A> {
        let at = Position {
            x: event.column,
            y: event.row,
        };

        match event.kind {
            MouseEventKind::Down(MouseButton::Left) => match self.rows.iter().find(|(result, _)| result.contains(at)) {
                Some(&(_, i)) => {
                    self.armed = self.armed && self.cur == i;
                    self.run(i)
                }
                None if self.area.contains(at) => Out::None,
                None => Out::Close,
            },
            MouseEventKind::ScrollUp => {
                self.cur = self.cur.saturating_sub(1);
                Out::None
            }
            MouseEventKind::ScrollDown => {
                self.cur = (self.cur + 1).min(self.items.len() - 1);
                Out::None
            }
            _ => Out::None,
        }
    }

    /// The lines of the box: a section's rule + subheading (None), a row (Some(i)).
    fn lines(&self) -> Vec<Option<usize>> {
        let mut out = Vec::new();

        for (i, it) in self.items.iter().enumerate() {
            self.push_section_rule(&mut out, i, it);
        }

        out
    }

    fn push_section_rule(&self, out: &mut Vec<Option<usize>>, i: usize, it: &Item<A>) {
        if i > 0 && self.items[i - 1].section != it.section {
            out.push(None);
        }

        out.push(Some(i));
    }

    /// The box at PLACE inside BOUNDS: the look of the ship menu (an accent frame, `›` on the
    /// cursor, the hint / state right, a rule with the subheading between 2 sections).
    pub(crate) fn render(&mut self, frame: &mut Frame, place: Place, bounds: Rect) {
        let lines = self.lines();

        let area_value = menu_area(self, place, bounds, lines.len());
        let top = u16::from(self.head.is_some());

        self.area = area_value;
        self.rows.clear();
        let inner = draw_frame(frame, area_value, &self.title);

        if inner.height <= top {
            return;
        }

        draw_head(frame, inner, self.head.as_deref());

        // the cursor's line in view
        let shown = usize::from(inner.height - top);
        let at = lines
            .iter()
            .position(|line_value| *line_value == Some(self.cur))
            .unwrap_or(0);

        if at < self.top {
            self.top = at.saturating_sub(usize::from(at > 0 && lines[at - 1].is_none()));
        } else if at >= self.top + shown {
            self.top = at + 1 - shown;
        }

        for (y, (li, line_2)) in (inner.y + top..).zip(lines.iter().enumerate().skip(self.top).take(shown)) {
            self.render_head(
                frame,
                &lines,
                *line_2,
                MenuLine {
                    area_value,
                    inner,
                    y,
                    li,
                },
            );
        }
    }

    fn render_head(&mut self, frame: &mut Frame, lines: &[Option<usize>], line_2: Option<usize>, settings: MenuLine) {
        let MenuLine {
            area_value,
            inner,
            y,
            li,
        } = settings;

        match line_2 {
            None => draw_separator(self, frame, lines, area_value, inner, y, li),
            Some(index) => draw_item(self, frame, inner, y, index),
        }
    }
}

#[cfg(test)]
#[path = "context_menu_tests.rs"]
mod tests;

fn menu_area<A>(menu: &Menu<A>, place: Place, bounds: Rect, line_count: usize) -> Rect {
    let right = |it: &Item<A>| {
        if it.state.is_empty() {
            it.hint.width()
        } else {
            it.state.width()
        }
    };

    let width_value = menu
        .items
        .iter()
        .map(|it| it.label.width() + right(it) + 9)
        .max()
        .unwrap_or(30)
        .max(menu.title.width() + 6)
        .clamp(30, 72) as u16;
    let width_2 = width_value.min(bounds.width);
    let top = u16::from(menu.head.is_some());
    let want = (line_count as u16 + top + 2).min(bounds.height.max(3));

    place_menu(place, bounds, width_2, want)
}
fn place_menu(place: Place, bounds: Rect, width_2: u16, want: u16) -> Rect {
    match place {
        Place::Beside(parent, y) => {
            let x = if parent.right() + width_2 <= bounds.right() {
                parent.right()
            } else {
                bounds.right().saturating_sub(width_2)
            };

            let y = y.min(bounds.bottom().saturating_sub(want)).max(bounds.y);

            Rect {
                x,
                y,
                width: width_2,
                height: want,
            }
        }
        Place::At(anchor) => {
            let up = anchor.y.saturating_sub(bounds.y);
            let down = bounds.bottom().saturating_sub(anchor.bottom());
            let height_value = want.min(up.max(down)).max(3);

            let y = if up >= want || up >= down {
                anchor.y.saturating_sub(height_value)
            } else {
                anchor.bottom()
            };

            let x = anchor.x.min(bounds.right().saturating_sub(width_2)).max(bounds.x);

            Rect {
                x,
                y,
                width: width_2,
                height: height_value,
            }
        }
    }
}
fn draw_frame(frame: &mut Frame, area_value: Rect, title: &str) -> Rect {
    frame.render_widget(Clear, area_value);
    frame.render_widget(
        Block::bordered()
            .border_style(theme::accent())
            .title(Span::styled(format!(" {title} "), theme::accent_bold()))
            .style(theme::text()),
        area_value,
    );
    Rect {
        x: area_value.x + 1,
        y: area_value.y + 1,
        width: area_value.width.saturating_sub(2),
        height: area_value.height.saturating_sub(2),
    }
}
fn draw_head(frame: &mut Frame, inner: Rect, head: Option<&str>) {
    if let Some(height_2) = head {
        let line = Line::from(Span::styled(
            format!(" {height_2} "),
            theme::pill().add_modifier(Modifier::BOLD),
        ));

        frame.render_widget(
            Paragraph::new(line),
            Rect {
                x: inner.x + 1,
                height: 1,
                width: inner.width.saturating_sub(1),
                ..inner
            },
        );
    }
}
fn draw_separator<A>(
    menu: &Menu<A>,
    frame: &mut Frame,
    lines: &[Option<usize>],
    area_value: Rect,
    inner: Rect,
    y: u16,
    li: usize,
) {
    // ├─ ship ──┤: the rule joins the border; a new subheading (`ship` of `ship/git`)
    // on it, a group of the same subheading: the rule alone
    let head = |i: Option<usize>| {
        i.map(|i| menu.items[i].section.split('/').next().unwrap_or_default().to_string())
            .unwrap_or_default()
    };

    let next = head(lines.get(li + 1).copied().flatten());

    let sec = if li > 0 && head(lines[li - 1]) == next {
        String::new()
    } else {
        next
    };

    let buf = frame.buffer_mut();

    buf[(area_value.x, y)].set_symbol("├").set_style(theme::accent());

    for x in inner.x..inner.right() {
        buf[(x, y)].set_symbol("─").set_style(theme::line());
    }

    buf[(area_value.right() - 1, y)]
        .set_symbol("┤")
        .set_style(theme::accent());

    if !sec.is_empty() {
        let thread = format!(" {sec} ");

        frame.render_widget(
            Paragraph::new(Span::styled(thread.clone(), theme::accent_bold())),
            Rect {
                x: inner.x + 1,
                y,
                width: (thread.width() as u16).min(inner.width.saturating_sub(1)),
                height: 1,
            },
        );
    }
}
fn draw_item<A>(menu: &mut Menu<A>, frame: &mut Frame, inner: Rect, y: u16, index: usize) {
    let it = &menu.items[index];
    let on = index == menu.cur;
    let armed = on && menu.armed;

    let state = if armed {
        "enter again".to_string()
    } else if it.state.is_empty() {
        it.hint.clone()
    } else {
        it.state.clone()
    };

    let state_st = if armed {
        theme::fg(theme::pal().caution()).add_modifier(Modifier::BOLD)
    } else {
        theme::muted()
    };

    let room = usize::from(inner.width).saturating_sub(5);
    let label = crate::ui::thread_card::fit(&it.label, room.saturating_sub(state.width() + 2));
    let gap = room.saturating_sub(label.width() + state.width());
    let base = if on { theme::pill_dim() } else { theme::text() };

    let (lab_st, mark_st) = if it.off {
        (base.patch(theme::muted()), base.patch(theme::muted()))
    } else {
        (
            if on { base.add_modifier(Modifier::BOLD) } else { base },
            base.patch(theme::accent()),
        )
    };

    let line = Line::from(vec![
        Span::styled(if on { "›" } else { " " }, base.patch(theme::accent())),
        Span::styled(if it.agent { " ✦ " } else { "   " }, mark_st),
        Span::styled(label, lab_st),
        Span::styled(" ".repeat(gap), base),
        Span::styled(state, base.patch(state_st)),
        Span::styled(" ", base),
    ]);
    let row = Rect { y, height: 1, ..inner };

    frame.render_widget(Paragraph::new(line), row);
    menu.rows.push((row, index));
}

#[derive(Clone, Copy)]
struct MenuLine {
    area_value: Rect,
    inner: Rect,
    y: u16,
    li: usize,
}
