//! codehead's palette (ancient amber: 1 accent, muted text) with pills, panels, 0 cursor glyph.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

/// codehead's palette: ancient amber, like codehead.sh. Warm paper, bone ink, 1 amber accent.
/// The single accent: amber #EC8F1A.
pub const ACCENT: Color = Color::Rgb(236, 143, 26);
/// Text on an accent pill: paper #150700.
pub const ON_ACCENT: Color = Color::Rgb(21, 7, 0);
/// Inputs, unfocused active tab, unfocused selection row.
pub const PANEL: Color = Color::Rgb(42, 22, 12);
/// Bg of a clickable row / item under the mouse.
pub const HOVER: Color = Color::Rgb(61, 36, 22);
/// Normal text: ink #E3D2BE.
pub const TEXT: Color = Color::Rgb(227, 210, 190);
/// Hints, labels, timestamps.
pub const MUTED: Color = Color::Rgb(168, 146, 124);
/// Borders, dividers.
pub const LINE: Color = Color::Rgb(92, 67, 52);
pub const OK: Color = Color::Rgb(143, 175, 96);
pub const DANGER: Color = Color::Rgb(214, 84, 60);
pub const WARN: Color = Color::Rgb(224, 170, 60);

/// Hover: solid HOVER bg on the cells of R (accent cells, like the selection pill, kept).
pub fn hover(buf: &mut Buffer, r: Rect) {
    let r = r.intersection(buf.area);
    for y in r.top()..r.bottom() {
        for x in r.left()..r.right() {
            if let Some(c) = buf.cell_mut((x, y))
                && c.bg != ACCENT
            {
                c.set_bg(HOVER);
            }
        }
    }
}

pub fn text() -> Style {
    Style::default().fg(TEXT)
}
pub fn muted() -> Style {
    Style::default().fg(MUTED)
}
pub fn line() -> Style {
    Style::default().fg(LINE)
}
pub fn accent() -> Style {
    Style::default().fg(ACCENT)
}
pub fn bold() -> Style {
    Style::default().fg(TEXT).add_modifier(Modifier::BOLD)
}
pub fn accent_bold() -> Style {
    accent().add_modifier(Modifier::BOLD)
}
/// Selection pill: full row, accent bg, dark text.
pub fn pill() -> Style {
    Style::default().bg(ACCENT).fg(ON_ACCENT)
}
pub fn panel() -> Style {
    Style::default().bg(PANEL).fg(TEXT)
}
/// Count badge: readable on the normal row and on the pill row.
pub fn badge(on_pill: bool) -> Style {
    if on_pill {
        Style::default().bg(ON_ACCENT).fg(ACCENT).add_modifier(Modifier::BOLD)
    } else {
        Style::default().bg(ACCENT).fg(ON_ACCENT).add_modifier(Modifier::BOLD)
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



#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn osc4_replies() {
        let replies = "\x1b]4;1;rgb:cdcd/3131/3131\x1b\\\x1b]4;4;rgb:24/72/c8\x07\x1b[?62;22c";
        assert_eq!(parse_osc4(replies), vec![(1, (205, 49, 49)), (4, (36, 114, 200))]);
        assert!(da1_seen(replies.as_bytes()));
    }
}
