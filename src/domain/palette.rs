use ratatui::style::Color;
/// The colors of a scheme. A preset, then the `[colors]` overrides.
///
/// Slots, as catppuccin: `base` < `surface0` < `surface1` < `surface2` (backgrounds),
/// `overlay0` (lines), `subtext0` < `text` (text), `accent` / `on_accent`, the hues. Call sites
/// use the roles ([`Palette::error`], …), not the hues, wherever a color means a state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Palette {
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
pub(crate) const LAVENDER: Palette = Palette {
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
pub(crate) const CORAL: Palette = Palette {
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
pub(crate) const AMBER: Palette = Palette {
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
pub(crate) const PRESETS: [(&str, Palette); 3] = [("lavender", LAVENDER), ("coral", CORAL), ("amber", AMBER)];

/// The color names a `[colors]` override can set.
pub(crate) const NAMES: [&str; 16] = [
    "base",
    "surface0",
    "surface1",
    "surface2",
    "overlay0",
    "subtext0",
    "text",
    "accent",
    "on_accent",
    "red",
    "peach",
    "yellow",
    "green",
    "teal",
    "sky",
    "mauve",
];

/// Names of the old palette (before the slot model) → their slot: old `[colors]` keys still work.
pub(crate) const ALIASES: [(&str, &str); 7] = [
    ("panel", "surface0"),
    ("hover", "surface1"),
    ("muted", "subtext0"),
    ("line", "overlay0"),
    ("ok", "green"),
    ("danger", "red"),
    ("warn", "yellow"),
];

/// NAME, or the slot an old name stands for.
pub(crate) fn slot_name(name: &str) -> &str {
    ALIASES
        .iter()
        .find(|(old, _)| *old == name)
        .map_or(name, |(_, new)| new)
}

impl Palette {
    pub(crate) fn preset(name: &str) -> Option<Palette> {
        PRESETS
            .iter()
            .find(|(preset, _)| *preset == name.trim())
            .map(|(_, palette)| *palette)
    }

    pub(crate) fn get(&self, name: &str) -> Option<Color> {
        let mut palette = *self;

        palette.slot(name).map(|color| *color)
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
    pub(crate) fn build<'a>(scheme: Option<&str>, overrides: impl IntoIterator<Item = (&'a str, &'a str)>) -> Palette {
        let mut palette = scheme.and_then(Palette::preset).unwrap_or(PRESETS[0].1);

        for (name, hex) in overrides {
            set_palette_slot(&mut palette, name, hex);
        }

        palette
    }

    // Roles: what a color means. 1 role, 1 hue; a hue may serve 2 roles of 1 family.

    /// Failed, broken, destructive: errors, failed checks, conflicts, deleted lines and files,
    /// closed PRs, critical findings, a quota at 90%+.
    pub(crate) fn error(&self) -> Color {
        self.red
    }
    /// Done, passed, added: a finished turn or flow, passed checks, added lines and files, an
    /// open or mergeable PR, reviewed.
    pub(crate) fn success(&self) -> Color {
        self.green
    }
    /// Waits on the owner: blocked, a question, a gate, needs action, unpushed commits.
    pub(crate) fn attention(&self) -> Color {
        self.yellow
    }
    /// A risk, not yet a failure: a quota at 70%+, high / major findings, unsaved settings, a
    /// small window, shell mode.
    pub(crate) fn caution(&self) -> Color {
        self.peach
    }
    /// Under way: running checks, a pending step or switch, open review comments.
    pub(crate) fn progress(&self) -> Color {
        self.sky
    }
    /// A reference: links, inline code, commands, branch names, modified files.
    pub(crate) fn info(&self) -> Color {
        self.sky
    }
    /// A merged PR.
    pub(crate) fn merged(&self) -> Color {
        self.mauve
    }
    /// A renamed file.
    pub(crate) fn renamed(&self) -> Color {
        self.teal
    }
}

fn set_palette_slot(palette: &mut Palette, name: &str, hex: &str) {
    if let (Some(slot), Some(color)) = (palette.slot(name), parse_hex(hex)) {
        *slot = color;
    }
}

/// `#RRGGBB` (or `RRGGBB`) → the color.
pub(crate) fn parse_hex(text: &str) -> Option<Color> {
    let hex = text.trim().trim_start_matches('#');

    if hex.len() != 6 || !hex.chars().all(|character| character.is_ascii_hexdigit()) {
        return None;
    }

    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();

    Some(Color::Rgb(byte(0)?, byte(2)?, byte(4)?))
}

/// C as `#RRGGBB`.
pub(crate) fn hex(color: Color) -> String {
    let (red, green, blue) = rgb(color);

    format!("#{red:02X}{green:02X}{blue:02X}")
}

static PAL: std::sync::RwLock<Palette> = std::sync::RwLock::new(LAVENDER);

/// The palette in use.
pub(crate) fn pal() -> Palette {
    PAL.read().map_or(LAVENDER, |palette| *palette)
}

/// Use P from now on (start, settings).
pub(crate) fn set_palette(palette: Palette) {
    if let Ok(mut cur) = PAL.write() {
        *cur = palette;
    }
}

/// The terminal's 16 palette colors (OSC 4 replies), read once at start. Unanswered: xterm's.
pub(crate) static PALETTE: std::sync::OnceLock<[(u8, u8, u8); 16]> = std::sync::OnceLock::new();

pub(crate) const XTERM: [(u8, u8, u8); 16] = [
    (0, 0, 0),
    (205, 0, 0),
    (0, 205, 0),
    (205, 205, 0),
    (0, 0, 238),
    (205, 0, 205),
    (0, 205, 205),
    (229, 229, 229),
    (127, 127, 127),
    (255, 0, 0),
    (0, 255, 0),
    (255, 255, 0),
    (92, 92, 255),
    (255, 0, 255),
    (0, 255, 255),
    (255, 255, 255),
];

/// RGB of C: palette entries from the terminal (or xterm), the 256-color cube, grays.
pub(crate) fn rgb(color: Color) -> (u8, u8, u8) {
    let term = PALETTE.get().unwrap_or(&XTERM);
    let named = [
        Color::Black,
        Color::Red,
        Color::Green,
        Color::Yellow,
        Color::Blue,
        Color::Magenta,
        Color::Cyan,
        Color::Gray,
        Color::DarkGray,
        Color::LightRed,
        Color::LightGreen,
        Color::LightYellow,
        Color::LightBlue,
        Color::LightMagenta,
        Color::LightCyan,
        Color::White,
    ];

    match color {
        Color::Rgb(red, green, blue) => (red, green, blue),
        Color::Indexed(i) if i < 16 => term[usize::from(i)],
        Color::Indexed(i) if i < 232 => {
            let level = |step: u8| if step == 0 { 0 } else { 55 + step * 40 };
            let i = i - 16;

            (level(i / 36), level(i / 6 % 6), level(i % 6))
        }
        Color::Indexed(i) => {
            let gray = 8 + (i - 232) * 10;

            (gray, gray, gray)
        }
        other => named
            .iter()
            .position(|named_color| *named_color == other)
            .map_or(rgb(pal().accent), |i| term[i]),
    }
}

/// C with its HSL saturation times KEEP (0.4: 60% less; 1.4: 40% more, at most full), same hue
/// and lightness.
pub(crate) fn desaturate(color: Color, keep: f32) -> Color {
    let (red, green, blue) = rgb(color);
    let (red, green, blue) = (
        f32::from(red) / 255.0,
        f32::from(green) / 255.0,
        f32::from(blue) / 255.0,
    );
    let (max, min) = (red.max(green).max(blue), red.min(green).min(blue));
    let lightness = f32::midpoint(max, min);

    if max.total_cmp(&min).is_eq() {
        return Color::Rgb((red * 255.0) as u8, (green * 255.0) as u8, (blue * 255.0) as u8);
    }

    let chroma = max - min;

    let saturation = if lightness > 0.5 {
        chroma / (2.0 - max - min)
    } else {
        chroma / (max + min)
    };

    let sector = if max.total_cmp(&red).is_eq() {
        (green - blue) / chroma + if green < blue { 6.0 } else { 0.0 }
    } else if max.total_cmp(&green).is_eq() {
        (blue - red) / chroma + 2.0
    } else {
        (red - green) / chroma + 4.0
    };

    hsl_to_rgb(keep, lightness, saturation, sector)
}

fn hsl_to_rgb(keep: f32, lightness: f32, saturation: f32, sector: f32) -> Color {
    let hue = sector / 6.0;
    let kept = (saturation * keep).min(1.0);

    let upper = if lightness < 0.5 {
        lightness * (1.0 + kept)
    } else {
        lightness + kept - lightness * kept
    };

    let lower = 2.0 * lightness - upper;

    let channel = |mut offset: f32| {
        if offset < 0.0 {
            offset += 1.0;
        }

        if offset > 1.0 {
            offset -= 1.0;
        }

        if offset < 1.0 / 6.0 {
            lower + (upper - lower) * 6.0 * offset
        } else if offset < 0.5 {
            upper
        } else if offset < 2.0 / 3.0 {
            lower + (upper - lower) * (2.0 / 3.0 - offset) * 6.0
        } else {
            lower
        }
    };

    let to = |level: f32| (level * 255.0).round().clamp(0.0, 255.0) as u8;

    Color::Rgb(
        to(channel(hue + 1.0 / 3.0)),
        to(channel(hue)),
        to(channel(hue - 1.0 / 3.0)),
    )
}

/// C mixed into BASE: AMOUNT of C (0: BASE, 1: C).
pub(crate) fn tint(color: Color, base: Color, amount: f32) -> Color {
    let ((red, green, blue), (base_red, base_green, base_blue)) = (rgb(color), rgb(base));
    let mix = |x: u8, y: u8| (f32::from(y) + (f32::from(x) - f32::from(y)) * amount).round() as u8;

    Color::Rgb(mix(red, base_red), mix(green, base_green), mix(blue, base_blue))
}
