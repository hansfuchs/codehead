use super::{Cell, EYE, Opts, Px, View, color};

const DOT_BITS: [u32; 8] = [0x01, 0x08, 0x02, 0x10, 0x04, 0x20, 0x40, 0x80];

/// Braille outline of the view in a W x H cell grid (2x4 dots per cell): only the edges on, the
/// areas empty (the regi look). A dot is on when it hits and: a 4-neighbor misses (the
/// silhouette), or a neighbor sits across a fold (normals apart by more than O.crease, min.
/// 0.2; the nearer of the 2 on), or it is the near side of a depth step. Eye dots: all on. Then lone dots and 1-dot spurs out.
pub(crate) fn outline(view: &View, width: usize, height: usize, opts: Opts) -> Vec<Cell> {
    let mut outline = Outline::new(view, width, height);
    let fold = 1.0 - opts.crease.max(0.2);

    for index in 0..outline.pixels.len() {
        outline.dot(index, fold);
    }

    crate::domain::avatar::regi::clean_in(&mut outline.dots, outline.width, outline.height, true);
    (0..width * height)
        .map(|index| outline_cell(index % width, index / width, &outline))
        .collect()
}

struct Outline {
    width: usize,
    height: usize,
    pixels: Vec<Px>,
    dots: Vec<u8>,
}

impl Outline {
    fn new(view: &View, width: usize, height: usize) -> Self {
        let dot_width = width * 2;
        let dot_height = height * 4;

        let pixels = (0..dot_width * dot_height)
            .map(|index| {
                let (x, y) = ((index % dot_width) as f32, (index / dot_width) as f32);
                let screen_x = (f32::midpoint(x, 0.5) - width as f32 / 2.0) / height as f32;
                let screen_y = 1.0 - 2.0 * ((y + 0.5) / 4.0) / height as f32;

                view.sample(screen_x, screen_y)
            })
            .collect();

        Self {
            width: dot_width,
            height: dot_height,
            pixels,
            dots: vec![0; dot_width * dot_height],
        }
    }

    fn hit(&self, x: i32, y: i32) -> bool {
        x >= 0
            && y >= 0
            && (x as usize) < self.width
            && (y as usize) < self.height
            && self.pixels[y as usize * self.width + x as usize].hit
    }

    fn dot(&mut self, index: usize, fold: f32) {
        let pixel = &self.pixels[index];

        if !pixel.hit {
            return;
        }

        if pixel.mat == EYE {
            self.dots[index] = 2;
            return;
        }

        let (x, y) = ((index % self.width) as i32, (index / self.width) as i32);

        if [(1, 0), (-1, 0), (0, 1), (0, -1)]
            .iter()
            .any(|&(ox, oy)| !self.hit(x + ox, y + oy))
        {
            self.dots[index] = 1;
        }

        for (ox, oy) in [(1, 0), (0, 1)] {
            fold_dot(index, x, y, ox, oy, fold, self);
            depth_step(index, x, y, ox, oy, self);
        }
    }
}

fn outline_cell(x: usize, y: usize, outline: &Outline) -> Cell {
    let (mut bits, mut eye, mut sum, mut count) = (0, false, 0.0, 0);

    for (offset, bit) in DOT_BITS.iter().enumerate() {
        let index = (y * 4 + offset / 2) * outline.width + x * 2 + offset % 2;

        if outline.dots[index] != 0 {
            bits |= bit;
            eye |= outline.dots[index] == 2;
            sum += outline.pixels[index].lum;
            count += 1;
        }
    }

    if bits == 0 {
        return Cell::BLANK;
    }

    let pixel = Px {
        hit: true,
        lum: 0.45 + 0.55 * sum / count as f32,
        mat: if eye { EYE } else { 0 },
        ..Px::default()
    };

    Cell {
        ch: char::from_u32(0x2800 + bits).unwrap_or(' '),
        fg: color(&pixel),
        bg: None,
    }
}

fn depth_step(index: usize, x: i32, y: i32, ox: i32, oy: i32, outline: &mut Outline) {
    if !outline.hit(x - ox, y - oy) || !outline.hit(x + ox, y + oy) {
        return;
    }

    let depth =
        |offset: i32| outline.pixels[(y + oy * offset) as usize * outline.width + (x + ox * offset) as usize].depth;

    if depth(-1) + depth(1) - 2.0 * outline.pixels[index].depth > 0.12 {
        outline.dots[index] = 1;
    }
}

fn fold_dot(index: usize, x: i32, y: i32, ox: i32, oy: i32, threshold: f32, outline: &mut Outline) {
    if !outline.hit(x + ox, y + oy) {
        return;
    }

    let neighbor = (y + oy) as usize * outline.width + (x + ox) as usize;
    let pixel = &outline.pixels[index];
    let other = &outline.pixels[neighbor];

    if other.mat == EYE {
        return;
    }

    if pixel.normal.dot(other.normal) < threshold {
        let nearer = if pixel.depth <= other.depth { index } else { neighbor };

        outline.dots[nearer] = outline.dots[nearer].max(1);
    }
}
