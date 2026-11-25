//! Avatar picker of the agent form: the categories as tabs, the avatars of 1 as a grid, live.
//! Keys as in the lab: tab / shift+tab: category · ←→↑↓: pick · s: status (preview only) ·
//! f: float speed · e: eye pattern (regi) · enter: take it (with its speed, eyes) · esc: back.

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::domain::avatar::{Avatar, CATS, SPEEDS, STATUSES, regi};
use crate::ui::sprites::{self, Sprite};
use crate::ui::theme;

pub(crate) enum Act {
    None,
    Close,
    Take(Sprite),
}

pub(crate) struct Picker {
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

const CELL_W: u16 = sprites::WIDTH as u16 + 3;
const CELL_H: u16 = sprites::HEIGHT as u16 + 1;

impl Picker {
    /// Opened on the avatar of CURRENT.
    pub(crate) fn new(current: &Sprite) -> Picker {
        let avatar = current.avatar;
        let cat = CATS.iter().position(|candidate| *candidate == avatar.cat).unwrap_or(0);
        let mut picks = [0; CATS.len()];

        picks[cat] = avatar.idx;
        let speed = SPEEDS
            .iter()
            .position(|candidate| candidate.total_cmp(&avatar.speed).is_eq())
            .unwrap_or(0);

        Picker {
            cat,
            picks,
            status: 0,
            speed,
            eyes: avatar.eyes,
            per_row: 1,
            top: 0,
        }
    }

    fn avatar(&self, idx: usize) -> Avatar {
        let cat = CATS[self.cat];

        Avatar {
            cat,
            idx,
            eyes: if cat == crate::domain::avatar::Cat::Regi {
                self.eyes
            } else {
                None
            },
            speed: SPEEDS[self.speed],
        }
    }

    fn count(&self) -> usize {
        CATS[self.cat].count()
    }

    pub(crate) fn key(&mut self, key: KeyEvent) -> Act {
        let count = self.count();
        let pick = &mut self.picks[self.cat];

        match key.code {
            KeyCode::Esc => return Act::Close,
            KeyCode::Enter => return Act::Take(Sprite::of(self.avatar(self.picks[self.cat]))),
            KeyCode::Tab => self.cat = (self.cat + 1) % CATS.len(),
            KeyCode::BackTab => self.cat = (self.cat + CATS.len() - 1) % CATS.len(),
            KeyCode::Left | KeyCode::Char('h') => *pick = pick.saturating_sub(1),
            KeyCode::Right | KeyCode::Char('l') => *pick = (*pick + 1).min(count - 1),
            KeyCode::Up | KeyCode::Char('k') => *pick = pick.saturating_sub(self.per_row),
            KeyCode::Down | KeyCode::Char('j') => *pick = (*pick + self.per_row).min(count - 1),
            KeyCode::Char('s') => self.status = (self.status + 1) % STATUSES.len(),
            KeyCode::Char('f') => self.speed = (self.speed + 1) % SPEEDS.len(),
            KeyCode::Char('e') => {
                self.eyes = match self.eyes {
                    None => Some(0),
                    Some(pattern) if pattern + 1 < regi::PATTERNS.len() => Some(pattern + 1),
                    _ => None,
                }
            }
            _ => {}
        }

        Act::None
    }

    /// The picker over AREA, the avatars in COLOR.
    pub(crate) fn render(&mut self, frame: &mut Frame, area: Rect, color: Color) {
        frame.render_widget(Clear, area);
        let block = Block::new()
            .borders(Borders::ALL)
            .border_style(theme::accent())
            .title(Span::styled(" avatar ", theme::accent_bold()))
            .style(theme::text());
        let inner = block.inner(area);

        frame.render_widget(block, area);

        if inner.height < CELL_H + 4 || inner.width < CELL_W {
            return;
        }

        draw_category_tabs(frame, inner, self.cat);
        draw_selection_header(frame, inner, self);
        let pick = self.picks[self.cat];
        let status = STATUSES[self.status];
        let grid = Rect {
            y: inner.y + 4,
            height: inner.height.saturating_sub(5),
            ..inner
        };

        self.per_row = ((grid.width + 2) / CELL_W).max(1) as usize;
        let rows = (grid.height / CELL_H).max(1) as usize;
        let row = pick / self.per_row;

        if row < self.top {
            self.top = row;
        } else if row >= self.top + rows {
            self.top = row + 1 - rows;
        }

        let count = self.count();

        for index in self.top * self.per_row..count.min((self.top + rows) * self.per_row) {
            draw_avatar_cell(frame, grid, color, index, status, self);
        }

        let keys = "tab category · ←→↑↓ pick · s status · f float · e eyes · enter take · esc back";

        frame.render_widget(
            Paragraph::new(Line::styled(keys, theme::muted())),
            Rect {
                y: inner.bottom() - 1,
                height: 1,
                ..inner
            },
        );
    }
}

fn category_style(index: usize, category: usize) -> Style {
    if index == category {
        theme::bold().bg(theme::pal().surface0)
    } else {
        theme::muted()
    }
}

fn draw_category_tabs(frame: &mut Frame, inner: Rect, category: usize) {
    let mut tabs = Vec::new();

    for (index, cat) in CATS.iter().enumerate() {
        tabs.push(Span::styled(
            format!(" {} {} ", cat.label(), cat.count()),
            category_style(index, category),
        ));
        tabs.push(Span::raw(" "));
    }

    frame.render_widget(Paragraph::new(Line::from(tabs)), Rect { height: 1, ..inner });
}

fn draw_selection_header(frame: &mut Frame, inner: Rect, picker: &Picker) {
    let pick = picker.picks[picker.cat];
    let avatar = picker.avatar(pick);
    let status = STATUSES[picker.status];

    let eyes = match (CATS[picker.cat], picker.eyes) {
        (crate::domain::avatar::Cat::Regi, Some(index)) => regi::PATTERNS[index].name(),
        (crate::domain::avatar::Cat::Regi, None) => "its own",
        _ => "-",
    };

    let heading = format!("{}/{}  {}", pick + 1, picker.count(), avatar.name());

    frame.render_widget(
        Paragraph::new(Line::styled(heading, theme::bold())),
        Rect {
            y: inner.y + 1,
            height: 1,
            ..inner
        },
    );
    let toggles = format!("status {} · float {}x · eyes {}", status.name(), avatar.speed, eyes);

    frame.render_widget(
        Paragraph::new(Line::styled(toggles, theme::muted())),
        Rect {
            y: inner.y + 2,
            height: 1,
            ..inner
        },
    );
}

fn draw_avatar_cell(
    frame: &mut Frame,
    grid: Rect,
    color: Color,
    index: usize,
    status: crate::domain::avatar::Status,
    picker: &Picker,
) {
    let (column, row) = (
        (index % picker.per_row) as u16,
        (index / picker.per_row - picker.top) as u16,
    );
    let (x, y) = (grid.x + column * CELL_W, grid.y + row * CELL_H);
    let selected = index == picker.picks[picker.cat];
    let rows = picker.avatar(index).rows(status, crate::domain::avatar::now());

    let lines = rows
        .into_iter()
        .map(|row| {
            Line::styled(
                row,
                Style::default().fg(if selected { color } else { theme::pal().subtext0 }),
            )
        })
        .collect::<Vec<_>>();

    frame.render_widget(
        Paragraph::new(lines),
        Rect {
            x,
            y,
            width: sprites::WIDTH as u16,
            height: sprites::HEIGHT as u16,
        },
    );
    let label = Line::styled(
        format!("{:03}", index + 1),
        if selected {
            theme::accent_bold()
        } else {
            Style::default().fg(theme::pal().overlay0)
        },
    );

    frame.render_widget(
        Paragraph::new(label),
        Rect {
            x,
            y: y + sprites::HEIGHT as u16,
            width: sprites::WIDTH as u16,
            height: 1,
        },
    );
}
