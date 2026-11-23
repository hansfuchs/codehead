//! Markdown → styled terminal rows, wrapped to a width: the agents' messages in the thread view.
//! Code blocks get syntax colors (`code::highlight`), links stay clickable (their rows carry the
//! URL ranges).

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use crate::theme;

/// A rendered row: its spans, the links in it (start col, end col, url), the spans a click
/// copies (start col, end col, text: inline code, a code block's copy button).
#[derive(Clone, Debug, Default)]
pub struct Row {
    pub line: Line<'static>,
    pub links: Vec<(u16, u16, String)>,
    pub copies: Vec<(u16, u16, String)>,
    /// The row goes on from the row above: 1 a wrap at a space, 2 a cut word. 0: a line of its own.
    pub wrap: u8,
}

impl Row {
    pub fn plain(line: Line<'static>) -> Row {
        Row { line, ..Default::default() }
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
pub fn set_marks(marks: Vec<(String, Color)>) {
    MARKS.with_borrow_mut(|m| *m = marks);
}

/// A hash of the marks: cached rows of another roster are stale.
pub fn marks_hash() -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    MARKS.with_borrow(|m| m.hash(&mut h));
    h.finish()
}

/// TEXT in STYLE as spans, each `@name` of MARKS in its agent's color, bold. The matcher of the
/// prompt (`textarea::mention_colors`): 1 rule for the prompt and the stream.
pub fn mention_spans(text: &str, style: Style, marks: &[(String, Color)]) -> Vec<Span<'static>> {
    let chars: Vec<char> = text.chars().collect();
    let colors = crate::textarea::mention_colors(&chars, marks);
    let mut out: Vec<Span<'static>> = Vec::new();
    let mut run = String::new();
    let mut cur: Option<Color> = None;
    let st = |c: Option<Color>| match c {
        Some(c) => style.fg(c).add_modifier(Modifier::BOLD),
        None => style,
    };
    for (ch, c) in chars.into_iter().zip(colors) {
        if c != cur && !run.is_empty() {
            out.push(Span::styled(std::mem::take(&mut run), st(cur)));
        }
        cur = c;
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
        pieces
            .iter()
            .flat_map(|p| {
                if p.copy.is_some() || p.link.is_some() || p.style.fg == Some(code_fg()) || !p.text.contains('@') {
                    return vec![p.clone()];
                }
                mention_spans(&p.text, p.style, marks).into_iter().map(|s| Piece { text: s.content.into_owned(), style: s.style, ..p.clone() }).collect()
            })
            .collect()
    })
}

/// Wrap PIECES into rows of WIDTH: FIRST before the 1st row, REST before the others.
fn wrap(pieces: &[Piece], width: usize, first: &[Span<'static>], rest: &[Span<'static>]) -> Vec<Row> {
    let pieces = &mention_pieces(pieces);
    let pre_w = |p: &[Span<'static>]| p.iter().map(|s| s.content.width()).sum::<usize>();
    let mut rows = Vec::new();
    let mut cur: Vec<Span<'static>> = first.to_vec();
    let mut links: Vec<(u16, u16, String)> = Vec::new();
    let mut copies: Vec<(u16, u16, String)> = Vec::new();
    let mut col = pre_w(first);
    let mut limit = width.max(pre_w(first) + 1);
    let mut empty = true;
    // how the row in the works goes on from the row above (Row::wrap)
    let mut kind = 0u8;
    let push_row = |cur: &mut Vec<Span<'static>>, links: &mut Vec<(u16, u16, String)>, copies: &mut Vec<(u16, u16, String)>, rows: &mut Vec<Row>, kind: u8| {
        rows.push(Row { line: Line::from(std::mem::take(cur)), links: std::mem::take(links), copies: std::mem::take(copies), wrap: kind });
    };
    // split into words with their trailing spaces kept apart
    let mut words: Vec<(Piece, bool)> = Vec::new();
    for p in pieces {
        let mut buf = String::new();
        for c in p.text.chars() {
            if c == ' ' {
                if !buf.is_empty() {
                    words.push((Piece { text: std::mem::take(&mut buf), ..p.clone() }, false));
                }
                words.push((Piece { text: " ".into(), ..p.clone() }, true));
            } else {
                buf.push(c);
            }
        }
        if !buf.is_empty() {
            words.push((Piece { text: buf, ..p.clone() }, false));
        }
    }
    // a word glued to the next piece (`code`. / **bold**,): they wrap as 1
    let glued: Vec<usize> = {
        let mut out = vec![0; words.len()];
        let mut i = words.len();
        let mut run = 0;
        while i > 0 {
            i -= 1;
            run = if words[i].1 { 0 } else { run + words[i].0.text.width() };
            out[i] = run;
        }
        out
    };
    let mut prev_space = true;
    for (i, (w, space)) in words.into_iter().enumerate() {
        let ww = w.text.width();
        // the 1st piece of a glued run: the whole run to the next row when it fits there
        if !space && prev_space && glued[i] > ww && !empty && col + glued[i] > limit && glued[i] <= width.saturating_sub(pre_w(rest)) {
            if cur.last().is_some_and(|s| s.content == " ") {
                cur.pop();
            }
            push_row(&mut cur, &mut links, &mut copies, &mut rows, kind);
            kind = 1;
            cur = rest.to_vec();
            col = pre_w(rest);
            limit = width.max(col + 1);
            empty = true;
        }
        prev_space = space;
        if space {
            if !empty && col < limit {
                cur.push(Span::styled(" ", w.style));
                col += 1;
            }
            continue;
        }
        if !empty && col + ww > limit {
            // drop the trailing space of the row
            if cur.last().is_some_and(|s| s.content == " ") {
                cur.pop();
            }
            push_row(&mut cur, &mut links, &mut copies, &mut rows, kind);
            kind = 1;
            cur = rest.to_vec();
            col = pre_w(rest);
            limit = width.max(col + 1);
        }
        // a word longer than a row: cut it
        let mut text = w.text.clone();
        while col + text.width() > limit {
            let room = limit.saturating_sub(col).max(1);
            let mut cut = String::new();
            let mut cw = 0;
            for c in text.chars() {
                let x = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
                if cw + x > room {
                    break;
                }
                cut.push(c);
                cw += x;
            }
            if cut.is_empty() {
                break;
            }
            text = text[cut.len()..].to_string();
            if let Some(u) = &w.link {
                links.push((col as u16, (col + cw) as u16, u.clone()));
            }
            if let Some(c) = &w.copy {
                copies.push((col as u16, (col + cw) as u16, c.clone()));
            }
            cur.push(Span::styled(cut, w.style));
            push_row(&mut cur, &mut links, &mut copies, &mut rows, kind);
            kind = 2;
            cur = rest.to_vec();
            col = pre_w(rest);
            limit = width.max(col + 1);
        }
        if let Some(u) = &w.link {
            links.push((col as u16, (col + text.width()) as u16, u.clone()));
        }
        if let Some(c) = &w.copy {
            copies.push((col as u16, (col + text.width()) as u16, c.clone()));
        }
        col += text.width();
        cur.push(Span::styled(text, w.style));
        empty = false;
    }
    if !empty || rows.is_empty() {
        push_row(&mut cur, &mut links, &mut copies, &mut rows, kind);
    }
    rows
}

/// Plain TEXT wrapped to WIDTH in STYLE, PREFIX before every row.
pub fn plain(text: &str, width: usize, style: Style, prefix: &[Span<'static>]) -> Vec<Row> {
    text.split('\n').flat_map(|l| wrap(&[Piece { text: l.replace('\t', "    "), style, ..Default::default() }], width, prefix, prefix)).collect()
}

struct Block {
    pieces: Vec<Piece>,
    first: Vec<Span<'static>>,
    rest: Vec<Span<'static>>,
}

/// `file://<abs path>#L<line>` for inline code T that names an existing file: absolute, `~/…`,
/// or relative to the current dir (the thread view runs in the worktree), with an optional
/// `:line` / `:line:col`. None: 0 file (a word, a command, a dir).
pub fn file_link(t: &str) -> Option<String> {
    let t = t.trim();
    if t.is_empty() || t.contains(char::is_whitespace) || t.contains("://") || t.len() > 300 {
        return None;
    }
    // `path:12` / `path:12:3`
    let mut parts = t.splitn(3, ':');
    let path = parts.next()?;
    let line: Option<u32> = parts.next().and_then(|l| l.parse().ok());
    if !(path.contains('/') || path.contains('.')) {
        return None;
    }
    let p = match path.strip_prefix("~/") {
        Some(rest) => crate::config::home().join(rest),
        None => std::path::PathBuf::from(path),
    };
    let abs = if p.is_absolute() { p } else { std::env::current_dir().ok()?.join(p) };
    abs.is_file().then(|| format!("file://{}{}", abs.display(), line.map_or(String::new(), |l| format!("#L{l}"))))
}

/// MARKDOWN as rows of WIDTH, PREFIX before every row (the message's margin).
pub fn markdown(markdown: &str, width: usize, prefix: &[Span<'static>]) -> Vec<Row> {
    let mut out: Vec<Row> = Vec::new();
    let mut styles: Vec<Style> = vec![theme::text()];
    let mut link: Option<String> = None;
    let mut block: Option<Block> = None;
    // list stack: next number (None: bullets)
    let mut lists: Vec<Option<u64>> = Vec::new();
    let mut quote = 0usize;
    let mut code: Option<(String, String)> = None;
    let mut heading = false;
    let margin = |quote: usize, lists: usize| -> Vec<Span<'static>> {
        let mut v = prefix.to_vec();
        for _ in 0..quote {
            v.push(Span::styled("▎ ", theme::muted()));
        }
        if lists > 0 {
            v.push(Span::raw("  ".repeat(lists - 1)));
        }
        v
    };
    // a blank row between 2 blocks only where the source has a blank line (like the source reads)
    let gap = |out: &mut Vec<Row>, at: usize| {
        let before = &markdown[..at.min(markdown.len())];
        let blank = before[before.trim_end().len()..].matches('\n').count() >= 2;
        if blank && out.last().is_some_and(|r| r.line.width() > prefix.iter().map(|s| s.content.width()).sum::<usize>()) {
            out.push(Row::plain(Line::from(prefix.to_vec())));
        }
    };
    let flush = |block: &mut Option<Block>, out: &mut Vec<Row>| {
        if let Some(b) = block.take()
            && !b.pieces.is_empty()
        {
            out.extend(wrap(&b.pieces, width, &b.first, &b.rest));
        }
    };
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_TASKLISTS);
    // a table: its rows of cells (each cell: its pieces), rows of the head
    let mut table: Option<(Vec<Vec<Vec<Piece>>>, usize)> = None;
    for (ev, range) in Parser::new_ext(markdown, opts).into_offset_iter() {
        let at = range.start;
        let style = *styles.last().unwrap_or(&Style::default());
        match ev {
            Event::Start(Tag::Paragraph) => {
                if block.is_none() {
                    gap(&mut out, at);
                    let m = margin(quote, lists.len());
                    let rest = if lists.is_empty() { m.clone() } else { [m.clone(), vec![Span::raw("  ")]].concat() };
                    block = Some(Block { pieces: Vec::new(), first: if lists.is_empty() { m } else { rest.clone() }, rest });
                }
            }
            Event::End(TagEnd::Paragraph) => flush(&mut block, &mut out),
            Event::Start(Tag::Heading { level, .. }) => {
                gap(&mut out, at);
                heading = true;
                let mark = match level {
                    HeadingLevel::H1 => "# ",
                    HeadingLevel::H2 => "## ",
                    _ => "### ",
                };
                let m = margin(quote, 0);
                block = Some(Block { pieces: vec![Piece { text: mark.into(), style: theme::muted(), ..Default::default() }], first: m.clone(), rest: m });
                styles.push(theme::bold());
            }
            Event::End(TagEnd::Heading(_)) => {
                heading = false;
                styles.pop();
                flush(&mut block, &mut out);
            }
            Event::Start(Tag::BlockQuote(_)) => {
                flush(&mut block, &mut out);
                quote += 1;
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                flush(&mut block, &mut out);
                quote = quote.saturating_sub(1);
            }
            Event::Start(Tag::List(start)) => {
                flush(&mut block, &mut out);
                if lists.is_empty() {
                    gap(&mut out, at);
                }
                lists.push(start);
            }
            Event::End(TagEnd::List(_)) => {
                flush(&mut block, &mut out);
                lists.pop();
            }
            Event::Start(Tag::Item) => {
                flush(&mut block, &mut out);
                let m = margin(quote, lists.len());
                let mark = match lists.last_mut() {
                    Some(Some(n)) => {
                        let s = format!("{n}. ");
                        *n += 1;
                        s
                    }
                    _ => "• ".into(),
                };
                let rest = [m.clone(), vec![Span::raw(" ".repeat(mark.width()))]].concat();
                let first = [m, vec![Span::styled(mark, theme::muted())]].concat();
                block = Some(Block { pieces: Vec::new(), first, rest });
            }
            // an empty item: its marker alone (the view shows what was sent)
            Event::End(TagEnd::Item) => match block.take() {
                Some(b) if b.pieces.is_empty() => out.push(Row::plain(Line::from(b.first))),
                b => {
                    block = b;
                    flush(&mut block, &mut out);
                }
            },
            Event::TaskListMarker(done) => {
                if let Some(b) = block.as_mut() {
                    b.pieces.push(Piece { text: if done { "☑ ".into() } else { "☐ ".into() }, style: theme::muted(), ..Default::default() });
                }
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                flush(&mut block, &mut out);
                gap(&mut out, at);
                let lang = match kind {
                    CodeBlockKind::Fenced(l) => l.split(|c: char| c.is_whitespace() || c == ',').next().unwrap_or_default().to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                code = Some((lang, String::new()));
            }
            Event::End(TagEnd::CodeBlock) => {
                if let Some((lang, text)) = code.take() {
                    out.extend(code_rows(&text, &lang, width, &margin(quote, lists.len()), true, None));
                }
            }
            Event::Start(Tag::Emphasis) => styles.push(style.add_modifier(Modifier::ITALIC)),
            Event::Start(Tag::Strong) => styles.push(style.add_modifier(Modifier::BOLD)),
            Event::Start(Tag::Strikethrough) => styles.push(style.add_modifier(Modifier::CROSSED_OUT)),
            Event::Start(Tag::Image { dest_url, .. }) => {
                let b = block.get_or_insert_with(|| {
                    let m = margin(quote, lists.len());
                    Block { pieces: Vec::new(), first: m.clone(), rest: m }
                });
                b.pieces.push(Piece { text: "[image: ".into(), style: theme::muted(), link: Some(dest_url.to_string()), ..Default::default() });
                link = Some(dest_url.to_string());
                styles.push(theme::muted());
            }
            Event::End(TagEnd::Image) => {
                styles.pop();
                if let Some(b) = block.as_mut() {
                    b.pieces.push(Piece { text: "]".into(), style: theme::muted(), link: link.clone(), ..Default::default() });
                }
                link = None;
            }
            Event::Start(Tag::Link { dest_url, .. }) => {
                link = Some(dest_url.to_string());
                styles.push(style.fg(theme::pal().info()).add_modifier(Modifier::UNDERLINED));
            }
            Event::End(TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough) => {
                styles.pop();
            }
            Event::End(TagEnd::Link) => {
                styles.pop();
                link = None;
            }
            Event::Start(Tag::Table(_)) => {
                flush(&mut block, &mut out);
                gap(&mut out, at);
                table = Some((Vec::new(), 0));
            }
            Event::Start(Tag::TableHead | Tag::TableRow) => {
                if let Some((rows, _)) = table.as_mut() {
                    rows.push(Vec::new());
                }
                if matches!(ev, Event::Start(Tag::TableHead)) {
                    styles.push(style.add_modifier(Modifier::BOLD));
                }
            }
            Event::End(TagEnd::TableHead) => {
                styles.pop();
                if let Some((rows, head)) = table.as_mut() {
                    *head = rows.len();
                }
            }
            Event::Start(Tag::TableCell) => block = Some(Block { pieces: Vec::new(), first: Vec::new(), rest: Vec::new() }),
            Event::End(TagEnd::TableCell) => {
                if let (Some((rows, _)), Some(b)) = (table.as_mut(), block.take())
                    && let Some(row) = rows.last_mut()
                {
                    row.push(b.pieces);
                }
            }
            Event::End(TagEnd::Table) => {
                if let Some((rows, head)) = table.take() {
                    out.extend(table_rows(&rows, head, width, &margin(quote, lists.len())));
                }
            }
            Event::Text(t) => {
                if let Some((_, text)) = code.as_mut() {
                    text.push_str(&t);
                    continue;
                }
                let b = block.get_or_insert_with(|| {
                    let m = margin(quote, lists.len());
                    Block { pieces: Vec::new(), first: m.clone(), rest: m }
                });
                let st = if heading { theme::bold() } else { style };
                // a bare URL in text: clickable too
                for (i, part) in t.split(' ').enumerate() {
                    if i > 0 {
                        b.pieces.push(Piece { text: " ".into(), style: st, ..Default::default() });
                    }
                    // `(https://…)`, `<https://…>`, `"https://…"`: the URL without the marks around it
                    let bare = part.find("https://").or_else(|| part.find("http://")).map(|i| part[i..].trim_end_matches(['.', ',', ')', ';', ':', '>', '"', '\'', ']']).to_string());
                    let url = link.clone().or(bare);
                    let st = if url.is_some() && link.is_none() { st.fg(theme::pal().info()).add_modifier(Modifier::UNDERLINED) } else { st };
                    b.pieces.push(Piece { text: part.to_string(), style: st, link: url, ..Default::default() });
                }
            }
            Event::Code(t) => {
                let b = block.get_or_insert_with(|| {
                    let m = margin(quote, lists.len());
                    Block { pieces: Vec::new(), first: m.clone(), rest: m }
                });
                // a panel background: inline code never reads as a link (same blue, links underlined);
                // a path of an existing file: a file link (a click opens it in the files view)
                let file = link.is_none().then(|| file_link(&t)).flatten();
                let style = Style::default().fg(code_fg()).bg(theme::pal().surface0);
                let style = if file.is_some() { style.add_modifier(Modifier::UNDERLINED) } else { style };
                let copy = file.is_none().then(|| t.to_string());
                b.pieces.push(Piece { text: t.to_string(), style, link: file.or_else(|| link.clone()), copy });
            }
            // a line break of the source stays a line break (agents write 1 statement per line)
            Event::SoftBreak | Event::HardBreak => {
                if let Some(b) = block.as_mut() {
                    let (first, rest) = (b.first.clone(), b.rest.clone());
                    let pieces = std::mem::take(&mut b.pieces);
                    out.extend(wrap(&pieces, width, &first, &rest));
                    b.first = rest;
                }
            }
            Event::Rule => {
                flush(&mut block, &mut out);
                let m = margin(quote, 0);
                let w = width.saturating_sub(m.iter().map(|s| s.content.width()).sum());
                out.push(Row::plain(Line::from([m, vec![Span::styled("─".repeat(w.min(40)), theme::line())]].concat())));
            }
            Event::Html(t) | Event::InlineHtml(t) => {
                let b = block.get_or_insert_with(|| {
                    let m = margin(quote, lists.len());
                    Block { pieces: Vec::new(), first: m.clone(), rest: m }
                });
                b.pieces.push(Piece { text: t.trim_end_matches('\n').to_string(), style: theme::muted(), ..Default::default() });
            }
            _ => {}
        }
    }
    flush(&mut block, &mut out);
    if let Some((lang, text)) = code.take() {
        out.extend(code_rows(&text, &lang, width, prefix, true, None));
    }
    out
}

/// A Markdown table: columns as wide as their widest cell, shrunk to fit WIDTH (cells wrap),
/// ` │ ` between columns, a `─┼─` line below the head rows.
fn table_rows(rows: &[Vec<Vec<Piece>>], head: usize, width: usize, margin: &[Span<'static>]) -> Vec<Row> {
    let cols = rows.iter().map(Vec::len).max().unwrap_or(0);
    if cols == 0 {
        return Vec::new();
    }
    let mw: usize = margin.iter().map(|s| s.content.width()).sum();
    let room = width.saturating_sub(mw + 3 * (cols - 1)).max(cols);
    let cell_w = |c: &[Piece]| c.iter().map(|p| p.text.width()).sum::<usize>();
    let natural: Vec<usize> = (0..cols).map(|i| rows.iter().filter_map(|r| r.get(i)).map(|c| cell_w(c)).max().unwrap_or(0).max(1)).collect();
    // narrow columns keep their width, the others share the room left
    let mut w = natural.clone();
    if natural.iter().sum::<usize>() > room {
        let mut left = room;
        let mut open: Vec<usize> = (0..cols).collect();
        loop {
            let share = left / open.len().max(1);
            let (fit, wide): (Vec<usize>, Vec<usize>) = open.iter().partition(|&&i| natural[i] <= share);
            if fit.is_empty() {
                for (k, &i) in wide.iter().enumerate() {
                    w[i] = (share + usize::from(k < left % wide.len().max(1))).max(1);
                }
                break;
            }
            for &i in &fit {
                w[i] = natural[i];
                left -= natural[i];
            }
            open = wide;
            if open.is_empty() {
                break;
            }
        }
    }
    let sep = Span::styled(" │ ", theme::line());
    let mut out = Vec::new();
    for (ri, r) in rows.iter().enumerate() {
        let cells: Vec<Vec<Row>> = (0..cols).map(|i| r.get(i).map_or_else(Vec::new, |c| wrap(c, w[i], &[], &[]))).collect();
        let h = cells.iter().map(Vec::len).max().unwrap_or(1).max(1);
        for k in 0..h {
            let mut spans = margin.to_vec();
            let mut row = Row::default();
            let mut x = mw;
            for i in 0..cols {
                if i > 0 {
                    spans.push(sep.clone());
                    x += 3;
                }
                let cell = cells[i].get(k);
                let cw = cell.map_or(0, |c| c.line.width());
                if let Some(c) = cell {
                    spans.extend(c.line.spans.iter().cloned());
                    row.links.extend(c.links.iter().map(|(a, b, u)| (a + x as u16, b + x as u16, u.clone())));
                    row.copies.extend(c.copies.iter().map(|(a, b, u)| (a + x as u16, b + x as u16, u.clone())));
                }
                if i + 1 < cols {
                    spans.push(Span::raw(" ".repeat(w[i].saturating_sub(cw))));
                }
                x += w[i];
            }
            row.line = Line::from(spans);
            out.push(row);
        }
        if ri + 1 == head {
            let line: Vec<String> = w.iter().map(|n| "─".repeat(*n)).collect();
            out.push(Row::plain(Line::from([margin.to_vec(), vec![Span::styled(line.join("─┼─"), theme::line())]].concat())));
        }
    }
    out
}

fn lang_w(lang: &str) -> usize {
    if lang == "text" { 0 } else { lang.width() + 1 }
}

/// A code block: syntax colors on the panel color (FG: 1 color for every char instead), a head row
/// with the language + a copy button. Long lines: cut at the row end, or WRAP (at a word end, `↪ ` rows).
/// Code blocks up to this many lines get syntax colors (1 pass each, cached).
const MAX_HIGHLIGHT: usize = 4000;

pub fn code_rows(text: &str, lang: &str, width: usize, margin: &[Span<'static>], wrap: bool, fg: Option<Color>) -> Vec<Row> {
    let lang = if lang.is_empty() { crate::code::language(text, &[]) } else { lang };
    let lines: Vec<&str> = text.trim_end_matches('\n').split('\n').collect();
    let mw: usize = margin.iter().map(|s| s.content.width()).sum();
    let room = width.saturating_sub(mw + 2).max(1);
    let bg = Style::default().bg(theme::pal().surface0);
    let mut out = Vec::new();
    // head row: the language left, a copy button right
    let label = if lang == "text" { String::new() } else { format!(" {lang}") };
    let button = " copy ";
    let fill = (room + 2).saturating_sub(label.width() + button.width());
    let mut head = margin.to_vec();
    head.push(Span::styled(label, bg.fg(theme::pal().subtext0)));
    head.push(Span::styled(" ".repeat(fill), bg));
    head.push(Span::styled(button, bg.fg(theme::pal().subtext0)));
    let at = (mw + fill + lang_w(lang)) as u16;
    out.push(Row { line: Line::from(head), copies: vec![(at, at + button.width() as u16, text.trim_end_matches('\n').to_string())], ..Default::default() });
    // 1 highlight pass per block (cached); very long blocks get plain rows
    let colored = (fg.is_none() && lines.len() <= MAX_HIGHLIGHT).then(|| crate::code::highlight(&lines.join("\n"), lang));
    for (i, l) in lines.iter().enumerate() {
        let chars: Vec<(Color, char)> = match (fg, colored.as_ref().and_then(|c| c.get(i))) {
            (Some(fg), _) => l.chars().filter(|c| !c.is_control()).map(|c| (fg, c)).collect(),
            (None, Some(c)) => c.clone(),
            (None, None) => l.chars().map(|c| (theme::pal().text, c)).collect(),
        };
        let segments: Vec<std::ops::Range<usize>> = if wrap { wrap_code(&chars, room) } else { std::iter::once(0..cut_at(&chars, room)).collect() };
        for (k, seg) in segments.into_iter().enumerate() {
            let mut spans = margin.to_vec();
            spans.push(Span::styled(" ", bg));
            let mut w = 0;
            // a continuation row: `↪ ` (the line goes on, 0 new line in the source)
            if k > 0 {
                spans.push(Span::styled(CONT, bg.fg(theme::pal().subtext0)));
                w += CONT.width();
            }
            let mut run = String::new();
            let mut run_color = None;
            for &(color, c) in &chars[seg] {
                if run_color != Some(color) && !run.is_empty() {
                    spans.push(Span::styled(std::mem::take(&mut run), bg.fg(run_color.unwrap_or(theme::pal().text))));
                }
                run_color = Some(color);
                run.push(c);
                w += unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
            }
            if !run.is_empty() {
                spans.push(Span::styled(run, bg.fg(run_color.unwrap_or(theme::pal().text))));
            }
            spans.push(Span::styled(" ".repeat((room + 1).saturating_sub(w)), bg));
            out.push(Row::plain(Line::from(spans)));
        }
    }
    out
}

/// The mark of a wrapped code line's continuation rows.
const CONT: &str = "↪ ";

fn char_w(c: char) -> usize {
    unicode_width::UnicodeWidthChar::width(c).unwrap_or(0)
}

/// End of the chars of CHARS that fit ROOM columns.
fn cut_at(chars: &[(Color, char)], room: usize) -> usize {
    let mut w = 0;
    chars.iter().position(|&(_, c)| {
        w += char_w(c);
        w > room
    }).unwrap_or(chars.len())
}

/// Rows of 1 code line CHARS in ROOM columns: the 1st row full width, the next ones after `↪ `.
/// A row ends after a space or a separator (`,` `;` `)` `/` …) in its 2nd half, else at the room.
fn wrap_code(chars: &[(Color, char)], room: usize) -> Vec<std::ops::Range<usize>> {
    let mut out = Vec::new();
    let mut s = 0;
    loop {
        let budget = if out.is_empty() { room } else { room.saturating_sub(CONT.width()).max(1) };
        let e = s + cut_at(&chars[s..], budget);
        if e >= chars.len() {
            out.push(s..chars.len());
            return out;
        }
        let half = s + (e - s) / 2;
        let brk = (half + 1..=e).rev().find(|&k| chars[k - 1].1.is_whitespace() || ",;:)]}/|&=>.-".contains(chars[k - 1].1));
        // 0 chars fit (a wide char in a 1-column room): take 1 anyway
        let end = brk.unwrap_or(e).max(s + 1);
        out.push(s..end);
        s = end;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(rows: &[Row]) -> Vec<String> {
        rows.iter().map(|r| r.line.spans.iter().map(|s| s.content.as_ref()).collect::<String>().trim_end().to_string()).collect()
    }

    #[test]
    fn empty_list_items_show_their_marker() {
        let t = text(&markdown("1. a\n2. \n3. c", 40, &[]));
        assert_eq!(t.iter().filter(|l| !l.is_empty()).cloned().collect::<Vec<_>>(), ["1. a", "2.", "3. c"], "{t:#?}");
    }

    #[test]
    fn long_code_lines_wrap_at_word_ends_with_a_mark() {
        let md = "```md\n# HOME REPO\n- home repo: `~/projects/doinstruct/bonfire`. Its rules (notes, other repos): @~/projects/doinstruct/bonfire/AGENTS.md\n```";
        let rows = markdown(md, 50, &[]);
        let t = text(&rows);
        // head row, the short line, the long line on several rows: 0 chars lost
        assert!(t.len() > 3, "{t:#?}");
        assert!(t[2].starts_with(" - home repo:"), "{t:#?}");
        assert!(t[3..].iter().all(|l| l.starts_with(" ↪ ")), "{t:#?}");
        let joined: String = t[2..].iter().flat_map(|l| l.trim_start().trim_start_matches("↪ ").chars()).filter(|c| !c.is_whitespace()).collect();
        assert!(joined.ends_with("@~/projects/doinstruct/bonfire/AGENTS.md"), "{joined}");
        // breaks after a space or a separator, 0 cut inside `repo:`
        assert!(t[2].trim_end().ends_with(['.', ' ', '/', ',', ':', ')', '`']) || t[2].trim_end().ends_with("bonfire"), "{t:#?}");
        // every row as wide as the block
        let w = rows[0].line.width();
        assert!(rows.iter().all(|r| r.line.width() == w), "{t:#?}");
        // the copy button copies the source lines, 0 marks
        assert_eq!(rows[0].copies[0].2, "# HOME REPO\n- home repo: `~/projects/doinstruct/bonfire`. Its rules (notes, other repos): @~/projects/doinstruct/bonfire/AGENTS.md");
    }

    #[test]
    fn wrap_code_breaks_hard_without_a_word_end() {
        let chars: Vec<(Color, char)> = "abcdefghijklmnopqrstuvwxyz".chars().map(|c| (Color::Reset, c)).collect();
        let segs = wrap_code(&chars, 10);
        assert_eq!(segs, [0..10, 10..18, 18..26]);
    }

    #[test]
    fn inline_code_of_an_existing_file_is_a_file_link() {
        let d = std::env::temp_dir().join(format!("codehead-filelink-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("plan.md");
        std::fs::write(&f, "x").unwrap();
        let abs = f.display().to_string();
        assert_eq!(file_link(&abs), Some(format!("file://{abs}")));
        assert_eq!(file_link(&format!("{abs}:12")), Some(format!("file://{abs}#L12")));
        assert_eq!(file_link("cargo test"), None);
        assert_eq!(file_link(&d.display().to_string()), None, "a dir: 0 link");
        assert_eq!(file_link("/no/such/file.rs"), None);
        let rows = markdown(&format!("see `{abs}:3` and `cargo test`"), 200, &[]);
        assert_eq!(rows[0].links.len(), 1);
        assert!(rows[0].links[0].2.ends_with("plan.md#L3"));
        assert!(rows[0].copies.iter().any(|c| c.2 == "cargo test"), "plain code still copies");
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn inline_code_has_a_background_links_do_not() {
        let rows = markdown("run `bun run link-branch` or see [docs](https://x.dev) now", 80, &[]);
        // per cell: (char, bg)
        let cells: Vec<(char, Option<Color>)> = rows[0].line.spans.iter().flat_map(|s| s.content.chars().map(move |c| (c, s.style.bg))).collect();
        let text: String = cells.iter().map(|c| c.0).collect();
        let at = text.find("bun run link-branch").unwrap();
        let code = &cells[at..at + "bun run link-branch".len()];
        assert!(code.iter().all(|c| c.1 == Some(theme::pal().surface0)), "every cell, spaces too: {code:?}");
        assert_eq!(cells[at - 1].1, None, "0 bg before the code");
        let link = text.find("docs").unwrap();
        assert_eq!(cells[link].1, None, "a link: 0 bg");
    }

    #[test]
    fn wraps_lists_code_and_links() {
        let md = "Some **bold** text that wraps here.\n\n- one\n- two\n\n```rust\nfn main() {}\n```\n\nsee [docs](https://x.dev) now";
        let rows = markdown(md, 20, &[Span::raw("  ")]);
        let t = text(&rows);
        assert_eq!(t[0], "  Some bold text");
        assert!(t.contains(&"  • one".to_string()), "{t:?}");
        assert!(t.iter().any(|l| l.contains("fn main() {}")), "{t:?}");
        let link = rows.iter().find_map(|r| r.links.first().cloned()).unwrap();
        assert_eq!(link.2, "https://x.dev");
        assert!(rows.iter().all(|r| r.line.width() <= 20), "{t:?}");
    }

    #[test]
    fn line_breaks_blank_lines_and_tables() {
        // a source line break stays, a list right below a line: 0 blank row
        let t = text(&markdown("※ SUMMARY\nDONE. it works.\n\n✱ INFO\n- a\n- b", 40, &[]));
        assert_eq!(t, vec!["※ SUMMARY", "DONE. it works.", "", "✱ INFO", "• a", "• b"]);
        let md = "| # | change | effect |\n|---|---|---|\n| 1 | a long change text here | short |";
        let rows = markdown(md, 30, &[Span::raw("  ")]);
        let t = text(&rows);
        assert_eq!(t[0], "  # │ change          │ effect");
        assert!(t[1].starts_with("  ──┼─"), "{t:?}");
        assert_eq!(t[2], "  1 │ a long change   │ short");
        assert_eq!(t[3], "    │ text here       │");
        assert!(rows.iter().all(|r| r.line.width() <= 30), "{t:?}");
    }

    #[test]
    fn glued_pieces_wrap_together() {
        let t = text(&markdown("now uses `bun x:install`.", 20, &[]));
        assert_eq!(t, vec!["now uses bun", "x:install."]);
    }

    #[test]
    fn long_words_cut() {
        let rows = plain("abcdefghijklmnop", 6, theme::text(), &[]);
        assert_eq!(text(&rows), vec!["abcdef", "ghijkl", "mnop"]);
    }
}
