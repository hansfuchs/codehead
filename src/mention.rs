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

use crate::agents::Agent;
use crate::cards;
use crate::dropdown;
use crate::textarea::{Pos, TextArea};
use crate::theme;

/// Rows of 1 card in the dropdown: the sprite (0 blank rows between cards).
const CARD_ROWS: u16 = crate::sprites::H as u16;

/// What a click hit.
#[derive(Clone, Debug, PartialEq)]
enum Hit {
    Card(String),
    Target(String),
}

#[derive(Default)]
pub struct Mention {
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
    preview: crate::dropdown::Preview,
}

/// Text after `@new` for a replace pick: ` (replaces @old)`.
pub fn replaces_note(old: &str) -> String {
    format!(" (replaces @{old})")
}

/// TEXT without its `(replaces @name)` notes (each with the space before it).
pub fn strip_replaces(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(i) = rest.find(" (replaces @") {
        let after = &rest[i + " (replaces @".len()..];
        let name_end = after.find(|c: char| !(c.is_alphanumeric() || matches!(c, '-' | '_' | '.'))).unwrap_or(after.len());
        if after[name_end..].starts_with(')') {
            out.push_str(&rest[..i]);
            rest = &after[name_end + 1..];
        } else {
            out.push_str(&rest[..i + 1]);
            rest = &rest[i + 1..];
        }
    }
    out.push_str(rest);
    out
}

/// Agents (indexes of AGENTS) whose name starts with Q (0 such: contains Q), in dropdown order:
/// by harness, A→Z.
fn visible(agents: &[Agent], q: &str) -> Vec<(String, Vec<usize>)> {
    let q = q.to_lowercase();
    let prefix = agents.iter().any(|a| a.name.to_lowercase().starts_with(&q));
    cards::groups(agents, |a| if prefix { a.name.to_lowercase().starts_with(&q) } else { a.name.to_lowercase().contains(&q) })
}

fn order(groups: &[(String, Vec<usize>)]) -> Vec<usize> {
    groups.iter().flat_map(|(_, g)| g.iter().copied()).collect()
}

impl Mention {
    /// Agents NAME can replace (every agent of the thread but NAME).
    fn targets(&self, name: &str) -> Vec<String> {
        self.replaceable.iter().filter(|r| *r != name).cloned().collect()
    }

    fn pick(&mut self, ta: &mut TextArea, at: Pos, name: &str, old: Option<String>) {
        let text = match &old {
            Some(o) => format!("@{name}{} ", replaces_note(o)),
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
        if self.closed.is_some() && ta.mention().map(|m| m.0) != self.closed {
            self.closed = None;
        }
    }

    /// The `@` of TA + its query. A live preview in the text: the query typed before it filters.
    /// None: 0 `@`, or closed.
    fn query(&self, ta: &TextArea) -> Option<(Pos, String)> {
        let (at, q) = ta.mention()?;
        if self.closed == Some(at) {
            return None;
        }
        let q = match self.preview.typed(ta) {
            Some((p, t)) if p == at => t.chars().skip(1).collect(),
            _ => q,
        };
        Some((at, q))
    }

    /// The open dropdown: `@` position + the matching agents (in order). None: closed.
    fn open(&self, ta: &TextArea, agents: &[Agent]) -> Option<(Pos, Vec<usize>)> {
        let (at, q) = self.query(ta)?;
        let m = order(&visible(agents, &q));
        (!m.is_empty()).then_some((at, m))
    }

    pub fn is_open(&self, ta: &TextArea, agents: &[Agent]) -> bool {
        self.open(ta, agents).is_some()
    }

    /// Key for the dropdown. true: used (the input must not see it).
    pub fn key(&mut self, k: KeyEvent, ta: &mut TextArea, agents: &[Agent]) -> bool {
        self.sync(ta);
        let Some((at, m)) = self.open(ta, agents) else {
            self.cursor = 0;
            self.replace = None;
            return false;
        };
        let plain = k.modifiers.is_empty() || k.modifiers == KeyModifiers::SHIFT;
        let n = m.len();
        self.cursor = self.cursor.min(n - 1);
        let name = agents[m[self.cursor]].name.clone();
        let targets = self.targets(&name);
        if let Some(c) = self.replace {
            let t = targets.len().max(1);
            match k.code {
                KeyCode::Up if plain => self.replace = Some((c + t - 1) % t),
                KeyCode::Down if plain => self.replace = Some((c + 1) % t),
                KeyCode::Left | KeyCode::Esc if k.modifiers.is_empty() => self.replace = None,
                KeyCode::Right if k.modifiers.is_empty() => {}
                KeyCode::Tab | KeyCode::Enter if k.modifiers.is_empty() => {
                    let old = targets.get(c.min(t - 1)).cloned();
                    self.pick(ta, at, &name, old);
                }
                _ => {
                    // typing goes on: back to add, the 1st match current again
                    self.replace = None;
                    self.cursor = 0;
                    return false;
                }
            }
            return true;
        }
        match k.code {
            KeyCode::Up | KeyCode::Down if plain => {
                self.cursor = if k.code == KeyCode::Up { (self.cursor + n - 1) % n } else { (self.cursor + 1) % n };
                // the agent's `@name` in the text at once
                let typed = match self.preview.typed(ta) {
                    Some((_, t)) => t,
                    None => ta.mention().map(|(_, q)| format!("@{q}")).unwrap_or_default(),
                };
                let name = agents[m[self.cursor]].name.clone();
                self.preview.show(ta, at, &typed, &format!("@{name}"));
            }
            KeyCode::Right if k.modifiers.is_empty() && !targets.is_empty() => self.replace = Some(0),
            KeyCode::Left if k.modifiers.is_empty() && !targets.is_empty() => {}
            KeyCode::Tab | KeyCode::Enter if k.modifiers.is_empty() => self.pick(ta, at, &name, None),
            KeyCode::Esc if k.modifiers.is_empty() => {
                self.closed = Some(at);
                self.cursor = 0;
            }
            // a name char after a preview: the typed query again, the char after it
            KeyCode::Char(c) if plain && (c.is_alphanumeric() || matches!(c, '-' | '_' | '.')) && self.preview.revert(ta) => {
                self.cursor = 0;
                return false;
            }
            _ => {
                // typing goes on: the 1st match is current again
                if matches!(k.code, KeyCode::Char(_) | KeyCode::Backspace) {
                    self.cursor = 0;
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

    /// Click on a card: insert it (add); on an agent of the replace dropdown: replace it. true: used.
    pub fn mouse(&mut self, ev: MouseEvent, ta: &mut TextArea, agents: &[Agent]) -> bool {
        if !matches!(ev.kind, MouseEventKind::Down(MouseButton::Left)) {
            return false;
        }
        let at = Position { x: ev.column, y: ev.row };
        let Some(hit) = self.rows.iter().find(|(r, _)| r.contains(at)).map(|(_, h)| h.clone()) else { return false };
        if let Some((pos, m)) = self.open(ta, agents) {
            match hit {
                Hit::Card(name) => self.pick(ta, pos, &name, None),
                Hit::Target(old) => {
                    let name = agents[m[self.cursor.min(m.len() - 1)]].name.clone();
                    self.pick(ta, pos, &name, Some(old));
                }
            }
        }
        true
    }

    /// Dropdown right above the `@` of TA (TA rendered this frame), inside BOUNDS. Below it when 0
    /// room above. Replace mode: the 2nd dropdown beside it.
    pub fn render(&mut self, f: &mut Frame, ta: &TextArea, agents: &[Agent], bounds: Rect) {
        self.rows.clear();
        self.sync(ta);
        let Some((at, q)) = self.query(ta) else { return };
        let groups = visible(agents, &q);
        let m = order(&groups);
        if m.is_empty() {
            return;
        }
        let Some(p) = ta.screen_pos(at) else { return };
        self.cursor = self.cursor.min(m.len() - 1);
        let cur = m[self.cursor];
        let text = |a: &Agent| [a.name.clone(), cards::tech(a), if self.replaceable.contains(&a.name) { "on this thread".into() } else { String::new() }];
        let text_w = cards::text_width(m.iter().flat_map(|&i| text(&agents[i])).collect::<Vec<_>>().iter());
        let card_w = cards::card_width(text_w, false) + 1;
        let g = cards::grid(&groups, card_w, CARD_ROWS, card_w);
        let toggle = !self.replaceable.is_empty();
        let w = (card_w + 3).max(if toggle { 22 } else { 16 });

        // as tall as the cards need, cut to the room of the screen
        let Some(r) = dropdown::place_max(Rect { x: p.x, y: p.y, width: 1, height: 1 }, w, g.height, usize::from(bounds.height), bounds, true) else { return };
        let view = (r.height as usize).saturating_sub(2);
        let top = g.places.iter().find(|pl| pl.0 == cur).map_or(0, |pl| pl.2);
        self.offset = cards::scroll(&g, top, CARD_ROWS, self.offset, view);
        let off = self.offset;
        let inner = dropdown::frame(f, r, "agents", "", off > 0, off + view < g.height, self.replace.is_none());
        let body = Rect { x: inner.x + 1, width: inner.width.saturating_sub(1), ..inner };
        let replace = self.replace;
        let mut cur_rect = None;
        let drawn = cards::draw_grid(f, body, &g, CARD_ROWS, off, |f, rr, i| {
            let a = &agents[i];
            let hl = i == cur;
            let card = Rect { height: CARD_ROWS, width: body.width, ..rr };
            let bg = hl.then_some(theme::pal().surface0);
            cards::draw(f, card, a, &text(a), cards::Look { cur: hl, lit: false, border: None, title: None, bg });
            if hl {
                let bar = Style::default().fg(theme::pal().accent).bg(theme::pal().on_accent);
                for y in card.y..card.bottom() {
                    f.buffer_mut()[(inner.x, y)].set_symbol(if replace.is_some() { "┃" } else { "▌" }).set_style(bar);
                }
            }
        });
        for (rr, i) in drawn {
            if i == cur {
                cur_rect = Some(rr);
            }
            self.rows.push((Rect { height: CARD_ROWS, width: body.width, ..rr }, Hit::Card(agents[i].name.clone())));
        }
        // add │ replace in the bottom border
        if toggle {
            let can = !self.targets(&agents[cur].name).is_empty();
            let on = |b: bool| if b { theme::pill().add_modifier(Modifier::BOLD) } else { theme::muted() };
            let spans = vec![
                Span::styled(" ‹ ", theme::accent()),
                Span::styled(" add ", on(replace.is_none())),
                Span::styled(" ", theme::muted()),
                Span::styled(" replace ", if can { on(replace.is_some()) } else { theme::line() }),
                Span::styled(" › ", theme::accent()),
            ];
            let lw = spans.iter().map(|s| s.content.width()).sum::<usize>() as u16;
            if r.width > lw + 2 {
                let row = Rect { x: r.right() - 1 - lw, y: r.bottom() - 1, width: lw, height: 1 };
                f.render_widget(Paragraph::new(Line::from(spans)).style(Style::default().bg(theme::pal().on_accent)), row);
            }
        }
        // the 2nd dropdown: the agents of the thread the current card can replace
        if let Some(c) = replace {
            let targets = self.targets(&agents[cur].name);
            if targets.is_empty() {
                return;
            }
            let c = c.min(targets.len() - 1);
            self.replace = Some(c);
            let tw = (targets.iter().map(|t| t.width()).max().unwrap_or(0) + 6).max(18) as u16;
            let row_y = cur_rect.map_or(r.y + 1, |cr| cr.y);
            let Some(sr) = dropdown::place_beside(r, row_y, tw, targets.len(), bounds, r) else { return };
            let shown = (sr.height as usize).saturating_sub(2);
            let soff = dropdown::fit(0, c, targets.len(), shown);
            let sin = dropdown::frame(f, sr, &format!("{} replaces", agents[cur].name), "", soff > 0, soff + shown < targets.len(), true);
            for (i, t) in targets.iter().enumerate().skip(soff).take(shown) {
                let row = Rect { y: sin.y + (i - soff) as u16, height: 1, ..sin };
                let hl = i == c;
                let color = agents.iter().find(|a| a.name == *t).map_or(theme::pal().accent, |a| crate::agents::color(a.color.as_deref()));
                let st = if hl { theme::pill().add_modifier(Modifier::BOLD) } else { Style::default().fg(theme::pal().text) };
                let dot = if hl { Style::default().fg(theme::pal().on_accent) } else { Style::default().fg(color) };
                f.render_widget(Paragraph::new(Line::from(vec![Span::styled(" ● ", dot), Span::styled(format!("@{t}"), st)])).style(st), row);
                self.rows.push((row, Hit::Target(t.clone())));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent(name: &str, harness: &str) -> Agent {
        Agent { name: name.into(), harness: harness.into(), model: None, color: None, mode: String::new(), prompt: None, last_seen: None }
    }

    fn key(m: &mut Mention, ta: &mut TextArea, agents: &[Agent], c: KeyCode) -> bool {
        m.key(KeyEvent::new(c, KeyModifiers::NONE), ta, agents)
    }

    fn screen(m: &mut Mention, ta: &mut TextArea, agents: &[Agent]) -> String {
        let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(70, 30)).unwrap();
        t.draw(|f| {
            ta.render(f, Rect::new(2, 28, 60, 1), true);
            m.render(f, ta, agents, f.area());
        })
        .unwrap();
        let b = t.backend().buffer().clone();
        let s =         (0..30).map(|y| (0..70).map(|x| b[(x, y)].symbol().to_string()).collect::<String>()).collect::<Vec<_>>().join("\n");
        if std::env::var_os("SHOW").is_some() {
            eprintln!("{s}");
        }
        s
    }

    #[test]
    fn cards_grouped_by_harness_filter_and_insert() {
        let agents = [agent("nova", "codex"), agent("amon", "claude"), agent("ramona", "claude")];
        assert_eq!(order(&visible(&agents, "mo")), [1, 2]);
        assert_eq!(order(&visible(&agents, "n")), [0]);
        assert_eq!(order(&visible(&agents, "")), [1, 2, 0]);
        let mut ta = TextArea::new(false, "");
        ta.set_text("ask @");
        ta.move_end();
        let mut m = Mention::default();
        assert!(m.is_open(&ta, &agents));
        let s = screen(&mut m, &mut ta, &agents);
        assert!(s.contains("claude · 2") && s.contains("codex · 1") && s.contains("amon") && s.contains("claude"), "{s}");
        // claude: amon, ramona; codex: nova
        assert!(key(&mut m, &mut ta, &agents, KeyCode::Down));
        assert!(key(&mut m, &mut ta, &agents, KeyCode::Down));
        assert!(key(&mut m, &mut ta, &agents, KeyCode::Tab));
        assert_eq!(ta.text(), "ask @nova ");
        assert_eq!(m.picked.take(), Some(("nova".into(), None)));
        assert!(!m.is_open(&ta, &agents));
        // ↓ previews `@ramona` in the text: a draw keeps every card, enter picks ramona
        ta.set_text("ask @");
        ta.move_end();
        assert!(key(&mut m, &mut ta, &agents, KeyCode::Down));
        let s = screen(&mut m, &mut ta, &agents);
        assert!(s.contains("nova") && s.contains("amon"), "{s}");
        assert!(key(&mut m, &mut ta, &agents, KeyCode::Enter));
        assert_eq!(ta.text(), "ask @ramona ");
        // 0 matches: closed, keys pass
        ta.insert("@zz");
        assert!(!key(&mut m, &mut ta, &agents, KeyCode::Enter));
    }

    #[test]
    fn esc_closes_until_the_at_is_gone() {
        let agents = [agent("amon", "claude")];
        let mut ta = TextArea::new(false, "");
        let mut m = Mention::default();
        ta.set_text("@");
        ta.move_end();
        assert!(key(&mut m, &mut ta, &agents, KeyCode::Esc));
        assert!(!m.is_open(&ta, &agents));
        // ⌫ deletes the `@`, a new `@` at the same place opens again
        assert!(!key(&mut m, &mut ta, &agents, KeyCode::Backspace));
        ta.key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        assert!(!key(&mut m, &mut ta, &agents, KeyCode::Char('@')));
        ta.insert("@");
        assert!(m.is_open(&ta, &agents));
        assert!(screen(&mut m, &mut ta, &agents).contains("amon"));
    }

    #[test]
    fn right_opens_replace_enter_picks_left_goes_back() {
        let agents = [agent("amon", "claude"), agent("lilith", "claude"), agent("nova", "codex")];
        let mut ta = TextArea::new(false, "");
        let mut m = Mention { replaceable: vec!["amon".into(), "lilith".into()], ..Default::default() };
        ta.set_text("x @no");
        ta.move_end();
        assert!(key(&mut m, &mut ta, &agents, KeyCode::Right));
        let s = screen(&mut m, &mut ta, &agents);
        assert!(s.contains("nova replaces") && s.contains("@amon") && s.contains("@lilith") && s.contains("replace"), "{s}");
        // ← back to add, → again, ↓ lilith, enter
        assert!(key(&mut m, &mut ta, &agents, KeyCode::Left));
        assert!(!screen(&mut m, &mut ta, &agents).contains("nova replaces"));
        assert!(key(&mut m, &mut ta, &agents, KeyCode::Right));
        assert!(key(&mut m, &mut ta, &agents, KeyCode::Down));
        assert!(key(&mut m, &mut ta, &agents, KeyCode::Enter));
        assert_eq!(ta.text(), "x @nova (replaces @lilith) ");
        assert_eq!(m.picked.take(), Some(("nova".into(), Some("lilith".into()))));
        assert!(!m.is_open(&ta, &agents));
        // amon can replace only lilith; the only agent of a thread: add only, ←→ pass
        m.replaceable = vec!["amon".into()];
        ta.set_text("x @am");
        ta.move_end();
        assert!(!key(&mut m, &mut ta, &agents, KeyCode::Right));
    }

    #[test]
    fn replace_notes_strip() {
        assert_eq!(strip_replaces("@nova (replaces @amon) go on"), "@nova go on");
        assert_eq!(strip_replaces("a (replaces @x y) b"), "a (replaces @x y) b");
        assert_eq!(strip_replaces("plain"), "plain");
    }
}
