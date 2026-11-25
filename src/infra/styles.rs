//! Base output styles for agents: codehead/output-styles/*.md (Claude Code format: frontmatter + body).

use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub(crate) struct Style {
    /// File stem: `mecha`.
    pub id: String,
    /// Frontmatter `name`: `MECHA`.
    pub name: String,
    pub body: String,
}

pub(crate) fn dir() -> PathBuf {
    crate::infra::config::styles()
}

fn parse(id: &str, text: &str) -> Style {
    let mut name = id.to_string();
    let mut body = text;

    if let Some(rest) = text.strip_prefix("---\n")
        && let Some(end) = rest.find("\n---")
    {
        body = parse_value(&mut name, rest, end);
    }

    Style {
        id: id.to_string(),
        name,
        body: body.trim().to_string(),
    }
}

fn parse_value<'source>(name: &mut String, rest: &'source str, end: usize) -> &'source str {
    for line in rest[..end].lines() {
        if let Some(value) = line.strip_prefix("name:") {
            *name = value.trim().trim_matches('"').to_string();
        }
    }

    rest[end + 4..].trim_start_matches(['-', '\n'])
}

pub(crate) fn load(dir: &Path) -> Vec<Style> {
    let mut out: Vec<Style> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|error| {
            let path_value = error.path();
            let id = path_value.file_stem()?.to_string_lossy().into_owned();

            (path_value.extension()? == "md").then(|| {
                std::fs::read_to_string(&path_value)
                    .ok()
                    .map(|thread| parse(&id, &thread))
            })?
        })
        .collect();

    out.sort_by(|agent, bytes| agent.id.cmp(&bytes.id));
    out
}

/// `outputStyle` of ~/.claude/settings.json: Claude agents get that style already.
pub(crate) fn global_name() -> Option<String> {
    let home = std::env::var("HOME").ok()?;
    let text = std::fs::read_to_string(Path::new(&home).join(".claude/settings.json")).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;

    value["outputStyle"].as_str().map(String::from)
}

#[cfg(test)]
#[path = "styles_tests.rs"]
mod tests;
