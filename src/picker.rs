//! Avatar picker of the agent form: the categories as tabs, the avatars of 1 as a grid, live.
//! Keys as in the lab: tab / shift+tab: category · ←→↑↓: pick · s: status (preview only) ·
//! f: float speed · e: eye pattern (regi) · enter: take it (with its speed, eyes) · esc: back.

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::avatar::{Avatar, CATS, SPEEDS, STATUSES, regi};
use crate::sprites::{self, Sprite};
use crate::theme;

pub enum Act {
    None,
    Close,
    Take(Sprite),
}

pub struct Picker {
    cat: usize,
    /// The pick per category.
    picks: [usize; CATS.len()],
    status: usize,
    speed: usize,
    eyes: Option<usize>,
    per_row: usize,
    /// Row of the grid in view at the top.
    top: usize,
}

const CELL_W: u16 = sprites::W as u16 + 3;
const CELL_H: u16 = sprites::H as u16 + 1;

impl Picker {
    /// Opened on the avatar of CURRENT.
    pub fn new(current: &Sprite) -> Picker {
        let a = current.avatar;
        let cat = CATS.iter().position(|c| *c == a.cat).unwrap_or(0);
        let mut picks = [0; CATS.len()];
        picks[cat] = a.idx;
        let speed = SPEEDS.iter().position(|s| *s == a.speed).unwrap_or(0);
        Picker { cat, picks, status: 0, speed, eyes: a.eyes, per_row: 1, top: 0 }
    }

    fn avatar(&self, idx: usize) -> Avatar {
        let cat = CATS[self.cat];
        Avatar { cat, idx, eyes: if cat == crate::avatar::Cat::Regi { self.eyes } else { None }, speed: SPEEDS[self.speed] }
    }

    fn count(&self) -> usize {
        CATS[self.cat].count()
    }

    pub fn key(&mut self, k: KeyEvent) -> Act {
        let n = self.count();
        let pick = &mut self.picks[self.cat];
        match k.code {
            KeyCode::Esc => return Act::Close,
            KeyCode::Enter => return Act::Take(Sprite::of(self.avatar(self.picks[self.cat]))),
            KeyCode::Tab => self.cat = (self.cat + 1) % CATS.len(),
            KeyCode::BackTab => self.cat = (self.cat + CATS.len() - 1) % CATS.len(),
            KeyCode::Left | KeyCode::Char('h') => *pick = pick.saturating_sub(1),
            KeyCode::Right | KeyCode::Char('l') => *pick = (*pick + 1).min(n - 1),
            KeyCode::Up | KeyCode::Char('k') => *pick = pick.saturating_sub(self.per_row),
            KeyCode::Down | KeyCode::Char('j') => *pick = (*pick + self.per_row).min(n - 1),
            KeyCode::Char('s') => self.status = (self.status + 1) % STATUSES.len(),
            KeyCode::Char('f') => self.speed = (self.speed + 1) % SPEEDS.len(),
            KeyCode::Char('e') => {
                self.eyes = match self.eyes {
                    None => Some(0),
                    Some(k) if k + 1 < regi::PATTERNS.len() => Some(k + 1),
                    _ => None,
                }
            }
            _ => {}
        }
        Act::None
    }

    /// The picker over AREA, the avatars in COLOR.
    pub fn render(&mut self, f: &mut Frame, area: Rect, color: Color) {
        f.render_widget(Clear, area);
        let block = Block::new().borders(Borders::ALL).border_style(theme::accent()).title(Span::styled(" avatar ", theme::accent_bold())).style(theme::text());
        let inner = block.inner(area);
        f.render_widget(block, area);
        if inner.height < CELL_H + 4 || inner.width < CELL_W {
            return;
        }
        // tabs
        let mut tabs = vec![];
        for (i, c) in CATS.iter().enumerate() {
            let st = if i == self.cat { theme::bold().bg(theme::pal().surface0) } else { theme::muted() };
            tabs.push(Span::styled(format!(" {} {} ", c.label(), c.count()), st));
            tabs.push(Span::raw(" "));
        }
        f.render_widget(Paragraph::new(Line::from(tabs)), Rect { height: 1, ..inner });
        // the pick: its name and the toggles
        let pick = self.picks[self.cat];
        let a = self.avatar(pick);
        let status = STATUSES[self.status];
        let eyes = match (CATS[self.cat], self.eyes) {
            (crate::avatar::Cat::Regi, Some(k)) => regi::PATTERNS[k].name(),
            (crate::avatar::Cat::Regi, None) => "its own",
            _ => "-",
        };
        let head = format!("{}/{}  {}", pick + 1, self.count(), a.name());
        f.render_widget(Paragraph::new(Line::styled(head, theme::bold())), Rect { y: inner.y + 1, height: 1, ..inner });
        let toggles = format!("status {} · float {}x · eyes {}", status.name(), a.speed, eyes);
        f.render_widget(Paragraph::new(Line::styled(toggles, theme::muted())), Rect { y: inner.y + 2, height: 1, ..inner });
        // the grid
        let grid = Rect { y: inner.y + 4, height: inner.height.saturating_sub(5), ..inner };
        self.per_row = ((grid.width + 2) / CELL_W).max(1) as usize;
        let rows = (grid.height / CELL_H).max(1) as usize;
        let row = pick / self.per_row;
        if row < self.top {
            self.top = row;
        } else if row >= self.top + rows {
            self.top = row + 1 - rows;
        }
        let n = self.count();
        for i in self.top * self.per_row..n.min((self.top + rows) * self.per_row) {
            let (c, r) = ((i % self.per_row) as u16, (i / self.per_row - self.top) as u16);
            let (x, y) = (grid.x + c * CELL_W, grid.y + r * CELL_H);
            let sel = i == pick;
            let rows = self.avatar(i).rows(status, crate::avatar::now());
            let lines: Vec<Line> = rows.into_iter().map(|r| Line::styled(r, Style::default().fg(if sel { color } else { theme::pal().subtext0 }))).collect();
            f.render_widget(Paragraph::new(lines), Rect { x, y, width: sprites::W as u16, height: sprites::H as u16 });
            let label = Line::styled(format!("{:03}", i + 1), if sel { theme::accent_bold() } else { Style::default().fg(theme::pal().overlay0) });
            f.render_widget(Paragraph::new(label), Rect { x, y: y + sprites::H as u16, width: sprites::W as u16, height: 1 });
        }
        let keys = "tab category · ←→↑↓ pick · s status · f float · e eyes · enter take · esc back";
        f.render_widget(Paragraph::new(Line::styled(keys, theme::muted())), Rect { y: inner.bottom() - 1, height: 1, ..inner });
    }
}

