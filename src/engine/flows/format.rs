use super::{Flow, Step};
use std::fmt::Write as _;

/// A valid step / flow / node id name.
pub(crate) fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
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

    let slug = text.trim_matches('-').to_string();

    if slug.is_empty() { "flow".into() } else { slug }
}
/// Step NAME from TEXT (front matter + body).
pub(crate) fn parse_step(name: &str, text: &str) -> Step {
    let mut step = Step {
        name: name.to_string(),
        ..Default::default()
    };
    let text = text.replace("\r\n", "\n");

    let body = match text.strip_prefix("---\n") {
        Some(rest) => match if rest.starts_with("---") {
            Some(0)
        } else {
            rest.find("\n---").map(|at| at + 1)
        } {
            Some(end) => read_step_frontmatter(&mut step, rest, end),
            None => &text[..],
        },
        None => &text[..],
    };

    step.text = body.trim().to_string();
    step
}

fn read_step_frontmatter<'text>(step: &mut Step, rest: &'text str, end: usize) -> &'text str {
    for line in rest[..end].lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };

        let value = unquote(value.trim());

        match key.trim() {
            "description" => step.description = value.to_string(),
            "role" => {
                step.old_role = Some(value.to_string()).filter(|role| !role.is_empty());
            }
            "gate" => step.gate = value == "true" || value == "yes",
            "done" => step.done = value.to_string(),
            "needs" => step.needs = list(value),
            _ => {}
        }
    }

    rest[end + 3..].trim_start_matches(['-']).trim_start_matches('\n')
}
pub(crate) fn unquote(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|inner| inner.strip_suffix('"'))
        .or_else(|| value.strip_prefix('\'').and_then(|inner| inner.strip_suffix('\'')))
        .unwrap_or(value)
}
/// `[a, b]` or `a, b`.
pub(crate) fn list(value: &str) -> Vec<String> {
    let items = value.trim().trim_start_matches('[').trim_end_matches(']');

    items
        .split(',')
        .map(|item| unquote(item.trim()).to_string())
        .filter(|item| !item.is_empty())
        .collect()
}
/// STEP as its file text.
pub(crate) fn render_step(step: &Step) -> String {
    let one = |value: &str| value.replace('\n', " ");
    let mut out = String::from("---\n");

    if !step.description.is_empty() {
        let _ = writeln!(out, "description: {}", one(&step.description));
    }

    if step.gate {
        out.push_str("gate: true\n");
    }

    if !step.done.is_empty() {
        let _ = writeln!(out, "done: {}", one(&step.done));
    }

    if !step.needs.is_empty() {
        let _ = writeln!(out, "needs: [{}]", step.needs.join(", "));
    }

    out.push_str("---\n");
    out.push_str(step.text.trim());
    out.push('\n');

    out
}
pub(crate) fn parse_flow(slug: &str, text: &str) -> Result<Flow, String> {
    let mut flow: Flow = toml::from_str(text).map_err(|error| error.to_string())?;

    flow.slug = slug.to_string();

    if flow.name.trim().is_empty() {
        flow.name = slug.to_string();
    }

    Ok(flow)
}
pub(crate) fn render_flow(flow: &Flow) -> Result<String, String> {
    toml::to_string_pretty(flow).map_err(|error| error.to_string())
}
