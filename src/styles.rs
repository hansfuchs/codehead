//! Base output styles for agents: codehead/output-styles/*.md (Claude Code format: frontmatter + body).

use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Style {
    /// File stem: `mecha`.
    pub id: String,
    /// Frontmatter `name`: `MECHA`.
    pub name: String,
    pub body: String,
}

pub fn dir() -> PathBuf {
    crate::config::styles()
}

fn parse(id: &str, text: &str) -> Style {
    let mut name = id.to_string();
    let mut body = text;
    if let Some(rest) = text.strip_prefix("---\n")
        && let Some(end) = rest.find("\n---")
    {
        for line in rest[..end].lines() {
            if let Some(v) = line.strip_prefix("name:") {
                name = v.trim().trim_matches('"').to_string();
            }
        }
        body = rest[end + 4..].trim_start_matches(['-', '\n']);
    }
    Style { id: id.to_string(), name, body: body.trim().to_string() }
}

pub fn load(dir: &Path) -> Vec<Style> {
    let mut out: Vec<Style> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            let id = p.file_stem()?.to_string_lossy().into_owned();
            (p.extension()? == "md").then(|| std::fs::read_to_string(&p).ok().map(|t| parse(&id, &t)))?
        })
        .collect();
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

/// `outputStyle` of ~/.claude/settings.json: Claude agents get that style already.
pub fn global_name() -> Option<String> {
    let home = std::env::var("HOME").ok()?;
    let text = std::fs::read_to_string(Path::new(&home).join(".claude/settings.json")).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    v["outputStyle"].as_str().map(String::from)
}

#[cfg(test)]
mod tests {
    #[test]
    fn frontmatter_split() {
        let s = super::parse("x", "---\nname: MECHA\ndescription: d\n---\n\nbody line\n");
        assert_eq!(s.name, "MECHA");
        assert_eq!(s.body, "body line");
    }
}
