//! codehead's palette: the base16 / catppuccin slot model (surfaces, overlay, text ramp, 1
//! accent, 7 hues) as a preset (lavender, coral, amber) + overrides, with roles over the hues,
//! pills, panels, 0 cursor glyph. 0 solid background: `base` is the bg the colors assume, for
//! tints only.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use std::fmt::Write as FormatWrite;
use std::io::{Read, Write};
use std::os::fd::AsRawFd;

/// Hover: solid pal().surface1 bg on the cells of R (accent cells, like the selection pill, kept).
pub(crate) fn hover(buf: &mut Buffer, area: Rect) {
    let clipped = area.intersection(buf.area);

    for y in clipped.top()..clipped.bottom() {
        for x in clipped.left()..clipped.right() {
            hover_cell(buf, x, y);
        }
    }
}

/// The cursor row R of a list: on the accent (the text `on_accent`), as the menu rows. Accent
/// text in it (filter marks): underlined, else it would vanish.
pub(crate) fn cursor_row(buf: &mut Buffer, area: Rect) {
    let clipped = area.intersection(buf.area);

    for y in clipped.top()..clipped.bottom() {
        for x in clipped.left()..clipped.right() {
            cursor_row_cell(buf, x, y);
        }
    }
}

/// A focused card / box R: its far left column on the accent (a solid bar), next to its accent
/// border. The top + bottom cells (the corners) stay.
pub(crate) fn accent_edge(buf: &mut Buffer, area: Rect) {
    let clipped = area.intersection(buf.area);

    if clipped.width == 0 || clipped.height < 3 {
        return;
    }

    for y in clipped.top() + 1..clipped.bottom() - 1 {
        if let Some(cell) = buf.cell_mut((clipped.x, y)) {
            cell.set_symbol(" ").set_bg(pal().accent).set_fg(pal().on_accent);
        }
    }
}

pub(crate) fn text() -> Style {
    Style::default().fg(pal().text)
}
pub(crate) fn muted() -> Style {
    Style::default().fg(pal().subtext0)
}
/// Shell mode (`!` in the prompt): its marks, its border. A caution: it runs commands.
pub(crate) fn shell() -> Color {
    pal().caution()
}

pub(crate) fn line() -> Style {
    Style::default().fg(pal().overlay0)
}
/// Text in COLOR (a role): `fg(pal().error())`.
pub(crate) fn fg(color: Color) -> Style {
    Style::default().fg(color)
}
/// A pane's border: the accent when it has the focus, else the line.
pub(crate) fn border(focused: bool) -> Style {
    if focused { accent() } else { line() }
}
pub(crate) fn accent() -> Style {
    Style::default().fg(pal().accent)
}
pub(crate) fn bold() -> Style {
    Style::default().fg(pal().text).add_modifier(Modifier::BOLD)
}
pub(crate) fn accent_bold() -> Style {
    accent().add_modifier(Modifier::BOLD)
}
/// Selection pill: full row, accent bg, dark text.
pub(crate) fn pill() -> Style {
    Style::default().bg(pal().accent).fg(pal().on_accent)
}
/// The selection pill of a list without the focus: panel bg, accent text.
pub(crate) fn pill_dim() -> Style {
    Style::default().bg(pal().surface0).fg(pal().accent)
}
pub(crate) fn panel() -> Style {
    Style::default().bg(pal().surface0).fg(pal().text)
}

/// A picked row (of a multi-pick) in R: the accent's tint as bg, `▌` in the accent at BAR (a
/// column of R, else 0 bar). Cells with a bg of their own keep it when `KEEP_BG`.
pub(crate) fn picked(buf: &mut Buffer, area: Rect, bar: Option<u16>, keep_bg: bool) {
    let clipped = area.intersection(buf.area);
    let bg = tint(pal().accent, pal().surface0, 0.28);

    for y in clipped.top()..clipped.bottom() {
        for x in clipped.left()..clipped.right() {
            picked_cell(buf, x, y, bar, keep_bg, bg);
        }

        if let Some(x) = bar
            && let Some(cell) = buf.cell_mut((x, y))
        {
            cell.set_symbol("▌").set_fg(pal().accent);
        }
    }
}

/// The backdrop of a modal: every cell of R faded halfway into `base`, so the modal drawn next
/// stands out. Default colors (Reset) fade from `text` / stay the terminal's bg.
pub(crate) fn dim(buf: &mut Buffer, area: Rect) {
    let clipped = area.intersection(buf.area);
    let palette = pal();

    for y in clipped.top()..clipped.bottom() {
        for x in clipped.left()..clipped.right() {
            dim_cell(buf, x, y, &palette);
        }
    }
}
/// A key hint line (`key: label · key label · …`) as spans: each key in text, its label and the
/// ` · ` between in muted. A key: the text before `: `, else the 1st word.
pub(crate) fn hints(hint_line: &str) -> Vec<ratatui::text::Span<'static>> {
    use ratatui::text::Span;
    let mut out = Vec::new();

    for (i, item) in hint_line.split(" · ").enumerate() {
        if i > 0 {
            out.push(Span::styled(" · ", muted()));
        }

        let (key, rest) = match item.find(": ") {
            Some(at) => item.split_at(at),
            None => item.split_at(
                item.trim_start()
                    .find(' ')
                    .map_or(item.len(), |at| at + item.len() - item.trim_start().len()),
            ),
        };

        out.push(Span::styled(key.to_string(), text()));
        out.push(Span::styled(rest.to_string(), muted()));
    }

    out
}

/// A mode the keys are in, as a badge at the left of the key hint row: its label in capitals
/// (` SHELL `), then 1 blank. A neutral mode: surface1 bg, text; a risky one (shell): the
/// caution bg, `on_accent` text.
pub(crate) fn mode_badge(label: &str, risky: bool) -> Vec<ratatui::text::Span<'static>> {
    use ratatui::text::Span;

    let st = if risky {
        Style::default().bg(pal().caution()).fg(pal().on_accent)
    } else {
        Style::default().bg(pal().surface1).fg(pal().text)
    };

    vec![
        Span::styled(format!(" {} ", label.to_uppercase()), st.add_modifier(Modifier::BOLD)),
        Span::raw(" "),
    ]
}

/// Count badge: readable on the normal row and on the pill row.
pub(crate) fn badge(on_pill: bool) -> Style {
    if on_pill {
        Style::default()
            .bg(pal().on_accent)
            .fg(pal().accent)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .bg(pal().accent)
            .fg(pal().on_accent)
            .add_modifier(Modifier::BOLD)
    }
}

/// Ask the terminal (raw mode, before any other read) for palette colors 0-15. A DA1 query
/// ends the replies; 0 reply in 200ms: xterm's colors.
pub(crate) fn query_palette() {
    if PALETTE.get().is_some() {
        return;
    }

    let mut colors = XTERM;

    if let Ok(mut tty) = std::fs::OpenOptions::new().read(true).write(true).open("/dev/tty") {
        let query = palette_query();

        if tty.write_all(query.as_bytes()).is_ok() {
            read_palette(&mut tty, &mut colors);
        }
    }

    let _ = PALETTE.set(colors);
}

fn da1_seen(bytes: &[u8]) -> bool {
    bytes
        .windows(3)
        .enumerate()
        .any(|(i, window)| window == b"\x1b[?" && bytes[i..].contains(&b'c'))
}

/// `ESC ] 4 ; N ; rgb:RRRR/GGGG/BBBB` replies → (N, rgb), 1-4 hex digits per channel.
fn parse_osc4(text: &str) -> Vec<(usize, (u8, u8, u8))> {
    let re = regex::Regex::new(r"\x1b\]4;(\d+);rgb:([0-9a-fA-F]{1,4})/([0-9a-fA-F]{1,4})/([0-9a-fA-F]{1,4})")
        .expect("valid regex");

    let ch = |hex: &str| -> u8 {
        let value = u32::from_str_radix(hex, 16).unwrap_or(0);
        let max = (1u32 << (4 * hex.len())) - 1;

        (value * 255 / max.max(1)) as u8
    };

    re.captures_iter(text)
        .filter_map(|captures| {
            let i: usize = captures[1].parse().ok()?;

            (i < 16).then(|| (i, (ch(&captures[2]), ch(&captures[3]), ch(&captures[4]))))
        })
        .collect()
}

#[cfg(test)]
#[path = "theme_tests.rs"]
mod tests;

pub(crate) use crate::domain::palette::*;

fn hover_cell(buf: &mut Buffer, x: u16, y: u16) {
    if let Some(cell) = buf.cell_mut((x, y))
        && cell.bg != pal().accent
    {
        cell.set_bg(pal().surface1);
    }
}

fn cursor_row_cell(buf: &mut Buffer, x: u16, y: u16) {
    if let Some(cell) = buf.cell_mut((x, y)) {
        if cell.fg == pal().accent {
            cell.modifier
                .insert(ratatui::style::Modifier::UNDERLINED | ratatui::style::Modifier::BOLD);
        }

        cell.set_bg(pal().accent).set_fg(pal().on_accent);
    }
}

fn picked_cell(buf: &mut Buffer, x: u16, y: u16, bar: Option<u16>, keep_bg: bool, bg: Color) {
    if Some(x) == bar {
        return;
    }

    if let Some(cell) = buf.cell_mut((x, y))
        && (!keep_bg || cell.bg == Color::Reset)
    {
        cell.set_bg(bg);
    }
}

fn dim_cell(buf: &mut Buffer, x: u16, y: u16, palette: &crate::domain::palette::Palette) {
    if let Some(cell) = buf.cell_mut((x, y)) {
        let fg = if cell.fg == Color::Reset { palette.text } else { cell.fg };

        cell.set_fg(tint(fg, palette.base, 0.45));

        if cell.bg != Color::Reset {
            cell.set_bg(tint(cell.bg, palette.base, 0.45));
        }
    }
}

fn read_palette(tty: &mut std::fs::File, colors: &mut [(u8, u8, u8); 16]) {
    let mut got = Vec::new();
    let end = std::time::Instant::now() + std::time::Duration::from_millis(200);
    let fd = tty.as_raw_fd();

    // non-blocking reads in 1 ms steps: poll on /dev/tty answers at once on macOS
    // (POLLNVAL), a blocking read then waits for the next key when 0 reply comes
    // SAFETY: FD is open (TTY lives); F_GETFL / F_SETFL take an int. Its own open file
    // description: stdin keeps its flags.
    unsafe { libc::fcntl(fd, libc::F_SETFL, libc::fcntl(fd, libc::F_GETFL) | libc::O_NONBLOCK) };

    // until DA1's `c` reply (`ESC [ ? … c`) or the deadline
    while !da1_seen(&got) && std::time::Instant::now() < end {
        let mut buf = [0u8; 1024];

        match tty.read(&mut buf) {
            Ok(count) if count > 0 => got.extend_from_slice(&buf[..count]),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            _ => break,
        }
    }

    for (i, rgb) in parse_osc4(&String::from_utf8_lossy(&got)) {
        colors[i] = rgb;
    }
}

fn palette_query() -> String {
    let mut query = String::new();

    for i in 0..16 {
        let _ = write!(query, "\x1b]4;{i};?\x1b\\");
    }

    query.push_str("\x1b[c");
    query
}
