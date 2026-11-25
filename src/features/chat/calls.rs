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
        .map(|call| {
            let text = call_summary(&call.name, &call.input);

            if text.is_empty() {
                tool_label(&call.name)
            } else {
                format!("{}: {text}", tool_label(&call.name))
            }
        })
        .collect();

    let mut head = labels.join(" · ");

    if calls.len() > 2 {
        write!(head, " and {} more", calls.len() - 2).unwrap();
    }

    let count = calls.len();

    format!(
        "{head} · {count} call{} · {secs:.1}s",
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
        .and_then(|extension| extension.to_str())
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

    out.extend(new.lines().map(|line| format!("+{line}")));
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

    let input = |key: &str| call.input[key].as_str();

    match call.name.as_str() {
        "Bash" => {
            if let Some(cmd) = input("command") {
                code(&mut out, cmd, "sh");
            }
        }
        "Write" => {
            if let (Some(file), Some(text)) = (input("file_path"), input("content")) {
                code(&mut out, text, &file_lang(file));
            }
        }
        "Edit" => {
            if let (Some(old), Some(new)) = (input("old_string"), input("new_string")) {
                code(&mut out, &diff(old, new), "diff");
            }
        }
        "MultiEdit" => {
            multi_edit_rows(call, &mut out, code);
        }
        _ => {}
    }

    if let Some((ok, text)) = &call.output {
        output_rows(width, margin, &mut out, note, *ok, text);
    }

    out
}

fn output_rows(
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

fn multi_edit_rows(call: &Call, out: &mut Vec<Row>, code: impl Fn(&mut Vec<Row>, &str, &str)) {
    let edits = call.input["edits"].as_array().cloned().unwrap_or_default();

    let all: Vec<String> = edits
        .iter()
        .map(|edit| {
            diff(
                edit["old_string"].as_str().unwrap_or_default(),
                edit["new_string"].as_str().unwrap_or_default(),
            )
        })
        .collect();

    if !all.is_empty() {
        code(out, &all.join("\n"), "diff");
    }
}
/// Short label of 1 call for a bubble's run row: `run tests`, `read view.rs`, `search foo`.
pub(super) fn call_label(call: &Call) -> String {
    let text = |key: &str| call.input[key].as_str().map(String::from);
    let base = |path: String| path.rsplit('/').next().unwrap_or_default().to_string();

    let lower = |text: String| {
        let mut chars = text.chars();

        chars
            .next()
            .map_or_else(String::new, |first| first.to_lowercase().chain(chars).collect())
    };

    let label = match call.name.as_str() {
        "Bash" => text("description")
            .map(lower)
            .or_else(|| text("command").map(|command| format!("run {command}"))),
        "Read" => text("file_path").map(|path| format!("read {}", base(path))),
        "Write" => text("file_path").map(|path| format!("write {}", base(path))),
        "Edit" | "MultiEdit" => text("file_path").map(|path| format!("edit {}", base(path))),
        "NotebookEdit" => text("notebook_path").map(|path| format!("edit {}", base(path))),
        "Grep" | "Glob" => text("pattern").map(|pattern| format!("search {pattern}")),
        "WebFetch" => text("url").map(|url| format!("fetch {url}")),
        "WebSearch" => text("query").map(|query| format!("search the web for {query}")),
        "Task" | "Agent" => text("description").map(lower),
        "TodoWrite" => Some("update todos".into()),
        "ToolSearch" => text("query").map(|query| {
            format!(
                "load {}",
                query
                    .trim_start_matches("select:")
                    .split(',')
                    .map(|tool| tool.rsplit("__").next().unwrap_or(tool))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }),
        name if name.starts_with("mcp__") => {
            let mut parts = name.trim_start_matches("mcp__").splitn(2, "__");
            let (server, tool) = (parts.next().unwrap_or_default(), parts.next().unwrap_or_default());

            Some(format!("{} ({server})", tool.replace('_', " ")))
        }
        _ => None,
    };

    let label = label.unwrap_or_else(|| tool_label(&call.name));

    label.lines().next().unwrap_or_default().chars().take(80).collect()
}
/// The bubble's run rows: `Load skills_list, run tests, read x.rs and 5 more`, `8 calls · 20.7s`.
pub(super) fn run_lines(calls: &[Call], secs: f64) -> (String, String) {
    let labels: Vec<String> = calls.iter().take(3).map(call_label).collect();
    let mut joined = labels.join(", ");

    if calls.len() > 3 {
        write!(joined, " and {} more", calls.len() - 3).unwrap();
    }

    let mut chars = joined.chars();
    let head = chars
        .next()
        .map_or_else(String::new, |first| first.to_uppercase().chain(chars).collect());
    let count = calls.len();

    (
        head,
        format!("{count} call{} · {secs:.1}s", if count == 1 { "" } else { "s" }),
    )
}
