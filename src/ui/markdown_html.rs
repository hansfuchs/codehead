use super::{Block, Piece, Style};

/// HTML elements hidden in rendered Markdown (their text stays), and custom elements (a `-` in
/// the name: `<relative-time>`). Other `<…>` (`<Foo>`, `<T>`): plain text.
pub(super) const HTML_TAGS: &[&str] = &[
    "a",
    "abbr",
    "b",
    "big",
    "blockquote",
    "br",
    "center",
    "cite",
    "code",
    "dd",
    "del",
    "details",
    "div",
    "dl",
    "dt",
    "em",
    "figcaption",
    "figure",
    "font",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "hr",
    "i",
    "img",
    "ins",
    "kbd",
    "li",
    "mark",
    "ol",
    "p",
    "picture",
    "pre",
    "q",
    "s",
    "samp",
    "small",
    "source",
    "span",
    "strong",
    "sub",
    "summary",
    "sup",
    "table",
    "tbody",
    "td",
    "tfoot",
    "th",
    "thead",
    "tr",
    "tt",
    "u",
    "ul",
    "var",
    "video",
];

/// A part of raw HTML.
pub(super) enum HtmlPart<'a> {
    /// A known tag: its lowercase name, closing, its source.
    Tag(String, bool, &'a str),
    Text(&'a str),
}

/// S split into known tags and text; comments dropped.
pub(super) fn html_parts(text: &str) -> Vec<HtmlPart<'_>> {
    let mut out = Vec::new();
    let (mut i, mut from) = (0, 0);

    while let Some(lt) = text[i..].find('<').map(|path| i + path) {
        html_parts_end(text, &mut out, &mut i, &mut from, lt);
    }

    if from < text.len() {
        out.push(HtmlPart::Text(&text[from..]));
    }

    out
}

fn html_parts_end<'source>(
    text: &'source str,
    out: &mut Vec<HtmlPart<'source>>,
    i: &mut usize,
    from: &mut usize,
    lt: usize,
) {
    if text[lt..].starts_with("<!--") {
        if *from < lt {
            out.push(HtmlPart::Text(&text[*from..lt]));
        }

        let end = text[lt..].find("-->").map_or(text.len(), |error| lt + error + 3);

        (*i, *from) = (end, end);
        return;
    }

    let close = text[lt + 1..].starts_with('/');
    let n0 = lt + 1 + usize::from(close);
    let n1 = text[n0..]
        .find(|character: char| !(character.is_ascii_alphanumeric() || character == '-'))
        .map_or(text.len(), |error_value| n0 + error_value);
    let name = &text[n0..n1];
    let lower = name.to_ascii_lowercase();
    let known = name.starts_with(|character_value: char| character_value.is_ascii_alphabetic())
        && (name == lower || name == name.to_ascii_uppercase())
        && (HTML_TAGS.contains(&lower.as_str()) || lower.contains('-'));
    let end = text[n1..]
        .find(['>', '<'])
        .filter(|&error_2| text.as_bytes()[n1 + error_2] == b'>')
        .map(|error_3| n1 + error_3 + 1);

    // `<IMG>` too, 0 `<Foo>` (a component in prose)

    match end {
        Some(end) if known => {
            if *from < lt {
                out.push(HtmlPart::Text(&text[*from..lt]));
            }

            out.push(HtmlPart::Tag(lower, close, &text[lt..end]));
            (*i, *from) = (end, end);
        }
        _ => *i = lt + 1,
    }
}

/// The value of attribute NAME of TAG (`<a href="…">`), entities decoded.
pub(super) fn html_attr(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let mut from = 0;

    while let Some(at) = lower[from..].find(name).map(|path| from + path) {
        from = at + name.len();
        let after = lower[from..].trim_start();

        if !lower[..at].ends_with(char::is_whitespace) || !after.starts_with('=') {
            continue;
        }

        // ASCII lowercase keeps the byte offsets: the value from TAG
        let value = tag[tag.len() - after.len() + 1..].trim_start();

        let (body, end) = match value.chars().next() {
            Some(query @ ('"' | '\'')) => (&value[1..], value[1..].find(query)),
            _ => (
                value,
                value.find(|character: char| character.is_whitespace() || character == '>'),
            ),
        };

        return Some(entities(&body[..end.unwrap_or(body.len())]));
    }

    None
}

/// S with its HTML entities decoded (`&amp;` `&lt;` `&#39;` `&#x2014;` …).
pub(super) fn entities(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;

    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let semi = rest
            .char_indices()
            .take(12)
            .find(|&(_, character)| character == ';')
            .map(|(error, _)| error);
        let character_value = semi.and_then(|error_value| match &rest[1..error_value] {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            count => count
                .strip_prefix('#')
                .and_then(|directory| match directory.strip_prefix(['x', 'X']) {
                    Some(height) => u32::from_str_radix(height, 16).ok(),
                    None => directory.parse().ok(),
                })
                .and_then(char::from_u32),
        });

        if let (Some(character_2), Some(error_2)) = (character_value, semi) {
            out.push(character_2);
            rest = &rest[error_2 + 1..];
        } else {
            out.push('&');
            rest = &rest[1..];
        }
    }

    out.push_str(rest);
    out
}

/// TEXT of HTML into B: whitespace collapsed (as a browser does), in STYLE, a link on LINK.
pub(super) fn html_words(bytes: &mut Block, text: &str, style: Style, link: Option<&str>) {
    let space = |bytes_value: &mut Block| {
        if bytes_value.pieces.last().is_some_and(|path| path.text != " ") {
            bytes_value.pieces.push(Piece {
                text: " ".into(),
                style,
                ..Default::default()
            });
        }
    };

    if text.starts_with(char::is_whitespace) {
        space(bytes);
    }

    for (i, width) in text.split_whitespace().enumerate() {
        if i > 0 {
            space(bytes);
        }

        bytes.pieces.push(Piece {
            text: width.to_string(),
            style,
            link: link.map(str::to_owned),
            ..Default::default()
        });
    }

    if text.ends_with(char::is_whitespace) && !text.trim().is_empty() {
        space(bytes);
    }
}
