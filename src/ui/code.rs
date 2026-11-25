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
pub(crate) struct Source {
    pub language: String,
    pub text: String,
}

#[cfg(test)]
pub(crate) fn fences(markdown: &str) -> Vec<Source> {
    let mut out = Vec::new();
    let mut block = None;

    for event in Parser::new(markdown) {
        fences_language(&mut out, &mut block, event);
    }

    out
}

#[cfg(test)]
fn fences_language(out: &mut Vec<Source>, block: &mut Option<Source>, event: Event<'_>) {
    match event {
        Event::Start(Tag::CodeBlock(kind)) => {
            let language = match kind {
                CodeBlockKind::Fenced(info) => info.split_whitespace().next().unwrap_or_default().to_string(),
                CodeBlockKind::Indented => String::new(),
            };

            *block = Some(Source {
                language,
                text: String::new(),
            });
        }
        Event::Text(text) => {
            if let Some(block) = block {
                block.text.push_str(&text);
            }
        }
        Event::End(TagEnd::CodeBlock) => {
            if let Some(block) = block.take() {
                out.push(block);
            }
        }
        _ => {}
    }
}

fn syntaxes() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(two_face::syntax::extra_newlines)
}

fn theme() -> &'static Theme {
    static THEME: OnceLock<Theme> = OnceLock::new();

    THEME.get_or_init(|| {
        ThemeSet::load_defaults()
            .themes
            .remove("base16-ocean.dark")
            .expect("embedded theme")
    })
}

fn token(language: &str) -> &str {
    match language {
        "typescript" => "ts",
        "javascript" | "node" => "js",
        "python" => "py",
        "bash" | "shell" | "sh" | "zsh" | "console" => "sh",
        "markdown" => "md",
        "rust" => "rs",
        "yml" => "yaml",
        other => other,
    }
}

/// The fence tag takes priority. Content detection is a fallback during the source read.
pub(crate) fn language<'a>(text: &str, sources: &'a [Source]) -> &'a str {
    let normalized = |text: &str| text.lines().map(str::trim).collect::<Vec<_>>().join("\n");
    let prefix = normalized(text);

    if let Some(source) = sources.iter().find(|source| {
        let candidate = normalized(&source.text);

        !source.language.is_empty()
            && !candidate.is_empty()
            && (candidate.starts_with(&prefix) || prefix.starts_with(&candidate))
    }) {
        return &source.language;
    }

    let text = text.trim_start();

    if text.starts_with("#!") && text.lines().next().is_some_and(|line| line.contains("python"))
        || text.starts_with("def ")
        || text.starts_with("from ")
        || text.starts_with("import ") && !text.contains(';')
    {
        "py"
    } else if text.starts_with("fn ") || text.starts_with("pub ") || text.starts_with("use ") {
        "rs"
    } else if text.starts_with('{') || text.starts_with('[') {
        "json"
    } else if text.starts_with("const ")
        || text.starts_with("let ")
        || text.starts_with("function ")
        || text.starts_with("interface ")
        || text.starts_with("export ")
    {
        "ts"
    } else if text.starts_with("# ") || text.starts_with("## ") {
        "md"
    } else if text.starts_with("#!/")
        || [
            "printf ", "echo ", "$ ", "git ", "cargo ", "bun ", "npm ", "sudo ", "pacman ",
        ]
        .iter()
        .any(|prefix| text.starts_with(prefix))
    {
        "sh"
    } else {
        "text"
    }
}

/// Major languages: (hyperpolyglot / linguist name, fence tag).
const MAJOR: [(&str, &str); 31] = [
    ("Rust", "rs"),
    ("Python", "py"),
    ("JavaScript", "js"),
    ("TypeScript", "ts"),
    ("Go", "go"),
    ("Java", "java"),
    ("C", "c"),
    ("C++", "cpp"),
    ("C#", "cs"),
    ("Ruby", "rb"),
    ("PHP", "php"),
    ("Shell", "sh"),
    ("Kotlin", "kotlin"),
    ("Swift", "swift"),
    ("SQL", "sql"),
    ("HTML", "html"),
    ("CSS", "css"),
    ("JSON", "json"),
    ("YAML", "yaml"),
    ("TOML", "toml"),
    ("Lua", "lua"),
    ("Haskell", "haskell"),
    ("Scala", "scala"),
    ("Dockerfile", "dockerfile"),
    ("Makefile", "makefile"),
    ("Elixir", "elixir"),
    ("Dart", "dart"),
    ("R", "r"),
    ("Perl", "perl"),
    ("Objective-C", "objc"),
    ("PowerShell", "powershell"),
];

/// Fence tag for pasted TEXT (major languages, hyperpolyglot's classifier). None: 0 language matches.
pub(crate) fn guess(text: &str) -> Option<&'static str> {
    let tag = |name: &str| {
        MAJOR
            .iter()
            .find(|(language, _)| *language == name)
            .map(|(_, fence)| *fence)
    };

    if serde_json::from_str::<serde_json::Value>(text).is_ok_and(|value| value.is_object() || value.is_array()) {
        return Some("json");
    }

    if let Ok(names) = hyperpolyglot::detectors::get_languages_from_shebang(text.as_bytes())
        && let Some(fence) = names.iter().find_map(|language| tag(language))
    {
        return Some(fence);
    }

    // strong marks the classifier misses on short snippets
    let trimmed = text.trim_start();

    if trimmed.starts_with("<?php") {
        return Some("php");
    }

    if trimmed.starts_with("<!DOCTYPE") || trimmed.starts_with("<html") {
        return Some("html");
    }

    if trimmed.starts_with("package ") && text.contains("func ") {
        return Some("go");
    }

    let names: Vec<&'static str> = MAJOR.iter().map(|(language, _)| *language).collect();
    let words = text
        .split(|character: char| !character.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .count();

    if words == 0 {
        return None;
    }

    tag(hyperpolyglot::detectors::classify(text, &names))
}

/// Pasted TEXT is source code: 2+ lines, most lines with code marks, or a known start.
pub(crate) fn looks_code(text: &str) -> bool {
    let lines: Vec<&str> = text.lines().filter(|line| !line.trim().is_empty()).collect();

    if lines.len() < 2 || text.trim_start().starts_with("```") {
        return false;
    }

    if !matches!(language(text, &[]), "text" | "md") {
        return true;
    }

    let marked = lines
        .iter()
        .filter(|line| {
            let trimmed = line.trim_end();

            trimmed.ends_with([';', '{', '}', ')', ':', ']', ','])
                || trimmed.starts_with("    ")
                || trimmed.starts_with('\t')
                || trimmed.trim_start().starts_with("//")
                || trimmed.trim_start().starts_with("/*")
                || trimmed.contains(" = ")
                || trimmed.contains("=>")
                || trimmed.contains("->")
        })
        .count();

    marked * 2 > lines.len()
}

/// Colored characters of each line of TEXT.
pub(crate) type Lines = std::sync::Arc<Vec<Vec<(Color, char)>>>;

/// Blocks kept by `highlight`: a redraw renders each block again.
const CACHE_CAP: usize = 256;

/// Each source line as colored characters, in 1 pass: the parser state goes through
/// multiline strings and comments. Cached per (text, language): redraws and streamed
/// continuations of other blocks cost 0 highlight.
pub(crate) fn highlight(text: &str, language: &str) -> Lines {
    use std::collections::HashMap;
    use std::hash::{Hash, Hasher};
    use std::sync::Mutex;
    static CACHE: OnceLock<Mutex<HashMap<(u64, String), Lines>>> = OnceLock::new();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();

    text.hash(&mut hasher);
    let key = (hasher.finish(), language.to_string());
    let cache = CACHE.get_or_init(Default::default);

    if let Some(cached) = cache.lock().ok().and_then(|map| map.get(&key).cloned()) {
        return cached;
    }

    let lines = Lines::new(highlight_all(text, language));

    if let Ok(mut map) = cache.lock() {
        if map.len() >= CACHE_CAP {
            map.clear();
        }

        map.insert(key, lines.clone());
    }

    lines
}

fn highlight_all(text: &str, language: &str) -> Vec<Vec<(Color, char)>> {
    let syntaxes = syntaxes();
    let language = language.to_lowercase();
    let syntax = syntaxes
        .find_syntax_by_token(token(&language))
        .unwrap_or_else(|| syntaxes.find_syntax_plain_text());
    let mut highlighter = HighlightLines::new(syntax, theme());
    let mut all = Vec::new();

    for line in LinesWithEndings::from(text) {
        highlight_all_out(syntaxes, &mut highlighter, &mut all, line);
    }

    all
}

fn highlight_all_out(
    syntaxes: &SyntaxSet,
    highlighter: &mut HighlightLines<'_>,
    all: &mut Vec<Vec<(Color, char)>>,
    line: &str,
) {
    let mut out = Vec::new();

    match highlighter.highlight_line(line, syntaxes) {
        Ok(spans) => {
            append_highlighted_spans(&mut out, spans);
        }
        Err(_) => out.extend(
            line.chars()
                .filter(|character| !character.is_control())
                .map(|character| (crate::ui::theme::pal().text, character)),
        ),
    }

    all.push(out);
}

fn append_highlighted_spans(
    out: &mut Vec<(Color, char)>,
    spans: Vec<(two_face::re_exports::syntect::highlighting::Style, &str)>,
) {
    for (style, text) in spans {
        let fg = style.foreground;

        for character in text.chars().filter(|character| !matches!(character, '\n' | '\r')) {
            append_highlight_char(out, Color::Rgb(fg.r, fg.g, fg.b), character);
        }
    }
}

/// Last source line as colored characters (the lines before set the parser state).
#[cfg(test)]
pub(crate) fn last_line(text: &str, language: &str) -> Vec<(Color, char)> {
    highlight_all(text, language).pop().unwrap_or_default()
}

#[cfg(test)]
#[path = "code_tests.rs"]
mod tests;

fn append_highlight_char(out: &mut Vec<(Color, char)>, color: Color, character: char) {
    if character == '\t' {
        out.extend(std::iter::repeat_n((color, ' '), 4));
    } else if !character.is_control() {
        out.push((color, character));
    }
}
