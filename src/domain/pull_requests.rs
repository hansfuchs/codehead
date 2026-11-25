use crate::infra::gh::PrRef;
use serde_json::{Value, json};
fn dir() -> std::path::PathBuf {
    crate::engine::data_dir().join("prs")
}
/// The thread THREAD has PRS (the merge tab found them): the index of the lists.
pub(crate) fn remember(thread: &str, prs: &[PrRef]) {
    if cfg!(test) {
        return;
    }

    let path = dir().join("threads.json");

    let _ = crate::engine::locked(&dir().join("threads.lock"), || {
        let mut value: Value = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_else(|| json!({}));

        if !value.is_object() {
            value = json!({});
        }

        value[thread] = json!(prs.iter().map(PrRef::key).collect::<Vec<_>>());
        let _ = std::fs::create_dir_all(dir());

        crate::engine::write_atomic(&path, value.to_string().as_bytes())
    });
}

/// The PRs the merge tab of THREAD found last (`remember`).
pub(crate) fn remembered(thread: &str) -> Vec<PrRef> {
    let value: Value = std::fs::read(dir().join("threads.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();

    value[thread]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter_map(|key| {
            let (repo, number) = key.rsplit_once('/')?;

            Some(PrRef::new(repo, number.parse().ok()?))
        })
        .collect()
}
