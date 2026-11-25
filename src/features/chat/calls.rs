use super::{Call, Row, Span, call_summary, markdown, theme};
use std::fmt::Write as FormatWrite;

pub(super) fn tool_label(name: &str) -> String {
    match name.strip_prefix("mcp__") {
        Some(rest) => rest.replace("__", ":"),
        None => name.to_string(),
    }
}
/// The run row: ` ◦ Bash: cargo test · Read: x.rs and 3 more · 5 calls · 12.0s`.
pub(super) fn run_summary(calls: &[Call], secs: f64) -> String {
    let labels: Vec<String> = calls
        .iter()
        .take(2)
        .map(|character| {
            let text = call_summary(&character.name, &character.input);

            if text.is_empty() {
                tool_label(&character.name)
            } else {
                format!("{}: {text}", tool_label(&character.name))
            }
        })
        .collect();

    let mut thread = labels.join(" · ");

    if calls.len() > 2 {
        write!(thread, " and {} more", calls.len() - 2).unwrap();
    }

    let count = calls.len();

    format!(
        "{thread} · {count} call{} · {secs:.1}s",
        if count == 1 { "" } else { "s" }
    )
}
/// Most lines of a code block in an open run (the rest: `… n more lines`); of an output: the last ones.
pub(super) const CALL_CODE_LINES: usize = 40;
pub(super) const CALL_OUTPUT_LINES: usize = 12;
/// Language of FILE for the colors: its extension (`src/app.rs` → `rs`), else from the text.
pub(super) fn file_lang(file: &str) -> String {
    std::path::Path::new(file)
        .extension()
        .and_then(|error| error.to_str())
        .unwrap_or_default()
        .to_string()
}
/// Max. `CALL_CODE_LINES` of TEXT; the rest as a count.
pub(super) fn head_lines(text: &str) -> (String, usize) {
    let lines: Vec<&str> = text.lines().collect();
    let more = lines.len().saturating_sub(CALL_CODE_LINES);

    (lines[..lines.len() - more].join("\n"), more)
}
/// `-` OLD lines, `+` NEW lines: an edit as a diff.
pub(super) fn diff(old: &str, new: &str) -> String {
    let mut out: Vec<String> = old.lines().map(|line| format!("-{line}")).collect();

    out.extend(new.lines().map(|line_value| format!("+{line_value}")));
    out.join("\n")
}
/// The open rows of CALL below its head row, MARGIN before each: its code (the command of a Bash
/// call, the text of a Write, the diff of an Edit) and its output, as code blocks with colors.
pub(super) fn call_rows(call: &Call, width: usize, margin: &[Span<'static>]) -> Vec<Row> {
    let mut out = Vec::new();
    let note = |count: usize, what: &str| markdown::plain(&format!("… {count} {what}"), width, theme::muted(), margin);

    let code = |out: &mut Vec<Row>, text: &str, lang: &str| {
        let (text, more) = head_lines(text);

        if !text.trim().is_empty() {
            out.extend(markdown::code_rows(&text, lang, width, margin, true, None));
        }

        if more > 0 {
            out.extend(note(more, "more lines"));
        }
    };

    let text_value = |key: &str| call.input[key].as_str();

    match call.name.as_str() {
        "Bash" => {
            if let Some(cmd) = text_value("command") {
                code(&mut out, cmd, "sh");
            }
        }
        "Write" => {
            if let (Some(file), Some(text)) = (text_value("file_path"), text_value("content")) {
                code(&mut out, text, &file_lang(file));
            }
        }
        "Edit" => {
            if let (Some(old), Some(new)) = (text_value("old_string"), text_value("new_string")) {
                code(&mut out, &diff(old, new), "diff");
            }
        }
        "MultiEdit" => {
            call_rows_multiedit(call, &mut out, code);
        }
        _ => {}
    }

    if let Some((ok, text)) = &call.output {
        call_rows_lines(width, margin, &mut out, note, *ok, text);
    }

    out
}

fn call_rows_lines(
    width: usize,
    margin: &[Span<'static>],
    out: &mut Vec<Row>,
    note: impl Fn(usize, &str) -> Vec<Row>,
    ok: bool,
    text: &str,
) {
    let lines: Vec<&str> = text.lines().collect();
    let tail = lines.len().saturating_sub(CALL_OUTPUT_LINES);
    let text = lines[tail..].join("\n");

    if tail > 0 {
        out.extend(note(tail, "lines"));
    }

    if !text.trim().is_empty() {
        // output: plain text (json: colors); an error in the danger color
        let lang = if crate::ui::code::language(&text, &[]) == "json" {
            "json"
        } else {
            "text"
        };

        let fg = if ok { None } else { Some(theme::pal().error()) };

        out.extend(markdown::code_rows(&text, lang, width, margin, true, fg));
    }
}

fn call_rows_multiedit(call: &Call, out: &mut Vec<Row>, code: impl Fn(&mut Vec<Row>, &str, &str)) {
    let edits = call.input["edits"].as_array().cloned().unwrap_or_default();

    let all: Vec<String> = edits
        .iter()
        .map(|error_value| {
            diff(
                error_value["old_string"].as_str().unwrap_or_default(),
                error_value["new_string"].as_str().unwrap_or_default(),
            )
        })
        .collect();

    if !all.is_empty() {
        code(out, &all.join("\n"), "diff");
    }
}
/// Short label of 1 call for a bubble's run row: `run tests`, `read view.rs`, `search foo`.
pub(super) fn call_label(character: &Call) -> String {
    let text = |key: &str| character.input[key].as_str().map(String::from);
    let base = |path: String| path.rsplit('/').next().unwrap_or_default().to_string();

    let lower = |thread: String| {
        let mut ch = thread.chars();

        ch.next()
            .map_or_else(String::new, |file| file.to_lowercase().chain(ch).collect())
    };

    let value = match character.name.as_str() {
        "Bash" => text("description")
            .map(lower)
            .or_else(|| text("command").map(|character_value| format!("run {character_value}"))),
        "Read" => text("file_path").map(|path_value| format!("read {}", base(path_value))),
        "Write" => text("file_path").map(|path_2| format!("write {}", base(path_2))),
        "Edit" | "MultiEdit" => text("file_path").map(|path_3| format!("edit {}", base(path_3))),
        "NotebookEdit" => text("notebook_path").map(|path_4| format!("edit {}", base(path_4))),
        "Grep" | "Glob" => text("pattern").map(|path_5| format!("search {path_5}")),
        "WebFetch" => text("url").map(|user| format!("fetch {user}")),
        "WebSearch" => text("query").map(|query| format!("search the web for {query}")),
        "Task" | "Agent" => text("description").map(lower),
        "TodoWrite" => Some("update todos".into()),
        "ToolSearch" => text("query").map(|query_value| {
            format!(
                "load {}",
                query_value
                    .trim_start_matches("select:")
                    .split(',')
                    .map(|thread_value| thread_value.rsplit("__").next().unwrap_or(thread_value))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }),
        count if count.starts_with("mcp__") => {
            let mut parts = count.trim_start_matches("mcp__").splitn(2, "__");
            let (server, tool) = (parts.next().unwrap_or_default(), parts.next().unwrap_or_default());

            Some(format!("{} ({server})", tool.replace('_', " ")))
        }
        _ => None,
    };

    let value_value = value.unwrap_or_else(|| tool_label(&character.name));

    value_value
        .lines()
        .next()
        .unwrap_or_default()
        .chars()
        .take(80)
        .collect()
}
/// The bubble's run rows: `Load skills_list, run tests, read x.rs and 5 more`, `8 calls · 20.7s`.
pub(super) fn run_lines(calls: &[Call], secs: f64) -> (String, String) {
    let labels: Vec<String> = calls.iter().take(3).map(call_label).collect();
    let mut thread = labels.join(", ");

    if calls.len() > 3 {
        write!(thread, " and {} more", calls.len() - 3).unwrap();
    }

    let mut ch = thread.chars();
    let thread_value = ch
        .next()
        .map_or_else(String::new, |file| file.to_uppercase().chain(ch).collect());
    let count = calls.len();

    (
        thread_value,
        format!("{count} call{} · {secs:.1}s", if count == 1 { "" } else { "s" }),
    )
}
