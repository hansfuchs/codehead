//! Fence languages from original Markdown; syntax colors without a background.
use std::sync::OnceLock;

#[cfg(test)]
use pulldown_cmark::{CodeBlockKind, Event, Parser, Tag, TagEnd};
use ratatui::style::Color;
use two_face::re_exports::syntect::{
    easy::HighlightLines,
    highlighting::{Theme, ThemeSet},
    parsing::SyntaxSet,
    util::LinesWithEndings,
};

#[derive(Clone, Debug, PartialEq)]
pub struct Source {
    pub language: String,
    pub text: String,
}

#[cfg(test)]
pub fn fences(markdown: &str) -> Vec<Source> {
    let mut out = Vec::new();
    let mut block = None;
    for event in Parser::new(markdown) {
        match event {
            Event::Start(Tag::CodeBlock(kind)) => {
                let language = match kind {
                    CodeBlockKind::Fenced(info) => info.split_whitespace().next().unwrap_or_default().to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                block = Some(Source { language, text: String::new() });
            }
            Event::Text(text) => if let Some(block) = &mut block { block.text.push_str(&text); },
            Event::End(TagEnd::CodeBlock) => if let Some(block) = block.take() { out.push(block); },
            _ => {}
        }
    }
    out
}


fn syntaxes() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(two_face::syntax::extra_newlines)
}

fn theme() -> &'static Theme {
    static THEME: OnceLock<Theme> = OnceLock::new();
    THEME.get_or_init(|| ThemeSet::load_defaults().themes.remove("base16-ocean.dark").expect("embedded theme"))
}

fn token(language: &str) -> &str {
    match language {
        "typescript" => "ts", "javascript" | "node" => "js", "python" => "py",
        "bash" | "shell" | "sh" | "zsh" | "console" => "sh",
        "markdown" => "md", "rust" => "rs", "yml" => "yaml",
        other => other,
    }
}

/// The fence tag takes priority. Content detection is a fallback during the source read.
pub fn language<'a>(text: &str, sources: &'a [Source]) -> &'a str {
    let normalized = |text: &str| text.lines().map(str::trim).collect::<Vec<_>>().join("\n");
    let prefix = normalized(text);
    if let Some(source) = sources.iter().find(|source| {
        let candidate = normalized(&source.text);
        !source.language.is_empty() && !candidate.is_empty()
            && (candidate.starts_with(&prefix) || prefix.starts_with(&candidate))
    }) {
        return &source.language;
    }
    let text = text.trim_start();
    if text.starts_with("#!") && text.lines().next().is_some_and(|line| line.contains("python")) { "py" }
    else if text.starts_with("def ") || text.starts_with("from ") || text.starts_with("import ") && !text.contains(";") { "py" }
    else if text.starts_with("fn ") || text.starts_with("pub ") || text.starts_with("use ") { "rs" }
    else if text.starts_with('{') || text.starts_with('[') { "json" }
    else if text.starts_with("const ") || text.starts_with("let ") || text.starts_with("function ") || text.starts_with("interface ") || text.starts_with("export ") { "ts" }
    else if text.starts_with("# ") || text.starts_with("## ") { "md" }
    else if text.starts_with("#!/") || ["printf ", "echo ", "$ ", "git ", "cargo ", "bun ", "npm ", "sudo ", "pacman "].iter().any(|prefix| text.starts_with(prefix)) { "sh" }
    else { "text" }
}

/// Last source line as colored characters. Replaying preceding lines preserves parser state
/// through multiline strings, comments, streamed continuations, and terminal redraws.
pub fn last_line(text: &str, language: &str) -> Vec<(Color, char)> {
    let syntaxes = syntaxes();
    let language = language.to_lowercase();
    let syntax = syntaxes.find_syntax_by_token(token(&language)).unwrap_or_else(|| syntaxes.find_syntax_plain_text());
    let mut highlighter = HighlightLines::new(syntax, theme());
    let mut out = Vec::new();
    for line in LinesWithEndings::from(text) {
        out.clear();
        match highlighter.highlight_line(line, syntaxes) {
            Ok(spans) => for (style, text) in spans {
                let fg = style.foreground;
                for c in text.chars().filter(|c| !matches!(c, '\n' | '\r')) {
                    if c == '\t' { out.extend(std::iter::repeat_n((Color::Rgb(fg.r, fg.g, fg.b), ' '), 4)); }
                    else if !c.is_control() { out.push((Color::Rgb(fg.r, fg.g, fg.b), c)); }
                }
            },
            Err(_) => out.extend(line.chars().filter(|c| !c.is_control()).map(|c| (crate::theme::pal().text, c))),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fence_tags_select_language_before_content_detection() {
        let sources = fences("```ts\n{ value: true }\n```\n```python\ndef greet():\n    return 'hello'\n```\n");
        assert_eq!(sources.len(), 2);
        assert_eq!(language("{ value: true }", &sources), "ts");
        assert_eq!(language("def greet():", &sources), "python");
        assert_eq!(language("echo hello", &[]), "sh");
    }

    #[test]
    fn requested_languages_get_token_colors() {
        for (language, text) in [
            ("ts", "const count: number = 42; // count"),
            ("python", "def greet(name): return 'hello'"),
            ("bash", "printf '%s' \"$HOME\" # home"),
            ("json", "{\"enabled\": true, \"count\": 100}"),
            ("md", "# title with **bold** and `code`"),
        ] {
            let chars = last_line(text, language);
            assert_eq!(chars.iter().map(|(_, c)| c).collect::<String>(), text);
            let colors: std::collections::HashSet<_> = chars.iter().filter(|(_, c)| !c.is_whitespace()).map(|(color, _)| *color).collect();
            assert!(colors.len() >= 2, "{language}: {colors:?}");
        }
    }
}
