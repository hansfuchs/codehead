//! Drafts of the task screen: 1 JSON file per unsent task in ~/.cache/codehead/drafts ($CODEHEAD_DRAFTS),
//! written after every key that changed it. A started task (or an emptied one) drops its file.
//! The task screen lists the other drafts ("view drafts (n)"): load 1, or delete it.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

/// Drafts older than this go at start.
const KEEP_DAYS: u64 = 30;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Draft {
    pub id: String,
    /// Seconds since the epoch of the last write.
    pub saved: u64,
    /// Repo (else dir) name the task was written in.
    pub place: String,
    pub title: String,
    pub prompt: String,
    /// Picked notes (relative to the notes root).
    pub notes: Vec<String>,
    /// Picked agents.
    pub picks: Vec<String>,
    /// The where row's choice (its debug text), checkout / stash boxes.
    pub place_choice: String,
    pub checkout: bool,
    pub stash: bool,
    /// Pasted files: marker, path.
    pub files: Vec<(String, String)>,
    /// Context: thread id (None: a Claude Code session), transcript path, title.
    pub carry: Vec<(Option<String>, String, String)>,
}

pub fn dir() -> PathBuf {
    std::env::var_os("CODEHEAD_DRAFTS").filter(|s| !s.is_empty()).map(PathBuf::from).unwrap_or_else(|| crate::config::home().join(".cache/codehead/drafts"))
}

/// Id of a new draft: start time (sorts by age) + process + a count (2 tasks of 1 process).
pub fn new_id() -> String {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    format!("{}-{}-{}", (crate::engine::now_ms() / 1000), std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default()
}

impl Draft {
    /// Nothing a person wrote: 0 file for it.
    pub fn is_empty(&self) -> bool {
        self.title.trim().is_empty() && self.prompt.trim().is_empty()
    }

    /// Name in the list: the title, else the 1st line of the prompt.
    pub fn name(&self) -> String {
        let t = self.title.trim();
        let name = if t.is_empty() { self.prompt.lines().find(|l| !l.trim().is_empty()).unwrap_or_default().trim() } else { t };
        name.to_string()
    }

    /// The content (0 id, 0 time): 2 drafts with the same text need 0 new write.
    pub fn content(&self) -> String {
        json!({
            "place": self.place,
            "title": self.title,
            "prompt": self.prompt,
            "notes": self.notes,
            "picks": self.picks,
            "where": self.place_choice,
            "checkout": self.checkout,
            "stash": self.stash,
            "files": self.files.iter().map(|(m, p)| json!([m, p])).collect::<Vec<_>>(),
            "carry": self.carry.iter().map(|(t, p, title)| json!({"thread": t, "path": p, "title": title})).collect::<Vec<_>>(),
        })
        .to_string()
    }

    fn parse(id: &str, text: &str) -> Option<Draft> {
        let v: Value = serde_json::from_str(text).ok()?;
        let s = |k: &str| v[k].as_str().unwrap_or_default().to_string();
        let pair = |x: &Value| Some((x[0].as_str()?.to_string(), x[1].as_str()?.to_string()));
        Some(Draft {
            id: id.to_string(),
            saved: v["saved"].as_u64().unwrap_or(0),
            place: s("place"),
            title: s("title"),
            prompt: s("prompt"),
            notes: strings(&v["notes"]),
            picks: strings(&v["picks"]),
            place_choice: s("where"),
            checkout: v["checkout"].as_bool().unwrap_or(true),
            stash: v["stash"].as_bool().unwrap_or(false),
            files: v["files"].as_array().map(|a| a.iter().filter_map(pair).collect()).unwrap_or_default(),
            carry: v["carry"]
                .as_array()
                .map(|a| a.iter().map(|c| (c["thread"].as_str().map(String::from), c["path"].as_str().unwrap_or_default().to_string(), c["title"].as_str().unwrap_or_default().to_string())).collect())
                .unwrap_or_default(),
        })
    }
}

fn file(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}.json"))
}

/// Write D (a temp file, then a rename: a crash mid-write keeps the last full draft).
pub fn save(dir: &Path, d: &Draft) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let mut v: Value = serde_json::from_str(&d.content()).map_err(|e| e.to_string())?;
    v["saved"] = json!((crate::engine::now_ms() / 1000));
    let tmp = dir.join(format!(".{}.tmp", d.id));
    std::fs::write(&tmp, serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, file(dir, &d.id)).map_err(|e| e.to_string())
}

pub fn remove(dir: &Path, id: &str) {
    let _ = std::fs::remove_file(file(dir, id));
}

pub fn path(dir: &Path, id: &str) -> PathBuf {
    file(dir, id)
}

/// Every draft but SKIP (the open task's own), oldest first. Drafts over KEEP_DAYS old: removed.
pub fn load(dir: &Path, skip: &str) -> Vec<Draft> {
    let now = crate::engine::now_ms() / 1000;
    let mut v = Vec::new();
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.extension().is_none_or(|x| x != "json") {
            continue;
        }
        let Some(id) = p.file_stem().map(|s| s.to_string_lossy().into_owned()) else { continue };
        let Some(d) = std::fs::read_to_string(&p).ok().and_then(|t| Draft::parse(&id, &t)) else { continue };
        if d.saved > 0 && now.saturating_sub(d.saved) > KEEP_DAYS * 86400 {
            let _ = std::fs::remove_file(&p);
            continue;
        }
        if id != skip && !d.is_empty() {
            v.push(d);
        }
    }
    v.sort_by(|a, b| (a.saved, &a.id).cmp(&(b.saved, &b.id)));
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_load_remove() {
        let dir = std::env::temp_dir().join(format!("codehead-drafts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let d = Draft {
            id: "a".into(),
            title: "fix the build".into(),
            prompt: "- step 1\n  - nested".into(),
            notes: vec!["x/y.md".into()],
            picks: vec!["amon".into()],
            place_choice: "NewWorktree".into(),
            checkout: true,
            files: vec![("[Image #1]".into(), "/tmp/i.png".into())],
            carry: vec![(Some("ab12".into()), "/t.jsonl".into(), "old thread".into())],
            ..Default::default()
        };
        save(&dir, &d).unwrap();
        save(&dir, &Draft { id: "b".into(), prompt: "2nd".into(), ..Default::default() }).unwrap();
        // an empty draft: never listed
        save(&dir, &Draft { id: "c".into(), ..Default::default() }).unwrap();
        let all = load(&dir, "b");
        assert_eq!(all.len(), 1);
        assert!(all[0].saved > 0);
        assert_eq!(Draft { saved: 0, ..all[0].clone() }, d);
        assert_eq!(all[0].name(), "fix the build");
        assert_eq!(load(&dir, "")[..].iter().map(|d| d.name()).collect::<Vec<_>>().len(), 2);
        remove(&dir, "a");
        assert!(load(&dir, "b").is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
