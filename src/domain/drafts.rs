use crate::engine::store;
use serde_json::{Value, json};
use std::path::PathBuf;
/// The unsent prompt of thread ID (JSON: text, attachments).
pub(crate) fn prompt_path(id: &str) -> PathBuf {
    store::thread_dir(id).join("prompt.json")
}

/// TEXT as the prompt draft of thread ID: its next view opens with it.
pub(crate) fn save_prompt_draft(id: &str, text: &str) {
    save_prompt_draft_files(id, text, &[]);
}

/// The unsent prompt of thread ID: TEXT with FILES attached (marker, path), as a paste leaves it.
pub(crate) fn save_prompt_draft_files(id: &str, text: &str, files: &[(String, PathBuf)]) {
    let files: Vec<Value> = files
        .iter()
        .map(|(message, path)| json!([message, path.display().to_string()]))
        .collect();
    let _ = crate::engine::write_atomic(
        &prompt_path(id),
        json!({"text": text, "attachments": files}).to_string().as_bytes(),
    );
}
