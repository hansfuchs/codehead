//! The dissolve: new data in an area of the screen (a background read, a cache refresh) comes in
//! as a short morph from what was there. Only the changed cells move: bars and blocks dither over
//! by a 4x4 Bayer order, text decodes in left to right (old, noise in the accent, new). A cursor
//! move is no new data: it shows at once.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use ratatui::Frame;
use ratatui::buffer::{Buffer, Cell};
use ratatui::layout::Rect;

use crate::ui::theme;

/// Length of 1 dissolve.
pub(crate) const TIME: Duration = Duration::from_millis(450);

/// Unix ms until which a dissolve runs somewhere: the screens draw at 30 frames per second.
static UNTIL: AtomicU64 = AtomicU64::new(0);

/// A dissolve runs: the next frame in 33 ms.
pub(crate) fn active() -> bool {
    crate::engine::now_ms() < UNTIL.load(Ordering::Relaxed)
}

fn keep_alive() {
    UNTIL.fetch_max(
        crate::engine::now_ms() + TIME.as_millis() as u64 + 40,
        Ordering::Relaxed,
    );
}

/// The dissolve of 1 area: what it showed last, the morph under way.
#[derive(Default)]
pub(crate) struct Dissolve {
    shown: Option<Buffer>,
    morph: Option<(Buffer, Instant)>,
    /// The data generation of the last frame: another one = new data.
    version: Option<u64>,
}

impl Dissolve {
    /// AREA of FRAME is painted with the data of generation VERSION: a new generation dissolves in from
    /// what the area showed (the 1st one, a resize: at once).
    pub(crate) fn apply(&mut self, frame: &mut Frame, area: Rect, version: u64) {
        apply_transition(self, frame, area, version);
    }
}

/// The cells of AREA of BUF, at 0, 0.
fn grab(buf: &Buffer, area: Rect) -> Buffer {
    let mut out = Buffer::empty(Rect::new(0, 0, area.width, area.height));

    for y in 0..area.height {
        grab_row(buf, area, &mut out, y);
    }

    out
}

fn grab_row(buf: &Buffer, area: Rect, out: &mut Buffer, y: u16) {
    for x in 0..area.width {
        if let Some(cell) = buf.cell((area.x + x, area.y + y)) {
            out[(x, y)] = cell.clone();
        }
    }
}

/// SRC (at 0, 0) into BUF at AREA.
pub(crate) fn put(buf: &mut Buffer, area: Rect, src: &Buffer) {
    for y in 0..area.height.min(src.area.height) {
        put_row(buf, area, src, y);
    }
}

/// FROM → TO at PROGRESS (0..1): equal cells as they are; bar cells by a 4x4 Bayer order, text cells by
/// a left-to-right decode. Buffers at 0, 0, of 1 size.
pub(crate) fn mix(from: &Buffer, to: &Buffer, progress: f32) -> Buffer {
    if progress >= 1.0 {
        return to.clone();
    }

    let mut out = to.clone();
    let bounds = to.area;
    let blank = Cell::default();
    let frame = (progress * 1000.0 / 33.0) as u64;

    for y in 0..bounds.height {
        mix_row(
            from,
            to,
            &mut out,
            y,
            DissolveFrame {
                progress,
                bounds,
                blank: &blank,
                frame,
            },
        );
    }

    out
}

fn mix_row(from: &Buffer, to: &Buffer, out: &mut Buffer, y: u16, settings: DissolveFrame<'_>) {
    let DissolveFrame {
        progress,
        bounds,
        blank,
        frame,
    } = settings;

    for x in 0..bounds.width {
        mix_cell(
            from,
            to,
            out,
            y,
            x,
            DissolveCell {
                progress,
                bounds,
                blank,
                frame,
            },
        );
    }
}

fn mix_cell(from: &Buffer, to: &Buffer, out: &mut Buffer, y: u16, x: u16, settings: DissolveCell<'_>) {
    let DissolveCell {
        progress,
        bounds,
        blank,
        frame,
    } = settings;

    let new = &to[(x, y)];
    let old = from.cell((x, y)).unwrap_or(blank);

    if old == new {
        return;
    }

    let cell = if is_bar(old) || is_bar(new) {
        if progress > BAYER[(y % 4) as usize][(x % 4) as usize] {
            new.clone()
        } else {
            old.clone()
        }
    } else {
        let at = f32::from(x) / f32::from(bounds.width.max(1));
        let cyc = progress * (1.0 + BAND);

        if cyc > at + BAND {
            new.clone()
        } else if cyc > at {
            let pick = crate::domain::avatar::regi::hash(frame * 131 + u64::from(y) * 977 + u64::from(x));
            let mut noisy = new.clone();

            noisy
                .set_char(NOISE[(pick * NOISE.len() as f32) as usize % NOISE.len()])
                .set_style(theme::accent());
            noisy
        } else {
            old.clone()
        }
    };

    out[(x, y)] = cell;
}

/// A cell of a bar or block (a colored blank, a block or sparkline glyph).
fn is_bar(cell: &Cell) -> bool {
    let sym = cell.symbol();

    (sym == " " && cell.bg != ratatui::style::Color::Reset)
        || sym.chars().next().is_some_and(|ch| "▁▂▃▄▅▆▇█░▒▓".contains(ch))
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
#[path = "dissolve_tests.rs"]
mod tests;

fn apply_transition(dissolve: &mut Dissolve, frame: &mut Frame, area: Rect, version: u64) {
    let area = area.intersection(frame.area());

    if area.is_empty() {
        return;
    }

    let fresh = grab(frame.buffer_mut(), area);

    if dissolve.version != Some(version) {
        if dissolve.version.is_some()
            && let Some(prev) = dissolve.shown.take()
            && prev.area == fresh.area
            && prev != fresh
        {
            dissolve.morph = Some((prev, Instant::now()));
        }

        dissolve.version = Some(version);
    }

    let shown = match &dissolve.morph {
        Some((from, at)) if at.elapsed() < TIME => {
            keep_alive();
            let progress = at.elapsed().as_secs_f32() / TIME.as_secs_f32();
            let out = mix(from, &fresh, progress);

            put(frame.buffer_mut(), area, &out);
            out
        }
        _ => {
            dissolve.morph = None;
            fresh
        }
    };

    dissolve.shown = Some(shown);
}

#[derive(Clone, Copy)]
struct DissolveFrame<'context> {
    progress: f32,
    bounds: Rect,
    blank: &'context Cell,
    frame: u64,
}

#[derive(Clone, Copy)]
struct DissolveCell<'context> {
    progress: f32,
    bounds: Rect,
    blank: &'context Cell,
    frame: u64,
}

fn put_row(buf: &mut Buffer, area: Rect, src: &Buffer, y: u16) {
    for x in 0..area.width.min(src.area.width) {
        if let Some(cell) = buf.cell_mut((area.x + x, area.y + y)) {
            *cell = src[(x, y)].clone();
        }
    }
}
