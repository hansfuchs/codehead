use super::{
    Block, CodeBlockKind, Event, HeadingLevel, HtmlPart, Line, Modifier, Options, Parser, Piece, Row, Span, Style, Tag,
    TagEnd, UnicodeWidthStr, code_fg, code_rows, entities, file_link, html_attr, html_parts, html_words, table_rows,
    theme, wrap,
};
#[path = "markdown_parser_blocks.rs"]
mod blocks;
#[path = "markdown_parser_html.rs"]
mod html;
#[path = "markdown_parser_inline.rs"]
mod inline;
#[path = "markdown_parser_markdown_tables.rs"]
mod tables;
use blocks::{
    end_heading, end_item, end_list, end_quote, rule, start_heading, start_html, start_item, start_list,
    start_paragraph, start_quote, task_marker,
};
use html::html_event;
use inline::{end_code, end_image, inline_code, line_break, start_code, start_image, start_link, text_event};
use tables::{end_table, end_table_cell, end_table_head, start_table, start_table_cell, start_table_row};

struct MarkdownState<'source> {
    markdown: &'source str,
    width: usize,
    prefix: &'source [Span<'static>],
    out: Vec<Row>,
    styles: Vec<Style>,
    link: Option<String>,
    block: Option<Block>,
    lists: Vec<Option<u64>>,
    quote: usize,
    code: Option<(String, String)>,
    heading: bool,
    table: Option<(Vec<Vec<Vec<Piece>>>, usize)>,
    html_open: Vec<String>,
}

pub(crate) fn markdown(markdown: &str, width: usize, prefix: &[Span<'static>]) -> Vec<Row> {
    let mut state = MarkdownState {
        markdown,
        width,
        prefix,
        out: Vec::new(),
        styles: vec![theme::text()],
        link: None,
        block: None,
        lists: Vec::new(),
        quote: 0,
        code: None,
        heading: false,
        table: None,
        html_open: Vec::new(),
    };
    let options = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS;

    for (event, range) in Parser::new_ext(markdown, options).into_offset_iter() {
        render_event(&mut state, event, range.start);
    }

    flush(&mut state);

    if let Some((language, text)) = state.code.take() {
        state.out.extend(code_rows(&text, &language, width, prefix, true, None));
    }

    state.out
}

fn render_event(state: &mut MarkdownState<'_>, event: Event<'_>, at: usize) {
    let style = *state.styles.last().unwrap_or(&Style::default());

    match event {
        Event::Start(tag) => start_tag(state, tag, style, at),
        Event::End(tag) => end_tag(state, tag),
        Event::TaskListMarker(done) => task_marker(state, done),
        Event::Text(text) => text_event(state, &text, style),
        Event::Code(text) => inline_code(state, &text),
        Event::SoftBreak | Event::HardBreak => line_break(state),
        Event::Rule => rule(state),
        Event::Html(text) | Event::InlineHtml(text) => html_event(state, &text),
        _ => {}
    }
}

fn start_tag(state: &mut MarkdownState<'_>, tag: Tag<'_>, style: Style, at: usize) {
    match tag {
        Tag::Paragraph => start_paragraph(state, at),
        Tag::Heading { level, .. } => start_heading(state, level, at),
        Tag::BlockQuote(_) => start_quote(state),
        Tag::List(start) => start_list(state, start, at),
        Tag::Item => start_item(state),
        Tag::CodeBlock(kind) => start_code(state, kind, at),
        Tag::Emphasis => state.styles.push(style.add_modifier(Modifier::ITALIC)),
        Tag::Strong => state.styles.push(style.add_modifier(Modifier::BOLD)),
        Tag::Strikethrough => state.styles.push(style.add_modifier(Modifier::CROSSED_OUT)),
        Tag::Image { dest_url, .. } => start_image(state, &dest_url),
        Tag::Link { dest_url, .. } => start_link(state, &dest_url, style),
        Tag::Table(_) => start_table(state, at),
        Tag::TableHead => start_table_row(state, style, true),
        Tag::TableRow => start_table_row(state, style, false),
        Tag::TableCell => start_table_cell(state),
        Tag::HtmlBlock => start_html(state, at),
        _ => {}
    }
}

fn end_tag(state: &mut MarkdownState<'_>, tag: TagEnd) {
    match tag {
        TagEnd::Paragraph | TagEnd::HtmlBlock => flush(state),
        TagEnd::Heading(_) => end_heading(state),
        TagEnd::BlockQuote(_) => end_quote(state),
        TagEnd::List(_) => end_list(state),
        TagEnd::Item => end_item(state),
        TagEnd::CodeBlock => end_code(state),
        TagEnd::Image => end_image(state),
        TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => {
            state.styles.pop();
        }
        TagEnd::Link => {
            state.styles.pop();
            state.link = None;
        }
        TagEnd::TableHead => end_table_head(state),
        TagEnd::TableCell => end_table_cell(state),
        TagEnd::Table => end_table(state),
        _ => {}
    }
}

fn margin(prefix: &[Span<'static>], quote: usize, lists: usize) -> Vec<Span<'static>> {
    let mut margin = prefix.to_vec();

    for _ in 0..quote {
        margin.push(Span::styled("▎ ", theme::muted()));
    }

    if lists > 0 {
        margin.push(Span::raw("  ".repeat(lists - 1)));
    }

    margin
}

fn gap(state: &mut MarkdownState<'_>, at: usize) {
    let before = &state.markdown[..at.min(state.markdown.len())];
    let blank = before[before.trim_end().len()..].matches('\n').count() >= 2;
    let width = state.prefix.iter().map(|span| span.content.width()).sum::<usize>();

    if blank && state.out.last().is_some_and(|row| row.line.width() > width) {
        state.out.push(Row::plain(Line::from(state.prefix.to_vec())));
    }
}

fn flush(state: &mut MarkdownState<'_>) {
    if let Some(block) = state.block.take()
        && !block.pieces.is_empty()
    {
        state
            .out
            .extend(wrap(&block.pieces, state.width, &block.first, &block.rest));
    }
}
