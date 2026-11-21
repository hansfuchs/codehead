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

/// Major languages: (hyperpolyglot / linguist name, fence tag).
const MAJOR: [(&str, &str); 31] = [
    ("Rust", "rs"), ("Python", "py"), ("JavaScript", "js"), ("TypeScript", "ts"), ("Go", "go"),
    ("Java", "java"), ("C", "c"), ("C++", "cpp"), ("C#", "cs"), ("Ruby", "rb"), ("PHP", "php"),
    ("Shell", "sh"), ("Kotlin", "kotlin"), ("Swift", "swift"), ("SQL", "sql"), ("HTML", "html"),
    ("CSS", "css"), ("JSON", "json"), ("YAML", "yaml"), ("TOML", "toml"), ("Lua", "lua"),
    ("Haskell", "haskell"), ("Scala", "scala"), ("Dockerfile", "dockerfile"), ("Makefile", "makefile"),
    ("Elixir", "elixir"), ("Dart", "dart"), ("R", "r"), ("Perl", "perl"), ("Objective-C", "objc"),
    ("PowerShell", "powershell"),
];

/// Fence tag for pasted TEXT (major languages, hyperpolyglot's classifier). None: 0 language matches.
pub fn guess(text: &str) -> Option<&'static str> {
    let tag = |name: &str| MAJOR.iter().find(|(n, _)| *n == name).map(|(_, t)| *t);
    if serde_json::from_str::<serde_json::Value>(text).is_ok_and(|v| v.is_object() || v.is_array()) {
        return Some("json");
    }
    if let Ok(names) = hyperpolyglot::detectors::get_languages_from_shebang(text.as_bytes())
        && let Some(t) = names.iter().find_map(|n| tag(n))
    {
        return Some(t);
    }
    // strong marks the classifier misses on short snippets
    let t = text.trim_start();
    if t.starts_with("<?php") { return Some("php"); }
    if t.starts_with("<!DOCTYPE") || t.starts_with("<html") { return Some("html"); }
    if t.starts_with("package ") && text.contains("func ") { return Some("go"); }
    let names: Vec<&'static str> = MAJOR.iter().map(|(n, _)| *n).collect();
    let words = text.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).count();
    if words == 0 {
        return None;
    }
    tag(hyperpolyglot::detectors::classify(text, &names))
}

/// Pasted TEXT is source code: 2+ lines, most lines with code marks, or a known start.
pub fn looks_code(text: &str) -> bool {
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines.len() < 2 || text.trim_start().starts_with("```") {
        return false;
    }
    if !matches!(language(text, &[]), "text" | "md") {
        return true;
    }
    let marked = lines.iter().filter(|l| {
        let t = l.trim_end();
        t.ends_with([';', '{', '}', ')', ':', ']', ','])
            || t.starts_with("    ") || t.starts_with('\t')
            || t.trim_start().starts_with("//") || t.trim_start().starts_with("/*")
            || t.contains(" = ") || t.contains("=>") || t.contains("->")
    }).count();
    marked * 2 > lines.len()
}

/// Colored characters of each line of TEXT.
pub type Lines = std::sync::Arc<Vec<Vec<(Color, char)>>>;

/// Blocks kept by `highlight`: a redraw renders each block again.
const CACHE_CAP: usize = 256;

/// Each source line as colored characters, in 1 pass: the parser state goes through
/// multiline strings and comments. Cached per (text, language): redraws and streamed
/// continuations of other blocks cost 0 highlight.
pub fn highlight(text: &str, language: &str) -> Lines {
    use std::collections::HashMap;
    use std::hash::{Hash, Hasher};
    use std::sync::Mutex;
    static CACHE: OnceLock<Mutex<HashMap<(u64, String), Lines>>> = OnceLock::new();
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    let key = (h.finish(), language.to_string());
    let cache = CACHE.get_or_init(Default::default);
    if let Some(l) = cache.lock().ok().and_then(|c| c.get(&key).cloned()) {
        return l;
    }
    let lines = Lines::new(highlight_all(text, language));
    if let Ok(mut c) = cache.lock() {
        if c.len() >= CACHE_CAP {
            c.clear();
        }
        c.insert(key, lines.clone());
    }
    lines
}

fn highlight_all(text: &str, language: &str) -> Vec<Vec<(Color, char)>> {
    let syntaxes = syntaxes();
    let language = language.to_lowercase();
    let syntax = syntaxes.find_syntax_by_token(token(&language)).unwrap_or_else(|| syntaxes.find_syntax_plain_text());
    let mut highlighter = HighlightLines::new(syntax, theme());
    let mut all = Vec::new();
    for line in LinesWithEndings::from(text) {
        let mut out = Vec::new();
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
        all.push(out);
    }
    all
}

/// Last source line as colored characters (the lines before set the parser state).
#[cfg(test)]
pub fn last_line(text: &str, language: &str) -> Vec<(Color, char)> {
    highlight_all(text, language).pop().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paste_guess_finds_major_languages() {
        for (src, tag) in [
            ("fn main() {\n    let x = vec![1];\n    println!(\"{x:?}\");\n}", "rs"),
            ("def run(a):\n    return [x for x in a if x]\n", "py"),
            ("package main\n\nimport \"fmt\"\n\nfunc main() {\n\tfmt.Println(\"hi\")\n}", "go"),
            ("#include <stdio.h>\nint main(void) {\n  printf(\"hi\\n\");\n  return 0;\n}", "c"),
            ("public class A {\n  public static void main(String[] args) {\n    System.out.println(1);\n  }\n}", "java"),
            ("SELECT id, name\nFROM users\nWHERE id = 1;", "sql"),
            ("#!/bin/bash\necho hi", "sh"),
            ("{\"a\": [1, 2]}", "json"),
        ] {
            assert_eq!(guess(src), Some(tag), "{src}");
        }
        assert_eq!(guess(";;;\n{}"), None);
    }

    #[test]
    fn fence_tags_select_language_before_content_detection() {
        let sources = fences("```ts\n{ value: true }\n```\n```python\ndef greet():\n    return 'hello'\n```\n");
        assert_eq!(sources.len(), 2);
        assert_eq!(language("{ value: true }", &sources), "ts");
        assert_eq!(language("def greet():", &sources), "python");
        assert_eq!(language("echo hello", &[]), "sh");
    }

    #[test]
    fn one_pass_matches_the_line_by_line_replay() {
        let text = "/* a\n   comment */\nconst s = `multi\nline`;\nlet n = 1;";
        let all = highlight(text, "ts");
        let lines: Vec<&str> = text.split('\n').collect();
        assert_eq!(all.len(), lines.len());
        for i in 0..lines.len() {
            assert_eq!(all[i], last_line(&lines[..=i].join("\n"), "ts"), "line {i}");
        }
        // cached: the same Arc
        assert!(std::sync::Arc::ptr_eq(&all, &highlight(text, "ts")));
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
