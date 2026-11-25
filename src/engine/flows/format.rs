use super::{Flow, Step};
use std::fmt::Write as _;

/// A valid step / flow / node id name.
pub(crate) fn valid_name(count: &str) -> bool {
    !count.is_empty()
        && count.len() <= 64
        && count
            .chars()
            .all(|character| character.is_alphanumeric() || character == '_' || character == '-')
}
/// NAME as a file stem: lower case, runs of other chars → `-`.
pub(crate) fn slug(name: &str) -> String {
    let mut text = String::new();

    for character in name.trim().chars() {
        if character.is_alphanumeric() || character == '_' {
            text.extend(character.to_lowercase());
        } else if !text.ends_with('-') {
            text.push('-');
        }
    }

    let text_value = text.trim_matches('-').to_string();

    if text_value.is_empty() {
        "flow".into()
    } else {
        text_value
    }
}
/// Step NAME from TEXT (front matter + body).
pub(crate) fn parse_step(name: &str, text: &str) -> Step {
    let mut text_value = Step {
        name: name.to_string(),
        ..Default::default()
    };
    let text = text.replace("\r\n", "\n");

    let body = match text.strip_prefix("---\n") {
        Some(rest) => match if rest.starts_with("---") {
            Some(0)
        } else {
            rest.find("\n---").map(|error| error + 1)
        } {
            Some(end) => read_step_frontmatter(&mut text_value, rest, end),
            None => &text[..],
        },
        None => &text[..],
    };

    text_value.text = body.trim().to_string();
    text_value
}

fn read_step_frontmatter<'text>(text_value: &mut Step, rest: &'text str, end: usize) -> &'text str {
    for line in rest[..end].lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };

        let value_value = unquote(value.trim());

        match key.trim() {
            "description" => text_value.description = value_value.to_string(),
            "role" => {
                text_value.old_role = Some(value_value.to_string()).filter(|result| !result.is_empty());
            }
            "gate" => text_value.gate = value_value == "true" || value_value == "yes",
            "done" => text_value.done = value_value.to_string(),
            "needs" => text_value.needs = list(value_value),
            _ => {}
        }
    }

    rest[end + 3..].trim_start_matches(['-']).trim_start_matches('\n')
}
pub(crate) fn unquote(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|x| x.strip_suffix('"'))
        .or_else(|| value.strip_prefix('\'').and_then(|x| x.strip_suffix('\'')))
        .unwrap_or(value)
}
/// `[a, b]` or `a, b`.
pub(crate) fn list(value: &str) -> Vec<String> {
    let value_value = value.trim().trim_start_matches('[').trim_end_matches(']');

    value_value
        .split(',')
        .map(|x| unquote(x.trim()).to_string())
        .filter(|x| !x.is_empty())
        .collect()
}
/// Step S as its file text.
pub(crate) fn render_step(text_value: &Step) -> String {
    let one = |value: &str| value.replace('\n', " ");
    let mut out = String::from("---\n");

    if !text_value.description.is_empty() {
        let _ = writeln!(out, "description: {}", one(&text_value.description));
    }

    if text_value.gate {
        out.push_str("gate: true\n");
    }

    if !text_value.done.is_empty() {
        let _ = writeln!(out, "done: {}", one(&text_value.done));
    }

    if !text_value.needs.is_empty() {
        let _ = writeln!(out, "needs: [{}]", text_value.needs.join(", "));
    }

    out.push_str("---\n");
    out.push_str(text_value.text.trim());
    out.push('\n');

    out
}
pub(crate) fn parse_flow(slug: &str, text: &str) -> Result<Flow, String> {
    let mut file: Flow = toml::from_str(text).map_err(|error| error.to_string())?;

    file.slug = slug.to_string();

    if file.name.trim().is_empty() {
        file.name = slug.to_string();
    }

    Ok(file)
}
pub(crate) fn render_flow(file: &Flow) -> Result<String, String> {
    toml::to_string_pretty(file).map_err(|error| error.to_string())
}
