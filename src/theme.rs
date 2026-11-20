//! codehead's palette (a preset: coral or amber, + overrides; 1 accent, muted text) with pills,
//! panels, 0 cursor glyph.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

/// The colors of a scheme. A preset (coral, amber), then the `[colors]` overrides.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Palette {
    /// The single accent.
    pub accent: Color,
    /// Text on an accent pill.
    pub on_accent: Color,
    /// Inputs, unfocused active tab, unfocused selection row.
    pub panel: Color,
    /// Bg of a clickable row / item under the mouse; the input bubble's border.
    pub hover: Color,
    /// Normal text.
    pub text: Color,
    /// Hints, labels, timestamps.
    pub muted: Color,
    /// Borders, dividers.
    pub line: Color,
    pub ok: Color,
    pub danger: Color,
    pub warn: Color,
}

/// hell's palette (axel's): coral #FF7565, cool grays. The default.
pub const CORAL: Palette = Palette {
    accent: Color::Rgb(255, 117, 101),
    on_accent: Color::Rgb(24, 22, 26),
    panel: Color::Rgb(46, 44, 52),
    hover: Color::Rgb(66, 63, 74),
    text: Color::Rgb(232, 230, 236),
    muted: Color::Rgb(146, 142, 156),
    line: Color::Rgb(84, 80, 94),
    ok: Color::Indexed(108),
    danger: Color::Indexed(167),
    warn: Color::Indexed(179),
};

/// Ancient amber, like codehead.sh: warm paper, bone ink, amber #EC8F1A.
pub const AMBER: Palette = Palette {
    accent: Color::Rgb(236, 143, 26),
    on_accent: Color::Rgb(21, 7, 0),
    panel: Color::Rgb(42, 22, 12),
    hover: Color::Rgb(61, 36, 22),
    text: Color::Rgb(227, 210, 190),
    muted: Color::Rgb(168, 146, 124),
    line: Color::Rgb(92, 67, 52),
    ok: Color::Rgb(143, 175, 96),
    danger: Color::Rgb(214, 84, 60),
    warn: Color::Rgb(224, 170, 60),
};

/// Presets: name, palette. The 1st one: the default.
pub const PRESETS: [(&str, Palette); 2] = [("coral", CORAL), ("amber", AMBER)];

/// The color names a `[colors]` override can set.
pub const NAMES: [&str; 10] = ["accent", "on_accent", "panel", "hover", "text", "muted", "line", "ok", "danger", "warn"];

impl Palette {
    pub fn preset(name: &str) -> Option<Palette> {
        PRESETS.iter().find(|(n, _)| *n == name.trim()).map(|(_, p)| *p)
    }

    pub fn get(&self, name: &str) -> Option<Color> {
        Some(match name {
            "accent" => self.accent,
            "on_accent" => self.on_accent,
            "panel" => self.panel,
            "hover" => self.hover,
            "text" => self.text,
            "muted" => self.muted,
            "line" => self.line,
            "ok" => self.ok,
            "danger" => self.danger,
            "warn" => self.warn,
            _ => return None,
        })
    }

    fn slot(&mut self, name: &str) -> Option<&mut Color> {
        Some(match name {
            "accent" => &mut self.accent,
            "on_accent" => &mut self.on_accent,
            "panel" => &mut self.panel,
            "hover" => &mut self.hover,
            "text" => &mut self.text,
            "muted" => &mut self.muted,
            "line" => &mut self.line,
            "ok" => &mut self.ok,
            "danger" => &mut self.danger,
            "warn" => &mut self.warn,
            _ => return None,
        })
    }

    /// Preset SCHEME (unknown / None: the default) with OVERRIDES (name → `#RRGGBB`).
    pub fn build<'a>(scheme: Option<&str>, overrides: impl IntoIterator<Item = (&'a str, &'a str)>) -> Palette {
        let mut p = scheme.and_then(Palette::preset).unwrap_or(PRESETS[0].1);
        for (name, hex) in overrides {
            if let (Some(slot), Some(c)) = (p.slot(name), parse_hex(hex)) {
                *slot = c;
            }
        }
        p
    }
}

/// `#RRGGBB` (or `RRGGBB`) → the color.
pub fn parse_hex(s: &str) -> Option<Color> {
    let h = s.trim().trim_start_matches('#');
    if h.len() != 6 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let v = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    Some(Color::Rgb(v(0)?, v(2)?, v(4)?))
}

/// C as `#RRGGBB`.
pub fn hex(c: Color) -> String {
    let (r, g, b) = rgb(c);
    format!("#{r:02X}{g:02X}{b:02X}")
}

static PAL: std::sync::RwLock<Palette> = std::sync::RwLock::new(CORAL);

/// The palette in use.
pub fn pal() -> Palette {
    PAL.read().map(|p| *p).unwrap_or(CORAL)
}

/// Use P from now on (start, settings).
pub fn set_palette(p: Palette) {
    if let Ok(mut cur) = PAL.write() {
        *cur = p;
    }
}

/// Hover: solid pal().hover bg on the cells of R (accent cells, like the selection pill, kept).
pub fn hover(buf: &mut Buffer, r: Rect) {
    let r = r.intersection(buf.area);
    for y in r.top()..r.bottom() {
        for x in r.left()..r.right() {
            if let Some(c) = buf.cell_mut((x, y))
                && c.bg != pal().accent
            {
                c.set_bg(pal().hover);
            }
        }
    }
}

/// The cursor row R of a list: on the accent (the text on_accent), as the menu rows. Accent
/// text in it (filter marks): underlined, else it would vanish.
pub fn cursor_row(buf: &mut Buffer, r: Rect) {
    let r = r.intersection(buf.area);
    for y in r.top()..r.bottom() {
        for x in r.left()..r.right() {
            if let Some(c) = buf.cell_mut((x, y)) {
                if c.fg == pal().accent {
                    c.modifier.insert(ratatui::style::Modifier::UNDERLINED | ratatui::style::Modifier::BOLD);
                }
                c.set_bg(pal().accent).set_fg(pal().on_accent);
            }
        }
    }
}

pub fn text() -> Style {
    Style::default().fg(pal().text)
}
pub fn muted() -> Style {
    Style::default().fg(pal().muted)
}
/// Shell mode (`!` in the prompt): its marks, its border.
pub fn shell() -> Color {
    Color::Rgb(253, 93, 177)
}

pub fn line() -> Style {
    Style::default().fg(pal().line)
}
pub fn accent() -> Style {
    Style::default().fg(pal().accent)
}
pub fn bold() -> Style {
    Style::default().fg(pal().text).add_modifier(Modifier::BOLD)
}
pub fn accent_bold() -> Style {
    accent().add_modifier(Modifier::BOLD)
}
/// Selection pill: full row, accent bg, dark text.
pub fn pill() -> Style {
    Style::default().bg(pal().accent).fg(pal().on_accent)
}
pub fn panel() -> Style {
    Style::default().bg(pal().panel).fg(pal().text)
}
/// Count badge: readable on the normal row and on the pill row.
pub fn badge(on_pill: bool) -> Style {
    if on_pill {
        Style::default().bg(pal().on_accent).fg(pal().accent).add_modifier(Modifier::BOLD)
    } else {
        Style::default().bg(pal().accent).fg(pal().on_accent).add_modifier(Modifier::BOLD)
    }
}

/// The terminal's 16 palette colors (OSC 4 replies), read once at start. Unanswered: xterm's.
static PALETTE: std::sync::OnceLock<[(u8, u8, u8); 16]> = std::sync::OnceLock::new();

const XTERM: [(u8, u8, u8); 16] = [
    (0, 0, 0), (205, 0, 0), (0, 205, 0), (205, 205, 0), (0, 0, 238), (205, 0, 205), (0, 205, 205), (229, 229, 229),
    (127, 127, 127), (255, 0, 0), (0, 255, 0), (255, 255, 0), (92, 92, 255), (255, 0, 255), (0, 255, 255), (255, 255, 255),
];

/// Ask the terminal (raw mode, before any other read) for palette colors 0-15. A DA1 query
/// ends the replies; 0 reply in 200ms: xterm's colors.
pub fn query_palette() {
    if PALETTE.get().is_some() { return; }
    use std::io::{Read, Write};
    use std::os::fd::AsRawFd;
    let mut colors = XTERM;
    if let Ok(mut tty) = std::fs::OpenOptions::new().read(true).write(true).open("/dev/tty") {
        let query: String = (0..16).map(|i| format!("\x1b]4;{i};?\x1b\\")).collect::<String>() + "\x1b[c";
        if tty.write_all(query.as_bytes()).is_ok() {
            let mut got = Vec::new();
            let end = std::time::Instant::now() + std::time::Duration::from_millis(200);
            let fd = tty.as_raw_fd();
            // until DA1's `c` reply (`ESC [ ? … c`) or the deadline
            while !da1_seen(&got) {
                let left = end.saturating_duration_since(std::time::Instant::now());
                if left.is_zero() { break; }
                let mut p = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
                if unsafe { libc::poll(&mut p, 1, left.as_millis() as i32) } <= 0 { break; }
                let mut buf = [0u8; 1024];
                match tty.read(&mut buf) { Ok(n) if n > 0 => got.extend_from_slice(&buf[..n]), _ => break }
            }
            for (i, rgb) in parse_osc4(&String::from_utf8_lossy(&got)) { colors[i] = rgb; }
        }
    }
    let _ = PALETTE.set(colors);
}

fn da1_seen(b: &[u8]) -> bool {
    b.windows(3).enumerate().any(|(i, w)| w == b"\x1b[?" && b[i..].contains(&b'c'))
}

/// `ESC ] 4 ; N ; rgb:RRRR/GGGG/BBBB` replies → (N, rgb), 1-4 hex digits per channel.
fn parse_osc4(text: &str) -> Vec<(usize, (u8, u8, u8))> {
    let re = regex::Regex::new(r"\x1b\]4;(\d+);rgb:([0-9a-fA-F]{1,4})/([0-9a-fA-F]{1,4})/([0-9a-fA-F]{1,4})").expect("valid regex");
    let ch = |h: &str| -> u8 {
        let v = u32::from_str_radix(h, 16).unwrap_or(0);
        let max = (1u32 << (4 * h.len())) - 1;
        (v * 255 / max.max(1)) as u8
    };
    re.captures_iter(text).filter_map(|c| {
        let i: usize = c[1].parse().ok()?;
        (i < 16).then(|| (i, (ch(&c[2]), ch(&c[3]), ch(&c[4]))))
    }).collect()
}



/// RGB of C: palette entries from the terminal (or xterm), the 256-color cube, grays.
pub fn rgb(c: Color) -> (u8, u8, u8) {
    let term = PALETTE.get().unwrap_or(&XTERM);
    let named = [Color::Black, Color::Red, Color::Green, Color::Yellow, Color::Blue, Color::Magenta, Color::Cyan, Color::Gray,
        Color::DarkGray, Color::LightRed, Color::LightGreen, Color::LightYellow, Color::LightBlue, Color::LightMagenta, Color::LightCyan, Color::White];
    match c {
        Color::Rgb(r, g, b) => (r, g, b),
        Color::Indexed(i) if i < 16 => term[usize::from(i)],
        Color::Indexed(i) if i < 232 => {
            let v = |n: u8| if n == 0 { 0 } else { 55 + n * 40 };
            let i = i - 16;
            (v(i / 36), v(i / 6 % 6), v(i % 6))
        }
        Color::Indexed(i) => { let g = 8 + (i - 232) * 10; (g, g, g) }
        c => named.iter().position(|n| *n == c).map_or(rgb(pal().accent), |i| term[i]),
    }
}

/// C with its HSL saturation times KEEP (0.4: 60% less; 1.4: 40% more, at most full), same hue
/// and lightness.
pub fn desaturate(c: Color, keep: f32) -> Color {
    let (r, g, b) = rgb(c);
    let (r, g, b) = (f32::from(r) / 255.0, f32::from(g) / 255.0, f32::from(b) / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let l = (max + min) / 2.0;
    if max == min { return Color::Rgb((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8); }
    let d = max - min;
    let s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
    let sector = if max == r {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    let h = sector / 6.0;
    let s = (s * keep).min(1.0);
    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p = 2.0 * l - q;
    let hue = |mut t: f32| {
        if t < 0.0 { t += 1.0 }
        if t > 1.0 { t -= 1.0 }
        if t < 1.0 / 6.0 { p + (q - p) * 6.0 * t } else if t < 0.5 { q } else if t < 2.0 / 3.0 { p + (q - p) * (2.0 / 3.0 - t) * 6.0 } else { p }
    };
    let to = |v: f32| (v * 255.0).round().clamp(0.0, 255.0) as u8;
    Color::Rgb(to(hue(h + 1.0 / 3.0)), to(hue(h)), to(hue(h - 1.0 / 3.0)))
}

/// C mixed into BASE: A of C (0: BASE, 1: C).
pub fn tint(c: Color, base: Color, a: f32) -> Color {
    let ((r, g, b), (r0, g0, b0)) = (rgb(c), rgb(base));
    let m = |x: u8, y: u8| (f32::from(y) + (f32::from(x) - f32::from(y)) * a).round() as u8;
    Color::Rgb(m(r, r0), m(g, g0), m(b, b0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn osc4_replies() {
        let replies = "\x1b]4;1;rgb:cdcd/3131/3131\x1b\\\x1b]4;4;rgb:24/72/c8\x07\x1b[?62;22c";
        assert_eq!(parse_osc4(replies), vec![(1, (205, 49, 49)), (4, (36, 114, 200))]);
        assert!(da1_seen(replies.as_bytes()));
    }

    #[test]
    fn desaturation_keeps_hue() {
        // gray stays gray; red keeps its hue, 40% of its saturation
        assert_eq!(desaturate(Color::Rgb(128, 128, 128), 0.4), Color::Rgb(128, 128, 128));
        let Color::Rgb(r, g, b) = desaturate(Color::Rgb(255, 0, 0), 0.4) else { panic!() };
        assert_eq!((r, g, b), (179, 77, 77));
    }
}

