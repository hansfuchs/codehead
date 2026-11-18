//! Markdown → styled terminal rows, wrapped to a width: the agents' messages in the thread view.
//! Code blocks get syntax colors (`code::last_line`), links stay clickable (their rows carry the
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
}

impl Row {
    pub fn plain(line: Line<'static>) -> Row {
        Row { line, ..Default::default() }
    }
}

const CODE: Color = Color::Indexed(110);

/// A styled word piece; LINK: the url it belongs to; COPY: what a click on it copies.
#[derive(Clone, Default)]
struct Piece {
    text: String,
    style: Style,
    link: Option<String>,
    copy: Option<String>,
}

/// Wrap PIECES into rows of WIDTH: FIRST before the 1st row, REST before the others.
fn wrap(pieces: &[Piece], width: usize, first: &[Span<'static>], rest: &[Span<'static>]) -> Vec<Row> {
    let pre_w = |p: &[Span<'static>]| p.iter().map(|s| s.content.width()).sum::<usize>();
    let mut rows = Vec::new();
    let mut cur: Vec<Span<'static>> = first.to_vec();
    let mut links: Vec<(u16, u16, String)> = Vec::new();
    let mut copies: Vec<(u16, u16, String)> = Vec::new();
    let mut col = pre_w(first);
    let mut limit = width.max(pre_w(first) + 1);
    let mut empty = true;
    let push_row = |cur: &mut Vec<Span<'static>>, links: &mut Vec<(u16, u16, String)>, copies: &mut Vec<(u16, u16, String)>, rows: &mut Vec<Row>| {
        rows.push(Row { line: Line::from(std::mem::take(cur)), links: std::mem::take(links), copies: std::mem::take(copies) });
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
            push_row(&mut cur, &mut links, &mut copies, &mut rows);
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
            push_row(&mut cur, &mut links, &mut copies, &mut rows);
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
            push_row(&mut cur, &mut links, &mut copies, &mut rows);
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
        push_row(&mut cur, &mut links, &mut copies, &mut rows);
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
                styles.push(theme::accent_bold());
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
                let first = [m, vec![Span::styled(mark, theme::accent())]].concat();
                block = Some(Block { pieces: Vec::new(), first, rest });
            }
            Event::End(TagEnd::Item) => flush(&mut block, &mut out),
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
                    out.extend(code_rows(&text, &lang, width, &margin(quote, lists.len())));
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
                styles.push(style.fg(theme::pal().accent).add_modifier(Modifier::UNDERLINED));
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
                let st = if heading { theme::accent_bold() } else { style };
                // a bare URL in text: clickable too
                for (i, part) in t.split(' ').enumerate() {
                    if i > 0 {
                        b.pieces.push(Piece { text: " ".into(), style: st, ..Default::default() });
                    }
                    let url = link.clone().or_else(|| (part.starts_with("https://") || part.starts_with("http://")).then(|| part.trim_end_matches(['.', ',', ')', ';']).to_string()));
                    let st = if url.is_some() && link.is_none() { st.fg(theme::pal().accent).add_modifier(Modifier::UNDERLINED) } else { st };
                    b.pieces.push(Piece { text: part.to_string(), style: st, link: url, ..Default::default() });
                }
            }
            Event::Code(t) => {
                let b = block.get_or_insert_with(|| {
                    let m = margin(quote, lists.len());
                    Block { pieces: Vec::new(), first: m.clone(), rest: m }
                });
                b.pieces.push(Piece { text: t.to_string(), style: Style::default().fg(CODE), link: link.clone(), copy: Some(t.to_string()) });
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
        out.extend(code_rows(&text, &lang, width, prefix));
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

/// A code block: syntax colors on the panel color, cut (not wrapped) at the row end.
fn code_rows(text: &str, lang: &str, width: usize, margin: &[Span<'static>]) -> Vec<Row> {
    let lang = if lang.is_empty() { crate::code::language(text, &[]) } else { lang };
    let lines: Vec<&str> = text.trim_end_matches('\n').split('\n').collect();
    let mw: usize = margin.iter().map(|s| s.content.width()).sum();
    let room = width.saturating_sub(mw + 2).max(1);
    let bg = Style::default().bg(theme::pal().panel);
    let mut out = Vec::new();
    // head row: the language left, a copy button right
    let label = if lang == "text" { String::new() } else { format!(" {lang}") };
    let button = " copy ";
    let fill = (room + 2).saturating_sub(label.width() + button.width());
    let mut head = margin.to_vec();
    head.push(Span::styled(label, bg.fg(theme::pal().muted)));
    head.push(Span::styled(" ".repeat(fill), bg));
    head.push(Span::styled(button, bg.fg(theme::pal().accent)));
    let at = (mw + fill + lang_w(lang)) as u16;
    out.push(Row { line: Line::from(head), copies: vec![(at, at + button.width() as u16, text.trim_end_matches('\n').to_string())], ..Default::default() });
    let mut so_far = String::new();
    for (i, l) in lines.iter().enumerate() {
        if i > 0 {
            so_far.push('\n');
        }
        so_far.push_str(l);
        // colors need the lines before (strings, comments): long blocks get plain rows
        let chars: Vec<(Color, char)> = if lines.len() <= 400 { crate::code::last_line(&so_far, lang) } else { l.chars().map(|c| (theme::pal().text, c)).collect() };
        let mut spans = margin.to_vec();
        spans.push(Span::styled(" ", bg));
        let mut w = 0;
        let mut run = String::new();
        let mut run_color = None;
        for (color, c) in chars {
            let cw = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
            if w + cw > room {
                break;
            }
            if run_color != Some(color) && !run.is_empty() {
                spans.push(Span::styled(std::mem::take(&mut run), bg.fg(run_color.unwrap_or(theme::pal().text))));
            }
            run_color = Some(color);
            run.push(c);
            w += cw;
        }
        if !run.is_empty() {
            spans.push(Span::styled(run, bg.fg(run_color.unwrap_or(theme::pal().text))));
        }
        spans.push(Span::styled(" ".repeat(room - w + 1), bg));
        out.push(Row::plain(Line::from(spans)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(rows: &[Row]) -> Vec<String> {
        rows.iter().map(|r| r.line.spans.iter().map(|s| s.content.as_ref()).collect::<String>().trim_end().to_string()).collect()
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
