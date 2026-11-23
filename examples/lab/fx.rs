//! Loaders: dithered bars, pulses, Game of Life, a spinning solid, decoding text. Each one
//! draws 1 row of cells for time T (seconds).

use crate::glyph::{self, BG, CORAL, Cell, INK, MUTED, Rgb, bayer, mix};
use crate::sdf::V;

const BIT: [[u32; 2]; 4] = [[0x01, 0x08], [0x02, 0x10], [0x04, 0x20], [0x40, 0x80]];

/// Braille row of W cells from a dot function (x, y) → on.
fn braille(w: usize, fg: Rgb, on: impl Fn(usize, usize) -> bool) -> Vec<Cell> {
    (0..w)
        .map(|cx| {
            let mut bits = 0;
            for dy in 0..4 {
                for dx in 0..2 {
                    if on(cx * 2 + dx, dy) {
                        bits |= BIT[dy][dx];
                    }
                }
            }
            Cell {
                ch: char::from_u32(0x2800 + bits).unwrap_or(' '),
                fg,
                bg: None,
            }
        })
        .collect()
}

fn text(s: &str, fg: Rgb) -> Vec<Cell> {
    s.chars().map(|ch| Cell { ch, fg, bg: None }).collect()
}

fn smooth(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// Tiny deterministic noise.
fn hash(n: u64) -> f32 {
    let mut x = n.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    x ^= x >> 29;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 32;
    (x & 0xFFFF) as f32 / 65535.0
}

pub struct Loaders;

pub const LW: usize = 24;

pub const NAMES: [(&str, &str); 11] = [
    ("wake", "a session starts: 3x3 dot eyes light up 1 by 1"),
    (
        "gaze",
        "watching the stream: 3 lit dots drift in a band of 7",
    ),
    (
        "seal",
        "a long job: the dot mark dithers from 1 pattern to the next",
    ),
    (
        "dither bar",
        "progress: a soft edge, broken up by a Bayer matrix",
    ),
    ("orbit", "waiting on a tool: 3 dots run around a 4x4 square"),
    (
        "solid",
        "thinking: a wireframe cube turns in 8x4 braille dots",
    ),
    ("decode", "reading a file: noise resolves left to right"),
    ("wave", "streaming tokens: a sine through ▁▂▃▄▅▆▇█"),
    ("rain", "tokens per second: dot density = rate"),
    ("scan", "searching: a coral head sweeps over the path"),
    ("fill", "context window: dithered fill from ░ to █"),
];

impl Loaders {
    pub fn new() -> Loaders {
        Loaders
    }

    /// Row I at time T.
    pub fn draw(&mut self, i: usize, t: f32) -> Vec<Cell> {
        let w = LW;
        match NAMES[i].0 {
            "dither bar" => {
                let p = (t * 0.22).fract();
                let head = p * (w * 2 + 12) as f32;
                braille(w, CORAL, |x, y| {
                    smooth((head - x as f32) / 12.0) > bayer(x, y)
                })
            }
            "orbit" => {
                const RING: [(usize, usize); 8] = [
                    (1, 0),
                    (2, 0),
                    (3, 1),
                    (3, 2),
                    (2, 3),
                    (1, 3),
                    (0, 2),
                    (0, 1),
                ];
                let k = (t * 12.0) as usize;
                let on: Vec<(usize, usize)> = (0..3).map(|j| RING[(k + 8 - j) % 8]).collect();
                let mut row = braille(2, CORAL, |x, y| on.contains(&(x, y)));
                row.extend(text("  calling bash", MUTED));
                row
            }
            "solid" => {
                // wireframe cube, 8x4 dots: 12 edges, 10 points each
                let (yaw, pitch) = (t * 1.9, 0.5 + 0.2 * (t * 0.7).sin());
                let pt = |i: usize| {
                    let v = V(
                        if i & 1 == 0 { -1.0 } else { 1.0 },
                        if i & 2 == 0 { -1.0 } else { 1.0 },
                        if i & 4 == 0 { -1.0 } else { 1.0 },
                    );
                    let r = v.rot_y(yaw).rot_x(pitch);
                    (3.5 + r.0 * 1.9, 1.5 - r.1 * 1.25)
                };
                let mut dots = [[false; 8]; 4];
                for a in 0..8usize {
                    for bit in [1, 2, 4] {
                        let b = a | bit;
                        if b == a {
                            continue;
                        }
                        let (p, q) = (pt(a), pt(b));
                        for k in 0..=10 {
                            let s = k as f32 / 10.0;
                            let (x, y) = (
                                (p.0 + (q.0 - p.0) * s).round(),
                                (p.1 + (q.1 - p.1) * s).round(),
                            );
                            if (0.0..8.0).contains(&x) && (0.0..4.0).contains(&y) {
                                dots[y as usize][x as usize] = true;
                            }
                        }
                    }
                }
                let mut row = braille(4, CORAL, |x, y| dots[y][x]);
                row.extend(text("  thinking", MUTED));
                row
            }
            "decode" => {
                let s = "reading src/theme.rs";
                let cyc = (t * 0.5).fract() * 1.6;
                let noise = ['░', '▒', '▓', '/', '\\', '|', '-', '+', '*', '#'];
                s.chars()
                    .enumerate()
                    .map(|(i, ch)| {
                        let at = i as f32 / s.len() as f32;
                        if cyc > at + 0.15 {
                            Cell {
                                ch,
                                fg: INK,
                                bg: None,
                            }
                        } else if cyc > at {
                            let n = hash((t * 30.0) as u64 * 131 + i as u64);
                            Cell {
                                ch: noise[(n * noise.len() as f32) as usize % noise.len()],
                                fg: CORAL,
                                bg: None,
                            }
                        } else {
                            Cell {
                                ch: '·',
                                fg: mix(BG, MUTED, 0.5),
                                bg: None,
                            }
                        }
                    })
                    .collect()
            }
            "wave" => {
                const BARS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
                (0..w)
                    .map(|x| {
                        let v = 0.5
                            + 0.5 * (x as f32 * 0.45 - t * 5.0).sin() * (x as f32 * 0.13 + t).cos();
                        Cell {
                            ch: BARS[((v * 7.99) as usize).min(7)],
                            fg: mix(MUTED, CORAL, v),
                            bg: None,
                        }
                    })
                    .collect()
            }
            "rain" => {
                let rate = 0.12 + 0.3 * (0.5 + 0.5 * (t * 0.8).sin());
                let f = (t * 14.0) as u64;
                let mut row = braille(16, CORAL, |x, y| hash(f * 977 + (y * 64 + x) as u64) < rate);
                row.extend(text(&format!(" {:>3} tok/s", (rate * 300.0) as u32), MUTED));
                row
            }
            "scan" => {
                let s = "grep -r \"pal()\" src/";
                let n = s.chars().count() as f32;
                let head = ((t * 1.4).sin() * 0.5 + 0.5) * (n - 1.0);
                s.chars()
                    .enumerate()
                    .map(|(i, ch)| {
                        let d = (i as f32 - head).abs();
                        if d < 0.5 {
                            Cell {
                                ch,
                                fg: BG,
                                bg: Some(CORAL),
                            }
                        } else {
                            Cell {
                                ch,
                                fg: mix(CORAL, MUTED, (d / 4.0).min(1.0)),
                                bg: None,
                            }
                        }
                    })
                    .collect()
            }
            "wake" => {
                // 3x3 dot eyes in 2 braille cells wake up 1 by 1, hold, go dark: a session starting
                const ORDER: [usize; 9] = [4, 1, 7, 3, 5, 0, 8, 2, 6];
                let k = ((t * 6.0) as usize) % 16;
                let mut row = braille(2, CORAL, |x, y| {
                    if x > 2 || y > 2 {
                        return false;
                    }
                    let pos = ORDER.iter().position(|&o| o == y * 3 + x).unwrap_or(0);
                    k > pos && k < 13
                });
                row.extend(text("  starting ada", MUTED));
                row
            }
            "gaze" => {
                // a band of 7 dots, the lit 3 drift and settle: watching the stream
                let c = 3.0 + 3.0 * (t * 0.9).sin() * (t * 0.37).cos();
                let mut row = text("[", MUTED);
                for i in 0..7 {
                    let d = (i as f32 - c).abs();
                    let (ch, fg) = if d < 0.6 {
                        ('●', CORAL)
                    } else if d < 1.6 {
                        ('•', mix(MUTED, CORAL, 0.5))
                    } else {
                        ('·', mix(BG, MUTED, 0.6))
                    };
                    row.push(Cell { ch, fg, bg: None });
                }
                row.extend(text("]", MUTED));
                row
            }
            "seal" => {
                // the dot mark morphs through the constellations: a long job
                const PATS: [u16; 4] = [0b010_111_010, 0b111_101_111, 0b101_111_101, 0b101_010_101];
                let f = t * 0.8;
                let (a, b) = (PATS[f as usize % 4], PATS[(f as usize + 1) % 4]);
                let mix_t = f.fract();
                let dots = braille(3, CORAL, |x, y| {
                    if x % 2 == 1 || y == 3 || x / 2 > 2 {
                        return false;
                    }
                    let bit = 8 - (y * 3 + x / 2);
                    let on_a = a >> bit & 1 == 1;
                    let on_b = b >> bit & 1 == 1;
                    if mix_t > bayer(x / 2, y) { on_b } else { on_a }
                });
                let mut row = dots;
                row.extend(text("  indexing", MUTED));
                row
            }
            _ => {
                const SH: [char; 5] = [' ', '░', '▒', '▓', '█'];
                let p = 0.15 + 0.8 * (0.5 + 0.5 * (t * 0.35).sin());
                let mut row: Vec<Cell> = (0..16)
                    .map(|x| {
                        let v = ((p * 16.0 - x as f32) + 0.5).clamp(0.0, 1.0);
                        Cell {
                            ch: SH[(v * 4.0).round() as usize],
                            fg: if p > 0.8 { glyph::WARN } else { CORAL },
                            bg: None,
                        }
                    })
                    .collect();
                row.extend(text(&format!(" {:>3}% ctx", (p * 100.0) as u32), MUTED));
                row
            }
        }
    }
}
