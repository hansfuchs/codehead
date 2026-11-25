use super::{
    Block, CodeBlockKind, MarkdownState, Modifier, Piece, Style, code_fg, code_rows, file_link, flush, gap, margin,
    theme, wrap,
};

pub(super) fn start_code(state: &mut MarkdownState<'_>, kind: CodeBlockKind<'_>, at: usize) {
    flush(state);
    gap(state, at);

    let lang = match kind {
        CodeBlockKind::Fenced(info) => info
            .split(|character: char| character.is_whitespace() || character == ',')
            .next()
            .unwrap_or_default()
            .to_string(),
        CodeBlockKind::Indented => String::new(),
    };

    state.code = Some((lang, String::new()));
}

pub(super) fn end_code(state: &mut MarkdownState<'_>) {
    if let Some((lang, text)) = state.code.take() {
        state.out.extend(code_rows(
            &text,
            &lang,
            state.width,
            &margin(state.prefix, state.quote, state.lists.len()),
            true,
            None,
        ));
    }
}

pub(super) fn start_image(state: &mut MarkdownState<'_>, dest_url: &str) {
    let block = state.block.get_or_insert_with(|| {
        let margin = margin(state.prefix, state.quote, state.lists.len());

        Block {
            pieces: Vec::new(),
            first: margin.clone(),
            rest: margin,
        }
    });

    block.pieces.push(Piece {
        text: "[image: ".into(),
        style: theme::muted(),
        link: Some(dest_url.to_string()),
        ..Default::default()
    });
    state.link = Some(dest_url.to_string());
    state.styles.push(theme::muted());
}

pub(super) fn end_image(state: &mut MarkdownState<'_>) {
    state.styles.pop();

    if let Some(block) = state.block.as_mut() {
        block.pieces.push(Piece {
            text: "]".into(),
            style: theme::muted(),
            link: state.link.clone(),
            ..Default::default()
        });
    }

    state.link = None;
}

pub(super) fn start_link(state: &mut MarkdownState<'_>, dest_url: &str, style: Style) {
    state.link = Some(dest_url.to_string());
    state
        .styles
        .push(style.fg(theme::pal().info()).add_modifier(Modifier::UNDERLINED));
}

pub(super) fn text_event(state: &mut MarkdownState<'_>, text: &str, style: Style) {
    if let Some((_, code)) = state.code.as_mut() {
        code.push_str(text);
        return;
    }

    let block = state.block.get_or_insert_with(|| {
        let margin = margin(state.prefix, state.quote, state.lists.len());

        Block {
            pieces: Vec::new(),
            first: margin.clone(),
            rest: margin,
        }
    });

    let st = if state.heading { theme::bold() } else { style };

    // a bare URL in text: clickable too
    for (i, part) in text.split(' ').enumerate() {
        if i > 0 {
            block.pieces.push(Piece {
                text: " ".into(),
                style: st,
                ..Default::default()
            });
        }

        // `(https://…)`, `<https://…>`, `"https://…"`: the URL without the marks around it
        let bare = part.find("https://").or_else(|| part.find("http://")).map(|i| {
            part[i..]
                .trim_end_matches(['.', ',', ')', ';', ':', '>', '"', '\'', ']'])
                .to_string()
        });

        let url = state.link.clone().or(bare);

        let st = if url.is_some() && state.link.is_none() {
            st.fg(theme::pal().info()).add_modifier(Modifier::UNDERLINED)
        } else {
            st
        };

        block.pieces.push(Piece {
            text: part.to_string(),
            style: st,
            link: url,
            ..Default::default()
        });
    }
}

pub(super) fn inline_code(state: &mut MarkdownState<'_>, code: &str) {
    let block = state.block.get_or_insert_with(|| {
        let margin = margin(state.prefix, state.quote, state.lists.len());

        Block {
            pieces: Vec::new(),
            first: margin.clone(),
            rest: margin,
        }
    });

    // a panel background: inline code never reads as a link (same blue, links underlined);
    // a path of an existing file: a file link (a click opens it in the files view)
    let file = state.link.is_none().then(|| file_link(code)).flatten();
    let style = Style::default().fg(code_fg()).bg(theme::pal().surface0);

    let style = if file.is_some() {
        style.add_modifier(Modifier::UNDERLINED)
    } else {
        style
    };

    let copy = file.is_none().then(|| code.to_string());

    block.pieces.push(Piece {
        text: code.to_string(),
        style,
        link: file.or_else(|| state.link.clone()),
        copy,
    });
}

pub(super) fn line_break(state: &mut MarkdownState<'_>) {
    if let Some(block) = state.block.as_mut() {
        let (first, rest) = (block.first.clone(), block.rest.clone());
        let pieces = std::mem::take(&mut block.pieces);

        state.out.extend(wrap(&pieces, state.width, &first, &rest));
        block.first = rest;
    }
}
