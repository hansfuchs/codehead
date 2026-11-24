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

use crate::theme;

/// The context menu key: ctrl+. (kitty keyboard protocol), ⌘., shift+F10 and the menu key (any
/// terminal: shift+F10).
pub fn is_key(k: &KeyEvent) -> bool {
    match k.code {
        KeyCode::Char('.') => k.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::SUPER) && !k.modifiers.contains(KeyModifiers::ALT),
        KeyCode::F(10) => k.modifiers.contains(KeyModifiers::SHIFT),
        KeyCode::Menu => true,
        _ => false,
    }
}

/// 1 row: LABEL, the key that does the same outside the menu (HINT, may be empty), ACT. A row
/// of a later SECTION sits under a rule; `ship/git`: the subheading `ship` (a rule alone between
/// 2 groups of 1 subheading). STATE: the row's state right of it (in
/// place of the hint); AGENT: an agent does it (✦); OFF: grayed, 0 run; ASKS: enter enter.
#[derive(Clone, Debug, PartialEq)]
pub struct Item<A> {
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
    pub fn new(label: impl Into<String>, hint: impl Into<String>, act: A) -> Item<A> {
        Item { label: label.into(), hint: hint.into(), act, section: String::new(), state: String::new(), agent: false, off: false, asks: false }
    }

    /// The row under the subheading S.
    pub fn in_section(mut self, s: impl Into<String>) -> Item<A> {
        self.section = s.into();
        self
    }

    pub fn map<B>(self, f: impl FnOnce(A) -> B) -> Item<B> {
        Item { label: self.label, hint: self.hint, act: f(self.act), section: self.section, state: self.state, agent: self.agent, off: self.off, asks: self.asks }
    }
}

/// A row's action in a view: a key the view knows, a command with 0 key of its own, a step of
/// the view's own list (the panel's ship steps: their index).
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    Key(KeyEvent),
    Cmd(&'static str),
    Step(usize),
}

/// The plain key C as a row's action.
pub fn key(c: char) -> Act {
    Act::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE))
}

/// Where the menu opens: never over the thing in focus.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Place {
    /// Above the row ANCHOR (below it when there is more room there).
    At(Rect),
    /// Right of PARENT (a side panel), its top at row Y; 0 room right: over the right edge.
    Beside(Rect, u16),
}

/// What a key / click did.
#[derive(Clone, Debug, PartialEq)]
pub enum Out<A> {
    /// The menu stays open.
    None,
    Close,
    Run(A),
}

pub struct Menu<A> {
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
    pub fn new(title: impl Into<String>, items: Vec<Item<A>>) -> Option<Menu<A>> {
        if items.is_empty() {
            return None;
        }
        Some(Menu { title: title.into(), head: None, items, cur: 0, armed: false, top: 0, rows: Vec::new(), area: Rect::default() })
    }

    /// The rows again (a push started, a commit came): the cursor stays on its label.
    pub fn set_items(&mut self, items: Vec<Item<A>>) {
        if items.is_empty() {
            return;
        }
        let label = self.items.get(self.cur).map(|i| (i.section.clone(), i.label.clone()));
        self.cur = label.and_then(|(s, l)| items.iter().position(|i| i.section == s && i.label == l)).unwrap_or(self.cur.min(items.len() - 1));
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

    pub fn key(&mut self, k: KeyEvent) -> Out<A> {
        let n = self.items.len();
        if is_key(&k) {
            return Out::Close;
        }
        let armed = std::mem::take(&mut self.armed);
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        match k.code {
            KeyCode::Esc => Out::Close,
            KeyCode::Up | KeyCode::Char('k') if !ctrl => {
                self.cur = (self.cur + n - 1) % n;
                Out::None
            }
            KeyCode::Down | KeyCode::Char('j') if !ctrl => {
                self.cur = (self.cur + 1) % n;
                Out::None
            }
            KeyCode::Home => {
                self.cur = 0;
                Out::None
            }
            KeyCode::End => {
                self.cur = n - 1;
                Out::None
            }
            KeyCode::Enter => {
                self.armed = armed;
                self.run(self.cur)
            }
            KeyCode::Char(c @ '1'..='9') if k.modifiers.is_empty() && usize::from(c as u8 - b'1') < n => {
                let i = usize::from(c as u8 - b'1');
                self.armed = armed && self.cur == i;
                self.run(i)
            }
            // the hint key of a row (`e`, `d d` → its 1st key): that row
            _ => {
                let name = crate::app::key_name(&k);
                match self.items.iter().position(|it| !it.hint.is_empty() && it.hint.split(" · ").any(|h| h.split(' ').next() == Some(name.as_str()))) {
                    Some(i) => {
                        self.armed = armed && self.cur == i;
                        self.run(i)
                    }
                    None => Out::None,
                }
            }
        }
    }

    pub fn mouse(&mut self, m: MouseEvent) -> Out<A> {
        let at = Position { x: m.column, y: m.row };
        match m.kind {
            MouseEventKind::Down(MouseButton::Left) => match self.rows.iter().find(|(r, _)| r.contains(at)) {
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
            if i > 0 && self.items[i - 1].section != it.section {
                out.push(None);
            }
            out.push(Some(i));
        }
        out
    }

    /// The box at PLACE inside BOUNDS: the look of the ship menu (an accent frame, `›` on the
    /// cursor, the hint / state right, a rule with the subheading between 2 sections).
    pub fn render(&mut self, f: &mut Frame, place: Place, bounds: Rect) {
        let lines = self.lines();
        let right = |it: &Item<A>| if it.state.is_empty() { it.hint.width() } else { it.state.width() };
        let w = self.items.iter().map(|it| it.label.width() + right(it) + 9).max().unwrap_or(30).max(self.title.width() + 6).clamp(30, 72) as u16;
        let w = w.min(bounds.width);
        let top = u16::from(self.head.is_some());
        let want = (lines.len() as u16 + top + 2).min(bounds.height.max(3));
        let r = match place {
            Place::Beside(parent, y) => {
                let x = if parent.right() + w <= bounds.right() { parent.right() } else { bounds.right().saturating_sub(w) };
                let y = y.min(bounds.bottom().saturating_sub(want)).max(bounds.y);
                Rect { x, y, width: w, height: want }
            }
            Place::At(anchor) => {
                let up = anchor.y.saturating_sub(bounds.y);
                let down = bounds.bottom().saturating_sub(anchor.bottom());
                let h = want.min(up.max(down)).max(3);
                let y = if up >= want || up >= down { anchor.y.saturating_sub(h) } else { anchor.bottom() };
                let x = anchor.x.min(bounds.right().saturating_sub(w)).max(bounds.x);
                Rect { x, y, width: w, height: h }
            }
        };
        self.area = r;
        self.rows.clear();
        f.render_widget(Clear, r);
        f.render_widget(Block::bordered().border_style(theme::accent()).title(Span::styled(format!(" {} ", self.title), theme::accent_bold())).style(theme::text()), r);
        let inner = Rect { x: r.x + 1, y: r.y + 1, width: r.width.saturating_sub(2), height: r.height.saturating_sub(2) };
        if inner.height <= top {
            return;
        }
        if let Some(h) = &self.head {
            let line = Line::from(Span::styled(format!(" {h} "), theme::pill().add_modifier(Modifier::BOLD)));
            f.render_widget(Paragraph::new(line), Rect { x: inner.x + 1, height: 1, width: inner.width.saturating_sub(1), ..inner });
        }
        // the cursor's line in view
        let shown = usize::from(inner.height - top);
        let at = lines.iter().position(|l| *l == Some(self.cur)).unwrap_or(0);
        if at < self.top {
            self.top = at.saturating_sub(usize::from(at > 0 && lines[at - 1].is_none()));
        } else if at >= self.top + shown {
            self.top = at + 1 - shown;
        }
        for (y, (li, l)) in (inner.y + top..).zip(lines.iter().enumerate().skip(self.top).take(shown)) {
            match *l {
                None => {
                    // ├─ ship ──┤: the rule joins the border; a new subheading (`ship` of `ship/git`)
                    // on it, a group of the same subheading: the rule alone
                    let head = |i: Option<usize>| i.map(|i| self.items[i].section.split('/').next().unwrap_or_default().to_string()).unwrap_or_default();
                    let next = head(lines.get(li + 1).copied().flatten());
                    let sec = if li > 0 && head(lines[li - 1]) == next { String::new() } else { next };
                    let buf = f.buffer_mut();
                    buf[(r.x, y)].set_symbol("├").set_style(theme::accent());
                    for x in inner.x..inner.right() {
                        buf[(x, y)].set_symbol("─").set_style(theme::line());
                    }
                    buf[(r.right() - 1, y)].set_symbol("┤").set_style(theme::accent());
                    if !sec.is_empty() {
                        let t = format!(" {sec} ");
                        f.render_widget(Paragraph::new(Span::styled(t.clone(), theme::accent_bold())), Rect { x: inner.x + 1, y, width: (t.width() as u16).min(inner.width.saturating_sub(1)), height: 1 });
                    }
                }
                Some(i) => {
                    let it = &self.items[i];
                    let on = i == self.cur;
                    let armed = on && self.armed;
                    let state = if armed { "enter again".to_string() } else if it.state.is_empty() { it.hint.clone() } else { it.state.clone() };
                    let state_st = if armed { theme::fg(theme::pal().caution()).add_modifier(Modifier::BOLD) } else { theme::muted() };
                    let room = usize::from(inner.width).saturating_sub(5);
                    let label = crate::nav::fit(&it.label, room.saturating_sub(state.width() + 2));
                    let gap = room.saturating_sub(label.width() + state.width());
                    let base = if on { theme::pill_dim() } else { theme::text() };
                    let (lab_st, mark_st) = if it.off { (base.patch(theme::muted()), base.patch(theme::muted())) } else { (if on { base.add_modifier(Modifier::BOLD) } else { base }, base.patch(theme::accent())) };
                    let line = Line::from(vec![
                        Span::styled(if on { "›" } else { " " }, base.patch(theme::accent())),
                        Span::styled(if it.agent { " ✦ " } else { "   " }, mark_st),
                        Span::styled(label, lab_st),
                        Span::styled(" ".repeat(gap), base),
                        Span::styled(state, base.patch(state_st)),
                        Span::styled(" ", base),
                    ]);
                    let row = Rect { y, height: 1, ..inner };
                    f.render_widget(Paragraph::new(line), row);
                    self.rows.push((row, i));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(code: KeyCode, m: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, m)
    }

    #[test]
    fn the_key_is_ctrl_dot_shift_f10_or_the_menu_key() {
        assert!(is_key(&k(KeyCode::Char('.'), KeyModifiers::CONTROL)));
        assert!(is_key(&k(KeyCode::Char('.'), KeyModifiers::SUPER)));
        assert!(is_key(&k(KeyCode::F(10), KeyModifiers::SHIFT)));
        assert!(is_key(&k(KeyCode::Menu, KeyModifiers::NONE)));
        assert!(!is_key(&k(KeyCode::Char('.'), KeyModifiers::NONE)));
        assert!(!is_key(&k(KeyCode::F(10), KeyModifiers::NONE)));
    }

    #[test]
    fn enter_a_digit_or_a_hint_key_runs_a_row() {
        let items = vec![Item::new("open in the IDE", "e", 1), Item::new("delete", "d d", 2), Item::new("resolve", "", 3)];
        let mut m = Menu::new("file", items).expect("items");
        assert_eq!(m.key(k(KeyCode::Down, KeyModifiers::NONE)), Out::None);
        assert_eq!(m.key(k(KeyCode::Enter, KeyModifiers::NONE)), Out::Run(2));
        assert_eq!(m.key(k(KeyCode::Char('3'), KeyModifiers::NONE)), Out::Run(3));
        assert_eq!(m.key(k(KeyCode::Char('e'), KeyModifiers::NONE)), Out::Run(1));
        assert_eq!(m.key(k(KeyCode::Char('d'), KeyModifiers::NONE)), Out::Run(2));
        assert_eq!(m.key(k(KeyCode::Char('z'), KeyModifiers::NONE)), Out::None);
        assert_eq!(m.key(k(KeyCode::Esc, KeyModifiers::NONE)), Out::Close);
        assert_eq!(m.key(k(KeyCode::Char('.'), KeyModifiers::CONTROL)), Out::Close);
        assert!(Menu::<u8>::new("x", Vec::new()).is_none());
    }

    #[test]
    fn an_outward_row_asks_a_2nd_enter_a_grayed_one_never_runs() {
        let mut push = Item::new("push", "", 1).in_section("ship");
        push.asks = true;
        let mut off = Item::new("merge", "", 2).in_section("ship");
        off.off = true;
        let mut m = Menu::new("panel", vec![Item::new("commit message", "m", 0), push, off]).expect("items");
        let enter = k(KeyCode::Enter, KeyModifiers::NONE);
        m.key(k(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(m.key(enter), Out::None, "1st enter: armed");
        assert_eq!(m.key(enter), Out::Run(1));
        m.key(k(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(m.key(enter), Out::None, "grayed");
        // a move between disarms
        m.key(k(KeyCode::Up, KeyModifiers::NONE));
        m.key(enter);
        m.key(k(KeyCode::Down, KeyModifiers::NONE));
        m.key(k(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(m.key(enter), Out::None);
        assert_eq!(m.lines(), vec![Some(0), None, Some(1), Some(2)], "a rule before the ship section");
    }

    #[test]
    fn beside_opens_right_of_the_panel_at_opens_above_its_row() {
        let mut m = Menu::new("x", vec![Item::new("a", "", 0), Item::new("b", "", 1)]).expect("items");
        let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 30)).unwrap();
        let panel = Rect { x: 0, y: 2, width: 30, height: 26 };
        t.draw(|f| m.render(f, Place::Beside(panel, 10), f.area())).unwrap();
        assert_eq!((m.area.x, m.area.y), (30, 10));
        let row = Rect { x: 40, y: 20, width: 60, height: 1 };
        t.draw(|f| m.render(f, Place::At(row), f.area())).unwrap();
        assert!(m.area.bottom() <= 20 && m.area.x == 40, "{:?}", m.area);
        let row = Rect { x: 40, y: 1, width: 60, height: 1 };
        t.draw(|f| m.render(f, Place::At(row), f.area())).unwrap();
        assert!(m.area.y >= 2, "0 room above: below the row {:?}", m.area);
    }
}
