use super::{Path, Value, cut};

/// The tail of PATH (MAX bytes, a cut 1st line left out), parsed lines.
pub(crate) fn tail_lines(path: &Path, max: u64) -> Vec<Value> {
    use std::io::{Read, Seek, SeekFrom};

    let Ok(mut file) = std::fs::File::open(path) else {
        return Vec::new();
    };

    let len = file.metadata().map_or(0, |metadata| metadata.len());
    let from = len.saturating_sub(max);
    let _ = file.seek(SeekFrom::Start(from));
    let mut buf = Vec::new();
    let _ = file.read_to_end(&mut buf);
    let text = String::from_utf8_lossy(&buf);

    let body = if from > 0 {
        text.split_once('\n').map_or("", |(_, rest)| rest)
    } else {
        &text
    };

    body.lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}
/// The report of a subagent: its `SubagentHandback` message, else its last text. From TRANSCRIPT.
pub(crate) fn result_of(transcript: &Path) -> Option<String> {
    for value in tail_lines(transcript, 512 << 10).iter().rev() {
        if value["type"] != "assistant" {
            continue;
        }

        let blocks = value["message"]["content"].as_array().cloned().unwrap_or_default();

        if let Some(message) = blocks
            .iter()
            .find(|block| block["type"] == "tool_use" && block["name"] == "SubagentHandback")
            .and_then(|block| block["input"]["message"].as_str())
        {
            return Some(cut(message, 16 << 10));
        }

        let text: Vec<&str> = blocks
            .iter()
            .filter(|block| block["type"] == "text")
            .filter_map(|block| block["text"].as_str())
            .filter(|part| !part.trim().is_empty())
            .collect();

        if !text.is_empty() {
            return Some(cut(&text.join("\n\n"), 16 << 10));
        }
    }

    None
}
/// What the subagent does now, from the end of its TRANSCRIPT: its last tool call (`Edit src/a.rs`)
/// or the 1st line of its last text.
pub(crate) fn activity_of(transcript: &Path) -> Option<String> {
    tail_lines(transcript, 64 << 10)
        .iter()
        .rev()
        .filter(|value| value["type"] == "assistant")
        .find_map(message_activity)
}

fn message_activity(value: &Value) -> Option<String> {
    value["message"]["content"]
        .as_array()
        .into_iter()
        .flatten()
        .rev()
        .find_map(block_activity)
}

fn block_activity(block: &Value) -> Option<String> {
    match block["type"].as_str() {
        Some("tool_use") => Some(tool_activity(block)),
        Some("text") => text_activity(block),
        _ => None,
    }
}

fn tool_activity(block: &Value) -> String {
    crate::domain::handoff::tool_line(block["name"].as_str().unwrap_or_default(), &block["input"])
}

fn text_activity(block: &Value) -> Option<String> {
    if let Some(first) = block["text"]
        .as_str()
        .and_then(|text| text.lines().find(|line| !line.trim().is_empty()))
    {
        return Some(first.trim().chars().take(160).collect());
    }

    None
}
/// The 1st user message of TRANSCRIPT: the subagent's task.
pub(crate) fn prompt_of(transcript: &Path) -> String {
    use std::io::{BufRead, BufReader};

    let Ok(file) = std::fs::File::open(transcript) else {
        return String::new();
    };

    for line in BufReader::new(file).lines().take(20).map_while(Result::ok) {
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };

        if let Some(value) = user_prompt(&value) {
            return value;
        }
    }

    String::new()
}

fn user_prompt(value: &Value) -> Option<String> {
    if value["type"] == "user" {
        let content = &value["message"]["content"];

        let text = content.as_str().map_or_else(
            || {
                content
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|block| block["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("\n")
            },
            String::from,
        );

        return Some(cut(&text, 2048));
    }

    None
}
pub(crate) fn mtime_ms(path: &Path) -> u64 {
    std::fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |since_epoch| since_epoch.as_millis() as u64)
}
