//! The dissolve: new data in an area of the screen (a background read, a cache refresh) comes in
//! as a short morph from what was there. Only the changed cells move: bars and blocks dither over
//! by a 4x4 Bayer order, text decodes in left to right (old, noise in the accent, new). A cursor
//! move is no new data: it shows at once.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use ratatui::Frame;
use ratatui::buffer::{Buffer, Cell};
use ratatui::layout::Rect;

use crate::theme;

/// Length of 1 dissolve.
pub const TIME: Duration = Duration::from_millis(450);

/// Unix ms until which a dissolve runs somewhere: the screens draw at 30 frames per second.
static UNTIL: AtomicU64 = AtomicU64::new(0);

/// A dissolve runs: the next frame in 33 ms.
pub fn active() -> bool {
    crate::engine::now_ms() < UNTIL.load(Ordering::Relaxed)
}

fn keep_alive() {
    UNTIL.fetch_max(crate::engine::now_ms() + TIME.as_millis() as u64 + 40, Ordering::Relaxed);
}

/// The dissolve of 1 area: what it showed last, the morph under way.
#[derive(Default)]
pub struct Dissolve {
    shown: Option<Buffer>,
    morph: Option<(Buffer, Instant)>,
    /// The data generation of the last frame: another one = new data.
    version: Option<u64>,
}

impl Dissolve {
    /// AREA of F is painted with the data of generation VERSION: a new generation dissolves in from
    /// what the area showed (the 1st one, a resize: at once).
    pub fn apply(&mut self, f: &mut Frame, area: Rect, version: u64) {
        let area = area.intersection(f.area());
        if area.is_empty() {
            return;
        }
        let fresh = grab(f.buffer_mut(), area);
        if self.version != Some(version) {
            if self.version.is_some()
                && let Some(prev) = self.shown.take()
                && prev.area == fresh.area
                && prev != fresh
            {
                self.morph = Some((prev, Instant::now()));
            }
            self.version = Some(version);
        }
        let shown = match &self.morph {
            Some((from, at)) if at.elapsed() < TIME => {
                keep_alive();
                let p = at.elapsed().as_secs_f32() / TIME.as_secs_f32();
                let out = mix(from, &fresh, p);
                put(f.buffer_mut(), area, &out);
                out
            }
            _ => {
                self.morph = None;
                fresh
            }
        };
        self.shown = Some(shown);
    }
}

/// The cells of AREA of BUF, at 0, 0.
fn grab(buf: &Buffer, area: Rect) -> Buffer {
    let mut out = Buffer::empty(Rect::new(0, 0, area.width, area.height));
    for y in 0..area.height {
        for x in 0..area.width {
            if let Some(c) = buf.cell((area.x + x, area.y + y)) {
                out[(x, y)] = c.clone();
            }
        }
    }
    out
}

/// B (at 0, 0) into BUF at AREA.
pub fn put(buf: &mut Buffer, area: Rect, b: &Buffer) {
    for y in 0..area.height.min(b.area.height) {
        for x in 0..area.width.min(b.area.width) {
            if let Some(c) = buf.cell_mut((area.x + x, area.y + y)) {
                *c = b[(x, y)].clone();
            }
        }
    }
}

/// FROM → TO at P (0..1): equal cells as they are; bar cells by a 4x4 Bayer order, text cells by
/// a left-to-right decode. Buffers at 0, 0, of 1 size.
pub fn mix(from: &Buffer, to: &Buffer, p: f32) -> Buffer {
    if p >= 1.0 {
        return to.clone();
    }
    let mut out = to.clone();
    let a = to.area;
    let blank = Cell::default();
    let frame = (p * 1000.0 / 33.0) as u64;
    for y in 0..a.height {
        for x in 0..a.width {
            let new = &to[(x, y)];
            let old = from.cell((x, y)).unwrap_or(&blank);
            if old == new {
                continue;
            }
            let cell = if is_bar(old) || is_bar(new) {
                if p > BAYER[(y % 4) as usize][(x % 4) as usize] { new.clone() } else { old.clone() }
            } else {
                let at = x as f32 / a.width.max(1) as f32;
                let cyc = p * (1.0 + BAND);
                if cyc > at + BAND {
                    new.clone()
                } else if cyc > at {
                    let n = crate::avatar::regi::hash(frame * 131 + u64::from(y) * 977 + u64::from(x));
                    let mut c = new.clone();
                    c.set_char(NOISE[(n * NOISE.len() as f32) as usize % NOISE.len()]).set_style(theme::accent());
                    c
                } else {
                    old.clone()
                }
            };
            out[(x, y)] = cell;
        }
    }
    out
}

/// A cell of a bar or block (a colored blank, a block or sparkline glyph).
fn is_bar(c: &Cell) -> bool {
    let sym = c.symbol();
    (sym == " " && c.bg != ratatui::style::Color::Reset) || sym.chars().next().is_some_and(|ch| "▁▂▃▄▅▆▇█░▒▓".contains(ch))
}

const BAND: f32 = 0.15;
const NOISE: [char; 10] = ['░', '▒', '▓', '/', '\\', '|', '-', '+', '*', '#'];
/// 4x4 Bayer thresholds, 0..1.
const BAYER: [[f32; 4]; 4] = [
    [0.5 / 16.0, 8.5 / 16.0, 2.5 / 16.0, 10.5 / 16.0],
    [12.5 / 16.0, 4.5 / 16.0, 14.5 / 16.0, 6.5 / 16.0],
    [3.5 / 16.0, 11.5 / 16.0, 1.5 / 16.0, 9.5 / 16.0],
    [15.5 / 16.0, 7.5 / 16.0, 13.5 / 16.0, 5.5 / 16.0],
];

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::text::Line;
    use ratatui::widgets::Paragraph;

    fn paint(t: &mut ratatui::Terminal<ratatui::backend::TestBackend>, d: &mut Dissolve, text: &str, version: u64) -> String {
        t.draw(|f| {
            f.render_widget(Paragraph::new(Line::raw(text.to_string())), f.area());
            d.apply(f, f.area(), version);
        })
        .unwrap();
        let b = t.backend().buffer().clone();
        (0..b.area.width).map(|x| b[(x, 0)].symbol().to_string()).collect()
    }

    #[test]
    fn new_data_dissolves_in_a_cursor_move_shows_at_once() {
        let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(20, 1)).unwrap();
        let mut d = Dissolve::default();
        // the 1st paint: at once
        assert_eq!(paint(&mut t, &mut d, "old text", 1).trim_end(), "old text");
        // the same generation, other cells (a cursor move): at once
        assert_eq!(paint(&mut t, &mut d, "old text!", 1).trim_end(), "old text!");
        // a new generation: a mix, then (after TIME) the new text
        let mid = paint(&mut t, &mut d, "new words", 2);
        assert_ne!(mid.trim_end(), "new words");
        assert!(active());
        d.morph.as_mut().unwrap().1 -= TIME;
        assert_eq!(paint(&mut t, &mut d, "new words", 2).trim_end(), "new words");
        assert!(d.morph.is_none());
    }

    #[test]
    fn a_mix_moves_only_the_changed_cells() {
        let mut from = Buffer::empty(Rect::new(0, 0, 10, 1));
        let mut to = Buffer::empty(Rect::new(0, 0, 10, 1));
        from.set_string(0, 0, "ab   ", ratatui::style::Style::default());
        to.set_string(0, 0, "ab cde", ratatui::style::Style::default());
        let m = mix(&from, &to, 0.0);
        assert_eq!((m[(0, 0)].symbol(), m[(1, 0)].symbol()), ("a", "b"));
        assert_eq!(mix(&from, &to, 1.0), to);
    }
}
