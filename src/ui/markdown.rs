//! Markdown → styled terminal rows, wrapped to a width: the agents' messages in the thread view.
//! Code blocks get syntax colors (`code::highlight`), links stay clickable (their rows carry the
//! URL ranges).

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use crate::ui::theme;

/// A rendered row: its spans, the links in it (start col, end col, url), the spans a click
/// copies (start col, end col, text: inline code, a code block's copy button).
#[derive(Clone, Debug, Default)]
pub(crate) struct Row {
    pub line: Line<'static>,
    pub links: Vec<(u16, u16, String)>,
    pub copies: Vec<(u16, u16, String)>,
    /// The row goes on from the row above: 1 a wrap at a space, 2 a cut word. 0: a line of its own.
    pub wrap: u8,
}

impl Row {
    pub(crate) fn plain(line: Line<'static>) -> Row {
        Row {
            line,
            ..Default::default()
        }
    }
}

/// Inline code and commands: the info role.
fn code_fg() -> Color {
    theme::pal().info()
}

/// A styled word piece; LINK: the url it belongs to; COPY: what a click on it copies.
#[derive(Clone, Default)]
struct Piece {
    text: String,
    style: Style,
    link: Option<String>,
    copy: Option<String>,
}

thread_local! {
    /// `@name` mentions of this draw: (agent name, color). The stream sets them per draw.
    static MARKS: std::cell::RefCell<Vec<(String, Color)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// The agents whose `@name` mentions rows drawn from now on color (the stream's roster).
pub(crate) fn set_marks(marks: Vec<(String, Color)>) {
    MARKS.with_borrow_mut(|current| *current = marks);
}

/// A hash of the marks: cached rows of another roster are stale.
pub(crate) fn marks_hash() -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();

    MARKS.with_borrow(|current| current.hash(&mut hasher));
    hasher.finish()
}

/// TEXT in STYLE as spans, each `@name` of MARKS in its agent's color, bold. The matcher of the
/// prompt (`textarea::mention_colors`): 1 rule for the prompt and the stream.
pub(crate) fn mention_spans(text: &str, style: Style, marks: &[(String, Color)]) -> Vec<Span<'static>> {
    let chars: Vec<char> = text.chars().collect();
    let colors = crate::ui::textarea::mention_colors(&chars, marks);
    let mut out: Vec<Span<'static>> = Vec::new();
    let mut run = String::new();
    let mut cur: Option<Color> = None;
    let st = |color: Option<Color>| match color {
        Some(color) => style.fg(color).add_modifier(Modifier::BOLD),
        None => style,
    };

    for (ch, color) in chars.into_iter().zip(colors) {
        if color != cur && !run.is_empty() {
            out.push(Span::styled(std::mem::take(&mut run), st(cur)));
        }

        cur = color;
        run.push(ch);
    }

    if !run.is_empty() || out.is_empty() {
        out.push(Span::styled(run, st(cur)));
    }

    out
}

/// PIECES with each `@name` mention of this draw's marks as its own colored piece. 0 change in
/// code (inline code, copies) and links.
fn mention_pieces(pieces: &[Piece]) -> Vec<Piece> {
    MARKS.with_borrow(|marks| {
        if marks.is_empty() {
            return pieces.to_vec();
        }

        pieces.iter().flat_map(|piece| mention_piece(piece, marks)).collect()
    })
}

struct Block {
    pieces: Vec<Piece>,
    first: Vec<Span<'static>>,
    rest: Vec<Span<'static>>,
}

/// `file://<abs path>#L<line>` for inline code T that names an existing file: absolute, `~/…`,
/// or relative to the current dir (the thread view runs in the worktree), with an optional
/// `:line` / `:line:col`. None: 0 file (a word, a command, a dir).
pub(crate) fn file_link(text: &str) -> Option<String> {
    let trimmed = text.trim();

    if trimmed.is_empty() || trimmed.contains(char::is_whitespace) || trimmed.contains("://") || trimmed.len() > 300 {
        return None;
    }

    // `path:12` / `path:12:3`
    let mut parts = trimmed.splitn(3, ':');
    let path = parts.next()?;
    let line: Option<u32> = parts.next().and_then(|number| number.parse().ok());

    if !(path.contains('/') || path.contains('.')) {
        return None;
    }

    let resolved = match path.strip_prefix("~/") {
        Some(rest) => crate::infra::config::home().join(rest),
        None => std::path::PathBuf::from(path),
    };

    let abs = if resolved.is_absolute() {
        resolved
    } else {
        std::env::current_dir().ok()?.join(resolved)
    };

    abs.is_file().then(|| {
        format!(
            "file://{}{}",
            abs.display(),
            line.map_or(String::new(), |line| format!("#L{line}"))
        )
    })
}

#[cfg(test)]
#[path = "markdown_tests.rs"]
mod tests;

#[path = "markdown_parser.rs"]
mod parser;
pub(crate) use parser::markdown;
#[path = "markdown_html.rs"]
mod html;
use html::{HtmlPart, entities, html_attr, html_parts, html_words};
#[path = "markdown_tables.rs"]
mod tables;
use tables::table_rows;
#[path = "markdown_code.rs"]
mod code;
pub(crate) use code::code_rows;
#[cfg(test)]
use code::wrap_code;
#[path = "markdown_wrap.rs"]
mod wrap;
pub(crate) use wrap::plain;
use wrap::wrap;

fn mention_piece(piece: &Piece, marks: &[(String, Color)]) -> Vec<Piece> {
    if piece.copy.is_some() || piece.link.is_some() || piece.style.fg == Some(code_fg()) || !piece.text.contains('@') {
        return vec![piece.clone()];
    }

    mention_spans(&piece.text, piece.style, marks)
        .into_iter()
        .map(|span| Piece {
            text: span.content.into_owned(),
            style: span.style,
            ..piece.clone()
        })
        .collect()
}
