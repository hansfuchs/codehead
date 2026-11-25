use super::sdf::{EYE, Px, View};

pub(crate) type Rgb = (u8, u8, u8);

pub(crate) const BG: Rgb = (21, 7, 0);
pub(crate) const INK: Rgb = (227, 210, 190);
pub(crate) const CORAL: Rgb = (255, 117, 101);
pub(crate) const MUTED: Rgb = (150, 124, 104);
pub(crate) const WARN: Rgb = (224, 170, 60);

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Cell {
    pub ch: char,
    pub fg: Rgb,
    pub bg: Option<Rgb>,
}

impl Cell {
    pub(crate) const BLANK: Cell = Cell {
        ch: ' ',
        fg: INK,
        bg: None,
    };
}

pub(crate) fn mix(agent: Rgb, bytes: Rgb, thread: f32) -> Rgb {
    let thread_value = thread.clamp(0.0, 1.0);
    let file = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * thread_value).round() as u8;

    (file(agent.0, bytes.0), file(agent.1, bytes.1), file(agent.2, bytes.2))
}

/// Color of a lit value.
pub(crate) fn color(px: &Px) -> Rgb {
    if px.mat == EYE {
        return mix(BG, CORAL, 0.25 + 0.75 * px.lum);
    }

    mix((70, 42, 30), INK, px.lum.powf(0.8))
}

pub(crate) const BAYER4: [[f32; 4]; 4] = [
    [0.0, 8.0, 2.0, 10.0],
    [12.0, 4.0, 14.0, 6.0],
    [3.0, 11.0, 1.0, 9.0],
    [15.0, 7.0, 13.0, 5.0],
];

/// Bayer threshold of dot (X, Y), 0..1.
pub(crate) fn bayer(x: usize, y: usize) -> f32 {
    (BAYER4[y % 4][x % 4] + 0.5) / 16.0
}

/// Options per render.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Opts {
    /// Lit value multiplier: fades, dissolves.
    pub gain: f32,
    /// Shift of the Bayer matrix per frame: shimmer in braille.
    pub jitter: usize,
    /// Lowest lit value of a hit: tiny renders keep their silhouette.
    pub floor: f32,
    /// Shape mode: fold between neighbor normals that cuts a gap (1 - cos), 0 = off.
    pub crease: f32,
}

impl Opts {
    pub(crate) fn new() -> Opts {
        Opts {
            gain: 1.0,
            jitter: 0,
            floor: 0.0,
            crease: 0.15,
        }
    }
}

/// The view in a W x H cell grid, row-major. A cell: ~1:2 (w:h), so 1 cell = 1/H of the height
/// in x, 2/H in y.
pub(crate) fn render(view: &View, width: usize, height: usize, output: Opts) -> Vec<Cell> {
    let at = |cx: f32, cy: f32| -> Px {
        let user = (cx - width as f32 / 2.0) / height as f32;
        let value = 1.0 - 2.0 * cy / height as f32;
        let mut px = view.sample(user, value);

        if px.hit {
            px.lum = ((output.floor + (1.0 - output.floor) * px.lum) * output.gain).clamp(0.0, 1.0);
        }

        px
    };

    let mut out = vec![Cell::BLANK; width * height];

    for y in 0..height {
        for x in 0..width {
            let (fx, fy) = (x as f32, y as f32);

            out[y * width + x] = braille_cell(output, at, y, x, fx, fy);
        }
    }

    out
}

fn braille_cell(output: Opts, at: impl Fn(f32, f32) -> Px, y: usize, x: usize, fx: f32, fy: f32) -> Cell {
    const DOT_BITS: [u32; 8] = [0x01, 0x08, 0x02, 0x10, 0x04, 0x20, 0x40, 0x80];
    let (mut bits, mut sum, mut count) = (0, 0.0, 0);
    let mut eye = Px::default();

    for (offset, bit) in DOT_BITS.iter().enumerate() {
        let (dx, dy) = (offset % 2, offset / 2);
        let pixel = at(fx + 0.25 + 0.5 * dx as f32, fy + 0.125 + 0.25 * dy as f32);

        if !pixel.hit {
            continue;
        }

        if pixel.mat == EYE {
            eye = pixel;
        }

        if pixel.lum > bayer(x * 2 + dx + output.jitter, y * 4 + dy + output.jitter / 2) {
            bits |= bit;
        }

        sum += pixel.lum;
        count += 1;
    }

    if bits == 0 {
        return Cell::BLANK;
    }

    let pixel = if eye.hit {
        eye
    } else {
        Px {
            hit: true,
            lum: 0.35 + 0.65 * sum / count as f32,
            ..Px::default()
        }
    };

    Cell {
        ch: char::from_u32(0x2800 + bits).unwrap_or(' '),
        fg: color(&pixel),
        bg: None,
    }
}

#[path = "glyph_shapes.rs"]
mod glyph_shapes;
pub(crate) use glyph_shapes::*;
