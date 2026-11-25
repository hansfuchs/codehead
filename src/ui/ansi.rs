//! ANSI SGR text (glow output) → ratatui lines. Other escapes (OSC links, cursor moves) are dropped.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

pub(crate) fn to_lines(input: &str) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let mut style = Style::default();

    for raw in input.split('\n') {
        append_ansi_line(&mut lines, &mut style, raw);
    }

    while lines
        .last()
        .is_some_and(|line| line.spans.iter().all(|text| text.content.trim().is_empty()))
    {
        lines.pop();
    }

    lines
}

fn append_ansi_line(lines: &mut Vec<Line<'_>>, style: &mut Style, raw: &str) {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut buf = String::new();
    let mut chars = raw.chars().peekable();

    while let Some(character) = chars.next() {
        parse_ansi_character(style, &mut spans, &mut buf, &mut chars, character);
    }

    if !buf.is_empty() {
        spans.push(Span::styled(buf, *style));
    }

    lines.push(Line::from(spans));
}

fn parse_ansi_character(
    style: &mut Style,
    spans: &mut Vec<Span<'static>>,
    buf: &mut String,
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    character: char,
) {
    if character != '\x1b' {
        if character == '\t' {
            buf.push_str("    ");
        } else if character != '\r' && !character.is_control() {
            buf.push(character);
        }

        return;
    }

    match chars.peek() {
        Some('[') => {
            chars.next();
            let (params, end) = read_csi(chars);

            if end == 'm' {
                flush_span(buf, spans, *style);

                *style = apply_sgr(*style, &params);
            }
        }
        Some(']') => {
            // OSC: until BEL or ESC \
            chars.next();

            skip_osc(chars);
        }
        _ => {
            chars.next();
        }
    }
}

pub(crate) fn apply_sgr(mut style: Style, params: &str) -> Style {
    let nums: Vec<u16> = if params.is_empty() {
        vec![0]
    } else {
        params.split(';').map(|path| path.parse().unwrap_or(0)).collect()
    };

    let mut i = 0;

    while i < nums.len() {
        let count = nums[i];

        match count {
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
            30..=37 => style = style.fg(Color::Indexed((count - 30) as u8)),
            39 => style.fg = None,
            40..=47 => style = style.bg(Color::Indexed((count - 40) as u8)),
            49 => style.bg = None,
            90..=97 => style = style.fg(Color::Indexed((count - 90 + 8) as u8)),
            100..=107 => style = style.bg(Color::Indexed((count - 100 + 8) as u8)),
            38 | 48 => {
                apply_sgr_color(&mut style, &nums, &mut i, count);
            }
            _ => {}
        }

        i += 1;
    }

    style
}

fn apply_sgr_color(style: &mut Style, nums: &[u16], i: &mut usize, count: u16) {
    let color = match nums.get(*i + 1) {
        Some(5) => {
            let character = nums.get(*i + 2).copied().unwrap_or(0) as u8;

            *i += 2;
            Some(Color::Indexed(character))
        }
        Some(2) => {
            let group = |key: usize| nums.get(*i + key).copied().unwrap_or(0) as u8;
            let color_value = Color::Rgb(group(2), group(3), group(4));

            *i += 4;
            Some(color_value)
        }
        _ => None,
    };

    if let Some(character_value) = color {
        *style = if count == 38 {
            style.fg(character_value)
        } else {
            style.bg(character_value)
        };
    }
}

fn read_csi(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> (String, char) {
    let mut params = String::new();
    let mut end = ' ';

    for path in chars.by_ref() {
        if ('\x40'..='\x7e').contains(&path) {
            end = path;
            break;
        }

        params.push(path);
    }

    (params, end)
}

fn flush_span(buf: &mut String, spans: &mut Vec<Span<'static>>, style: Style) {
    if !buf.is_empty() {
        spans.push(Span::styled(std::mem::take(buf), style));
    }
}

fn skip_osc(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    while let Some(path_value) = chars.next() {
        if path_value == '\x07' {
            break;
        }

        if path_value == '\x1b' {
            chars.next();
            break;
        }
    }
}
