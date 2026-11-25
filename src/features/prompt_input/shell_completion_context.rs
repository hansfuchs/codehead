use super::{Ctx, Pos, TextArea};

/// The context of the cursor in TA: shell mode, 1 line, 0 selection, the cursor at a word's end.
pub(super) fn context(ta: &TextArea) -> Option<Ctx> {
    let text = ta.text();

    if text.contains('\n') || ta.selected_text().is_some() || ta.cursor().0 != 0 {
        return None;
    }

    context_of(&text, ta.cursor().1)
}

/// [`context`] of TEXT (1 line), the cursor at COL.
pub(super) fn context_of(text: &str, col: usize) -> Option<Ctx> {
    let chars: Vec<char> = text.chars().collect();

    if !text.starts_with('!') || col == 0 || chars.get(col).is_some_and(|character| !character.is_whitespace()) {
        return None;
    }

    let mut start = col;

    while start > 1 && !chars[start - 1].is_whitespace() {
        start -= 1;
    }

    let word: String = chars[start..col].iter().collect();

    if word.contains('/') || word.contains('"') || word.contains('\'') {
        return None;
    }

    let line: String = chars[1..col].iter().collect::<String>().trim_start().to_string();
    let head = &line[..line.len() - word.len()];
    let mut before: Vec<String> = Vec::new();

    for width in head.split_whitespace() {
        if matches!(width, "|" | "||" | "&&" | ";") {
            before.clear();
        } else {
            before.push(width.to_string());
        }
    }

    Some(Ctx {
        at: Pos { row: 0, col: start },
        line,
        word,
        before,
    })
}

/// The runner whose tasks go after BEFORE (the words before the cursor's word), None: 0 runner.
pub(super) fn runner_of(before: &[String]) -> Option<&'static str> {
    let width: Vec<&str> = before.iter().map(String::as_str).collect();

    match width.as_slice() {
        ["just", ..] => Some("just"),
        ["make", ..] => Some("make"),
        ["npm", "run" | "run-script"] | ["pnpm" | "yarn"] | ["pnpm" | "yarn" | "bun", "run"] => Some("package"),
        _ => None,
    }
}
