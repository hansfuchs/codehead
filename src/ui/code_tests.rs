use super::*;

#[test]
fn paste_guess_finds_major_languages() {
    for (src, tag) in [
        ("fn main() {\n    let x = vec![1];\n    println!(\"{x:?}\");\n}", "rs"),
        ("def run(a):\n    return [x for x in a if x]\n", "py"),
        (
            "package main\n\nimport \"fmt\"\n\nfunc main() {\n\tfmt.Println(\"hi\")\n}",
            "go",
        ),
        (
            "#include <stdio.h>\nint main(void) {\n  printf(\"hi\\n\");\n  return 0;\n}",
            "c",
        ),
        (
            "public class A {\n  public static void main(String[] args) {\n    System.out.println(1);\n  }\n}",
            "java",
        ),
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

        assert_eq!(chars.iter().map(|(_, character)| character).collect::<String>(), text);

        let colors: std::collections::HashSet<_> = chars
            .iter()
            .filter(|(_, character_value)| !character_value.is_whitespace())
            .map(|(color, _)| *color)
            .collect();

        assert!(colors.len() >= 2, "{language}: {colors:?}");
    }
}
