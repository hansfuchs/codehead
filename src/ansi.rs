//! ANSI SGR text (glow output) → ratatui lines. Other escapes (OSC links, cursor moves) are dropped.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

pub fn to_lines(input: &str) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let mut style = Style::default();
    for raw in input.split('\n') {
        let mut spans: Vec<Span<'static>> = Vec::new();
        let mut buf = String::new();
        let mut chars = raw.chars().peekable();
        while let Some(c) = chars.next() {
            if c != '\x1b' {
                if c == '\t' {
                    buf.push_str("    ");
                } else if c != '\r' && !c.is_control() {
                    buf.push(c);
                }
                continue;
            }
            match chars.peek() {
                Some('[') => {
                    chars.next();
                    let mut params = String::new();
                    let mut end = ' ';
                    for p in chars.by_ref() {
                        if ('\x40'..='\x7e').contains(&p) {
                            end = p;
                            break;
                        }
                        params.push(p);
                    }
                    if end == 'm' {
                        if !buf.is_empty() {
                            spans.push(Span::styled(std::mem::take(&mut buf), style));
                        }
                        style = apply_sgr(style, &params);
                    }
                }
                Some(']') => {
                    // OSC: until BEL or ESC \
                    chars.next();
                    while let Some(p) = chars.next() {
                        if p == '\x07' {
                            break;
                        }
                        if p == '\x1b' {
                            chars.next();
                            break;
                        }
                    }
                }
                _ => {
                    chars.next();
                }
            }
        }
        if !buf.is_empty() {
            spans.push(Span::styled(buf, style));
        }
        lines.push(Line::from(spans));
    }
    while lines.last().is_some_and(|l| l.spans.iter().all(|s| s.content.trim().is_empty())) {
        lines.pop();
    }
    lines
}

pub(crate) fn apply_sgr(mut style: Style, params: &str) -> Style {
    let nums: Vec<u16> = if params.is_empty() {
        vec![0]
    } else {
        params.split(';').map(|p| p.parse().unwrap_or(0)).collect()
    };
    let mut i = 0;
    while i < nums.len() {
        let n = nums[i];
        match n {
            0 => style = Style::default(),
            1 => style = style.add_modifier(Modifier::BOLD),
            2 => style = style.add_modifier(Modifier::DIM),
            3 => style = style.add_modifier(Modifier::ITALIC),
            4 => style = style.add_modifier(Modifier::UNDERLINED),
            7 => style = style.add_modifier(Modifier::REVERSED),
            9 => style = style.add_modifier(Modifier::CROSSED_OUT),
            22 => style = style.remove_modifier(Modifier::BOLD | Modifier::DIM),
            23 => style = style.remove_modifier(Modifier::ITALIC),
            24 => style = style.remove_modifier(Modifier::UNDERLINED),
            27 => style = style.remove_modifier(Modifier::REVERSED),
            29 => style = style.remove_modifier(Modifier::CROSSED_OUT),
            30..=37 => style = style.fg(Color::Indexed((n - 30) as u8)),
            39 => style.fg = None,
            40..=47 => style = style.bg(Color::Indexed((n - 40) as u8)),
            49 => style.bg = None,
            90..=97 => style = style.fg(Color::Indexed((n - 90 + 8) as u8)),
            100..=107 => style = style.bg(Color::Indexed((n - 100 + 8) as u8)),
            38 | 48 => {
                let color = match nums.get(i + 1) {
                    Some(5) => {
                        let c = nums.get(i + 2).copied().unwrap_or(0) as u8;
                        i += 2;
                        Some(Color::Indexed(c))
                    }
                    Some(2) => {
                        let g = |k: usize| nums.get(i + k).copied().unwrap_or(0) as u8;
                        let c = Color::Rgb(g(2), g(3), g(4));
                        i += 4;
                        Some(c)
                    }
                    _ => None,
                };
                if let Some(c) = color {
                    style = if n == 38 { style.fg(c) } else { style.bg(c) };
                }
            }
            _ => {}
        }
        i += 1;
    }
    style
}
