//! Deduplicated background reads. Callers retain the last value during refresh.
use std::collections::HashMap;
use std::hash::Hash;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

pub(crate) struct Reads<K, V> {
    tx: Sender<(K, u64, V)>,
    rx: Receiver<(K, u64, V)>,
    pending: HashMap<K, u64>,
    updated: HashMap<K, Instant>,
    generation: u64,
}

impl<K, V> Default for Reads<K, V> {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();

        Self {
            tx,
            rx,
            pending: HashMap::new(),
            updated: HashMap::new(),
            generation: 0,
        }
    }
}

impl<K: Clone + Eq + Hash + Send + 'static, V: Send + 'static> Reads<K, V> {
    pub(crate) fn pending(&self) -> usize {
        self.pending.len()
    }

    /// A read of KEY is under way.
    pub(crate) fn waiting(&self, key: &K) -> bool {
        self.pending.contains_key(key)
    }

    pub(crate) fn request(&mut self, key: K, read: impl FnOnce() -> V + Send + 'static) {
        if self.pending.contains_key(&key)
            || self
                .updated
                .get(&key)
                .is_some_and(|updated_at| updated_at.elapsed() < Duration::from_secs(5))
        {
            return;
        }

        let generation = self.generation;

        self.pending.insert(key.clone(), generation);
        let tx = self.tx.clone();

        std::thread::spawn(move || {
            let _ = tx.send((key, generation, read()));

            crate::infra::bus::wake();
        });
    }

    /// KEY's last read forgotten: a new request of it runs at once (0 wait of 5 s).
    pub(crate) fn forget(&mut self, key: &K) {
        self.updated.remove(key);
    }

    pub(crate) fn invalidate(&mut self) {
        self.generation += 1;
        self.pending.clear();
        self.updated.clear();
    }

    pub(crate) fn poll(&mut self) -> Vec<(K, V)> {
        poll_reads(self)
    }
}

fn path(name: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default())
        .join(".cache/codehead/snapshots")
        .join(format!("{name}.json"))
}

pub(crate) fn snapshot<T: serde::de::DeserializeOwned>(name: &str) -> Option<T> {
    serde_json::from_slice(&std::fs::read(path(name)).ok()?).ok()
}

/// Atomic replacement; private permissions also protect transcript text.
pub(crate) fn save<T: serde::Serialize>(name: &str, value: &T) {
    static LAST: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, u64>>> =
        std::sync::OnceLock::new();

    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    use std::io::Write;
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    let path = path(name);
    let Some(dir) = path.parent() else { return };
    let _ = std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir);
    let Ok(bytes) = serde_json::to_vec(value) else { return };

    // the same value as the last write of this process: 0 disk write (pollers save every few s)

    let hash = {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();

        bytes.hash(&mut hasher);
        hasher.finish()
    };

    if let Ok(mut last) = LAST.get_or_init(Default::default).lock() {
        if last.get(name) == Some(&hash) && path.exists() {
            return;
        }

        last.insert(name.to_string(), hash);
    }

    let seq = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = path.with_extension(format!("{}-{seq}.tmp", std::process::id()));

    if let Ok(mut file) = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)
        && file.write_all(&bytes).is_ok()
    {
        let _ = std::fs::rename(tmp, path);
    }
}

pub(crate) fn scope(path: &std::path::Path) -> String {
    use std::hash::Hasher;
    let mut hash = std::collections::hash_map::DefaultHasher::new();

    path.hash(&mut hash);
    format!("{:x}", hash.finish())
}

#[cfg(test)]
#[path = "background_tests.rs"]
mod tests;

fn poll_reads<K: Clone + Eq + Hash + Send + 'static, V: Send + 'static>(reads: &mut Reads<K, V>) -> Vec<(K, V)> {
    let mut collected = Vec::new();

    while let Ok((key, generation, value)) = reads.rx.try_recv() {
        if reads.pending.get(&key) != Some(&generation) {
            continue;
        }

        reads.pending.remove(&key);
        reads.updated.insert(key.clone(), Instant::now());
        collected.push((key, value));
    }

    collected
}
