//! codehead's palette: the base16 / catppuccin slot model (surfaces, overlay, text ramp, 1
//! accent, 7 hues) as a preset (lavender, coral, amber) + overrides, with roles over the hues,
//! pills, panels, 0 cursor glyph. 0 solid background: `base` is the bg the colors assume, for
//! tints only.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

/// The colors of a scheme. A preset, then the `[colors]` overrides.
///
/// Slots, as catppuccin: `base` < `surface0` < `surface1` < `surface2` (backgrounds),
/// `overlay0` (lines), `subtext0` < `text` (text), `accent` / `on_accent`, the hues. Call sites
/// use the roles ([`Palette::error`], …), not the hues, wherever a color means a state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Palette {
    /// The terminal background the colors assume (0 cell is painted with it): tints mix into it.
    pub base: Color,
    /// Inputs, unfocused active tab, unfocused selection row.
    pub surface0: Color,
    /// Bg of a clickable row / item under the mouse; the input bubble's border.
    pub surface1: Color,
    /// Bars, tracks, the 3rd step of the surfaces.
    pub surface2: Color,
    /// Borders, dividers.
    pub overlay0: Color,
    /// Hints, labels, timestamps.
    pub subtext0: Color,
    /// Normal text.
    pub text: Color,
    /// Focus and selection only: the cursor pill, the active tab, the focused border, marks.
    pub accent: Color,
    /// Text on an accent (or a hue) pill.
    pub on_accent: Color,
    pub red: Color,
    pub peach: Color,
    pub yellow: Color,
    pub green: Color,
    pub teal: Color,
    pub sky: Color,
    pub mauve: Color,
}

/// Soft pastel lavender on cool violet grays. The default: its accent is 0 status hue, so red
/// means "error" only.
pub const LAVENDER: Palette = Palette {
    base: Color::Rgb(29, 26, 40),
    surface0: Color::Rgb(43, 39, 57),
    surface1: Color::Rgb(58, 53, 80),
    surface2: Color::Rgb(74, 68, 98),
    overlay0: Color::Rgb(113, 106, 142),
    subtext0: Color::Rgb(173, 167, 198),
    text: Color::Rgb(232, 228, 246),
    accent: Color::Rgb(184, 166, 255),
    on_accent: Color::Rgb(29, 26, 40),
    red: Color::Rgb(244, 122, 142),
    peach: Color::Rgb(245, 166, 115),
    yellow: Color::Rgb(237, 212, 138),
    green: Color::Rgb(157, 219, 152),
    teal: Color::Rgb(133, 216, 196),
    sky: Color::Rgb(140, 206, 240),
    mauve: Color::Rgb(236, 156, 216),
};

/// hell's palette (axel's): coral #FF7565, cool grays. Its accent is close to its red.
pub const CORAL: Palette = Palette {
    base: Color::Rgb(24, 22, 26),
    surface0: Color::Rgb(46, 44, 52),
    surface1: Color::Rgb(66, 63, 74),
    surface2: Color::Rgb(84, 80, 94),
    overlay0: Color::Rgb(98, 94, 110),
    subtext0: Color::Rgb(146, 142, 156),
    text: Color::Rgb(232, 230, 236),
    accent: Color::Rgb(255, 117, 101),
    on_accent: Color::Rgb(24, 22, 26),
    red: Color::Rgb(215, 95, 95),
    peach: Color::Rgb(240, 160, 90),
    yellow: Color::Rgb(215, 175, 95),
    green: Color::Rgb(135, 175, 135),
    teal: Color::Rgb(95, 175, 175),
    sky: Color::Rgb(135, 175, 215),
    mauve: Color::Rgb(163, 113, 247),
};

/// Ancient amber, like codehead.sh: warm paper, bone ink, amber #EC8F1A. Its accent is close
/// to its yellow.
pub const AMBER: Palette = Palette {
    base: Color::Rgb(21, 7, 0),
    surface0: Color::Rgb(42, 22, 12),
    surface1: Color::Rgb(61, 36, 22),
    surface2: Color::Rgb(78, 52, 37),
    overlay0: Color::Rgb(104, 78, 62),
    subtext0: Color::Rgb(168, 146, 124),
    text: Color::Rgb(227, 210, 190),
    accent: Color::Rgb(236, 143, 26),
    on_accent: Color::Rgb(21, 7, 0),
    red: Color::Rgb(214, 84, 60),
    peach: Color::Rgb(226, 120, 70),
    yellow: Color::Rgb(224, 170, 60),
    green: Color::Rgb(143, 175, 96),
    teal: Color::Rgb(110, 170, 150),
    sky: Color::Rgb(125, 170, 190),
    mauve: Color::Rgb(186, 130, 196),
};

/// Presets: name, palette. The 1st one: the default.
pub const PRESETS: [(&str, Palette); 3] = [("lavender", LAVENDER), ("coral", CORAL), ("amber", AMBER)];

/// The color names a `[colors]` override can set.
pub const NAMES: [&str; 16] = [
    "base", "surface0", "surface1", "surface2", "overlay0", "subtext0", "text", "accent", "on_accent",
    "red", "peach", "yellow", "green", "teal", "sky", "mauve",
];

/// Names of the old palette (before the slot model) → their slot: old `[colors]` keys still work.
pub const ALIASES: [(&str, &str); 7] = [
    ("panel", "surface0"),
    ("hover", "surface1"),
    ("muted", "subtext0"),
    ("line", "overlay0"),
    ("ok", "green"),
    ("danger", "red"),
    ("warn", "yellow"),
];

/// NAME, or the slot an old name stands for.
pub fn slot_name(name: &str) -> &str {
    ALIASES.iter().find(|(old, _)| *old == name).map_or(name, |(_, new)| new)
}

impl Palette {
    pub fn preset(name: &str) -> Option<Palette> {
        PRESETS.iter().find(|(n, _)| *n == name.trim()).map(|(_, p)| *p)
    }

    pub fn get(&self, name: &str) -> Option<Color> {
        let mut p = *self;
        p.slot(name).map(|c| *c)
    }

    fn slot(&mut self, name: &str) -> Option<&mut Color> {
        Some(match slot_name(name) {
            "base" => &mut self.base,
            "surface0" => &mut self.surface0,
            "surface1" => &mut self.surface1,
            "surface2" => &mut self.surface2,
            "overlay0" => &mut self.overlay0,
            "subtext0" => &mut self.subtext0,
            "text" => &mut self.text,
            "accent" => &mut self.accent,
            "on_accent" => &mut self.on_accent,
            "red" => &mut self.red,
            "peach" => &mut self.peach,
            "yellow" => &mut self.yellow,
            "green" => &mut self.green,
            "teal" => &mut self.teal,
            "sky" => &mut self.sky,
            "mauve" => &mut self.mauve,
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

    // Roles: what a color means. 1 role, 1 hue; a hue may serve 2 roles of 1 family.

    /// Failed, broken, destructive: errors, failed checks, conflicts, deleted lines and files,
    /// closed PRs, critical findings, a quota at 90%+.
    pub fn error(&self) -> Color {
        self.red
    }
    /// Done, passed, added: a finished turn or flow, passed checks, added lines and files, an
    /// open or mergeable PR, reviewed.
    pub fn success(&self) -> Color {
        self.green
    }
    /// Waits on the owner: blocked, a question, a gate, needs action, unpushed commits.
    pub fn attention(&self) -> Color {
        self.yellow
    }
    /// A risk, not yet a failure: a quota at 70%+, high / major findings, unsaved settings, a
    /// small window, shell mode.
    pub fn caution(&self) -> Color {
        self.peach
    }
    /// Under way: running checks, a pending step or switch, open review comments.
    pub fn progress(&self) -> Color {
        self.sky
    }
    /// A reference: links, inline code, commands, branch names, modified files.
    pub fn info(&self) -> Color {
        self.sky
    }
    /// A merged PR.
    pub fn merged(&self) -> Color {
        self.mauve
    }
    /// A renamed file.
    pub fn renamed(&self) -> Color {
        self.teal
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

static PAL: std::sync::RwLock<Palette> = std::sync::RwLock::new(LAVENDER);

/// The palette in use.
pub fn pal() -> Palette {
    PAL.read().map(|p| *p).unwrap_or(LAVENDER)
}

/// Use P from now on (start, settings).
pub fn set_palette(p: Palette) {
    if let Ok(mut cur) = PAL.write() {
        *cur = p;
    }
}

/// Hover: solid pal().surface1 bg on the cells of R (accent cells, like the selection pill, kept).
pub fn hover(buf: &mut Buffer, r: Rect) {
    let r = r.intersection(buf.area);
    for y in r.top()..r.bottom() {
        for x in r.left()..r.right() {
            if let Some(c) = buf.cell_mut((x, y))
                && c.bg != pal().accent
            {
                c.set_bg(pal().surface1);
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
    Style::default().fg(pal().subtext0)
}
/// Shell mode (`!` in the prompt): its marks, its border. A caution: it runs commands.
pub fn shell() -> Color {
    pal().caution()
}

pub fn line() -> Style {
    Style::default().fg(pal().overlay0)
}
/// Text in COLOR (a role): `fg(pal().error())`.
pub fn fg(color: Color) -> Style {
    Style::default().fg(color)
}
/// A pane's border: the accent when it has the focus, else the line.
pub fn border(focused: bool) -> Style {
    if focused { accent() } else { line() }
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
/// The selection pill of a list without the focus: panel bg, accent text.
pub fn pill_dim() -> Style {
    Style::default().bg(pal().surface0).fg(pal().accent)
}
pub fn panel() -> Style {
    Style::default().bg(pal().surface0).fg(pal().text)
}

/// A picked row (of a multi-pick) in R: the accent's tint as bg, `▌` in the accent at BAR (a
/// column of R, else 0 bar). Cells with a bg of their own keep it when KEEP_BG.
pub fn picked(buf: &mut Buffer, r: Rect, bar: Option<u16>, keep_bg: bool) {
    let r = r.intersection(buf.area);
    let bg = tint(pal().accent, pal().surface0, 0.28);
    for y in r.top()..r.bottom() {
        for x in r.left()..r.right() {
            if Some(x) == bar { continue; }
            if let Some(c) = buf.cell_mut((x, y))
                && (!keep_bg || c.bg == Color::Reset)
            {
                c.set_bg(bg);
            }
        }
        if let Some(x) = bar
            && let Some(c) = buf.cell_mut((x, y))
        {
            c.set_symbol("▌").set_fg(pal().accent);
        }
    }
}

/// The backdrop of a modal: every cell of R faded halfway into `base`, so the modal drawn next
/// stands out. Default colors (Reset) fade from `text` / stay the terminal's bg.
pub fn dim(buf: &mut Buffer, r: Rect) {
    let r = r.intersection(buf.area);
    let p = pal();
    for y in r.top()..r.bottom() {
        for x in r.left()..r.right() {
            if let Some(c) = buf.cell_mut((x, y)) {
                let fg = if c.fg == Color::Reset { p.text } else { c.fg };
                c.set_fg(tint(fg, p.base, 0.45));
                if c.bg != Color::Reset {
                    c.set_bg(tint(c.bg, p.base, 0.45));
                }
            }
        }
    }
}
/// A key hint line (`key: label · key label · …`) as spans: each key in text, its label and the
/// ` · ` between in muted. A key: the text before `: `, else the 1st word.
pub fn hints(s: &str) -> Vec<ratatui::text::Span<'static>> {
    use ratatui::text::Span;
    let mut out = Vec::new();
    for (i, item) in s.split(" · ").enumerate() {
        if i > 0 {
            out.push(Span::styled(" · ", muted()));
        }
        let (key, rest) = match item.find(": ") {
            Some(at) => item.split_at(at),
            None => item.split_at(item.trim_start().find(' ').map_or(item.len(), |at| at + item.len() - item.trim_start().len())),
        };
        out.push(Span::styled(key.to_string(), text()));
        out.push(Span::styled(rest.to_string(), muted()));
    }
    out
}

/// A mode the keys are in, as a badge at the left of the key hint row: its label in capitals
/// (` SHELL `), then 1 blank. A neutral mode: surface1 bg, text; a risky one (shell): the
/// caution bg, on_accent text.
pub fn mode_badge(label: &str, risky: bool) -> Vec<ratatui::text::Span<'static>> {
    use ratatui::text::Span;
    let st = if risky {
        Style::default().bg(pal().caution()).fg(pal().on_accent)
    } else {
        Style::default().bg(pal().surface1).fg(pal().text)
    };
    vec![Span::styled(format!(" {} ", label.to_uppercase()), st.add_modifier(Modifier::BOLD)), Span::raw(" ")]
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
    fn hint_keys_stand_out() {
        let spans = hints("tab: menu · ctrl+f filter · enter");
        let keys: Vec<&str> = spans.iter().filter(|s| s.style == text()).map(|s| s.content.as_ref()).collect();
        assert_eq!(keys, ["tab", "ctrl+f", "enter"]);
        assert_eq!(spans.iter().map(|s| s.content.as_ref()).collect::<String>(), "tab: menu · ctrl+f filter · enter");
    }

    #[test]
    fn mode_badges() {
        let b = mode_badge("shell", true);
        assert_eq!(b[0].content, " SHELL ");
        assert_eq!((b[0].style.bg, b[0].style.fg), (Some(pal().caution()), Some(pal().on_accent)));
        assert_eq!(mode_badge("search", false)[0].style.bg, Some(pal().surface1));
    }

    #[test]
    fn old_color_names_still_set_a_slot() {
        let p = Palette::build(Some("coral"), [("danger", "#112233"), ("panel", "#445566")]);
        assert_eq!((p.red, p.surface0), (Color::Rgb(0x11, 0x22, 0x33), Color::Rgb(0x44, 0x55, 0x66)));
        assert_eq!(Palette::build(None, []), LAVENDER);
    }

    #[test]
    fn lavender_accent_is_no_status_hue() {
        // the default's accent stays apart from every status hue (hue distance ≥ 40°)
        let hue = |c: Color| {
            let (r, g, b) = rgb(c);
            let (r, g, b) = (f32::from(r), f32::from(g), f32::from(b));
            let (max, min) = (r.max(g).max(b), r.min(g).min(b));
            let d = max - min;
            let h = if max == r { (g - b) / d % 6.0 } else if max == g { (b - r) / d + 2.0 } else { (r - g) / d + 4.0 };
            (h * 60.0 + 360.0) % 360.0
        };
        let p = LAVENDER;
        for c in [p.red, p.peach, p.yellow, p.green, p.teal, p.sky, p.mauve] {
            let d = (hue(p.accent) - hue(c)).abs();
            assert!(d.min(360.0 - d) >= 40.0, "{c:?}");
        }
    }

    #[test]
    fn desaturation_keeps_hue() {
        // gray stays gray; red keeps its hue, 40% of its saturation
        assert_eq!(desaturate(Color::Rgb(128, 128, 128), 0.4), Color::Rgb(128, 128, 128));
        let Color::Rgb(r, g, b) = desaturate(Color::Rgb(255, 0, 0), 0.4) else { panic!() };
        assert_eq!((r, g, b), (179, 77, 77));
    }
}

