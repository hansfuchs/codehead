use super::{
    Block, HtmlPart, MarkdownState, Modifier, Style, code_fg, entities, html_attr, html_parts, html_words, margin,
    theme, wrap,
};

pub(super) fn html_event(state: &mut MarkdownState<'_>, text: &str) {
    if state.block.is_none() {
        let margin = margin(state.prefix, state.quote, state.lists.len());

        state.block = Some(Block {
            pieces: Vec::new(),
            first: margin.clone(),
            rest: margin,
        });
    }

    for part in html_parts(text) {
        html_part(state, part);
    }
}

fn html_part(state: &mut MarkdownState<'_>, part: HtmlPart<'_>) {
    let style = *state.styles.last().unwrap_or(&Style::default());

    match part {
        HtmlPart::Text(text) => {
            if let Some(block) = state.block.as_mut() {
                html_words(block, &entities(text), style, state.link.as_deref());
            }
        }
        HtmlPart::Tag(name, close, raw) => html_tag(state, &name, close, raw, style),
    }
}

fn html_tag(state: &mut MarkdownState<'_>, name: &str, close: bool, raw: &str, style: Style) {
    match name {
        "a" if !close => {
            state.html_open.push(name.to_string());
            state.link = html_attr(raw, "href");
            state
                .styles
                .push(style.fg(theme::pal().info()).add_modifier(Modifier::UNDERLINED));
        }
        "b" | "strong" | "i" | "em" | "s" | "del" | "code" | "kbd" if !close => {
            state.html_open.push(name.to_string());
            state.styles.push(html_style(name, style));
        }
        _ if close && state.html_open.last().is_some_and(|open| open == name) => close_html_tag(state, name),
        "img" => html_image(state, raw),
        "br" | "hr" | "p" | "div" | "tr" | "li" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "summary" | "details"
        | "table" | "ul" | "ol" | "pre" | "blockquote" => html_break(state, style),
        "td" | "th" => {
            if let Some(block) = state.block.as_mut() {
                html_words(block, " ", style, None);
            }
        }
        _ => {}
    }
}

fn html_style(name: &str, style: Style) -> Style {
    match name {
        "b" | "strong" => style.add_modifier(Modifier::BOLD),
        "i" | "em" => style.add_modifier(Modifier::ITALIC),
        "s" | "del" => style.add_modifier(Modifier::CROSSED_OUT),
        _ => Style::default().fg(code_fg()).bg(theme::pal().surface0),
    }
}

fn close_html_tag(state: &mut MarkdownState<'_>, name: &str) {
    state.html_open.pop();
    state.styles.pop();

    if name == "a" {
        state.link = None;
    }
}

fn html_image(state: &mut MarkdownState<'_>, raw: &str) {
    let Some(alt) = html_attr(raw, "alt").filter(|alt| !alt.trim().is_empty()) else {
        return;
    };

    if let Some(block) = state.block.as_mut() {
        html_words(
            block,
            &format!("[image: {}]", alt.trim()),
            theme::muted(),
            state.link.as_deref(),
        );
    }
}

fn html_break(state: &mut MarkdownState<'_>, style: Style) {
    let Some(block) = state.block.as_mut() else {
        return;
    };

    if state.table.is_some() {
        html_words(block, " ", style, None);
        return;
    }

    if block.pieces.is_empty() {
        return;
    }

    let (first, rest) = (block.first.clone(), block.rest.clone());
    let pieces = std::mem::take(&mut block.pieces);

    state.out.extend(wrap(&pieces, state.width, &first, &rest));
    block.first = rest;
}
