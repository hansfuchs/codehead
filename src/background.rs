//! Deduplicated background reads. Callers retain the last value during refresh.
use std::collections::HashMap;
use std::hash::Hash;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

pub struct Reads<K, V> {
    tx: Sender<(K, u64, V)>,
    rx: Receiver<(K, u64, V)>,
    pending: HashMap<K, u64>,
    updated: HashMap<K, Instant>,
    generation: u64,
}

impl<K, V> Default for Reads<K, V> {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self { tx, rx, pending: HashMap::new(), updated: HashMap::new(), generation: 0 }
    }
}

impl<K: Clone + Eq + Hash + Send + 'static, V: Send + 'static> Reads<K, V> {
    pub fn pending(&self) -> usize { self.pending.len() }

    /// A read of KEY is under way.
    pub fn waiting(&self, key: &K) -> bool { self.pending.contains_key(key) }

    pub fn request(&mut self, key: K, read: impl FnOnce() -> V + Send + 'static) {
        if self.pending.contains_key(&key) || self.updated.get(&key).is_some_and(|t| t.elapsed() < Duration::from_secs(5)) {
            return;
        }
        let generation = self.generation;
        self.pending.insert(key.clone(), generation);
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send((key, generation, read()));
            crate::bus::wake();
        });
    }


    /// KEY's last read forgotten: a new request of it runs at once (0 wait of 5 s).
    pub fn forget(&mut self, key: &K) {
        self.updated.remove(key);
    }

    pub fn invalidate(&mut self) {
        self.generation += 1;
        self.pending.clear();
        self.updated.clear();
    }

    pub fn poll(&mut self) -> Vec<(K, V)> {
        let mut ready = Vec::new();
        while let Ok((key, generation, value)) = self.rx.try_recv() {
            if self.pending.get(&key) != Some(&generation) {
                continue;
            }
            self.pending.remove(&key);
            self.updated.insert(key.clone(), Instant::now());
            ready.push((key, value));
        }
        ready
    }
}

fn path(name: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".cache/codehead/snapshots").join(format!("{name}.json"))
}

pub fn snapshot<T: serde::de::DeserializeOwned>(name: &str) -> Option<T> {
    serde_json::from_slice(&std::fs::read(path(name)).ok()?).ok()
}

/// Atomic replacement; private permissions also protect transcript text.
pub fn save<T: serde::Serialize>(name: &str, value: &T) {
    use std::io::Write;
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    let path = path(name);
    let Some(dir) = path.parent() else { return };
    let _ = std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir);
    let Ok(bytes) = serde_json::to_vec(value) else { return };
    // the same value as the last write of this process: 0 disk write (pollers save every few s)
    static LAST: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, u64>>> = std::sync::OnceLock::new();
    let hash = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        bytes.hash(&mut h);
        h.finish()
    };
    if let Ok(mut last) = LAST.get_or_init(Default::default).lock() {
        if last.get(name) == Some(&hash) && path.exists() {
            return;
        }
        last.insert(name.to_string(), hash);
    }
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = path.with_extension(format!("{}-{seq}.tmp", std::process::id()));
    if let Ok(mut f) = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp)
        && f.write_all(&bytes).is_ok()
    {
        let _ = std::fs::rename(tmp, path);
    }
}

pub fn scope(path: &std::path::Path) -> String {
    use std::hash::Hasher;
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut hash);
    format!("{:x}", hash.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_return_before_completion_and_deduplicate() {
        let mut reads = Reads::default();
        let (release, wait) = mpsc::channel();
        reads.request("rows", move || wait.recv().unwrap());
        // A blocked worker cannot block a tab change or start a duplicate read.
        reads.request("rows", || panic!("duplicate read"));
        assert!(reads.poll().is_empty());
        release.send(42).unwrap();
        let (_, _, value) = reads.rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(value, 42);
    }

    #[test]
    fn invalidation_discards_results_from_before_a_write() {
        let mut reads = Reads::default();
        reads.request("rows", || 1);
        let stale = reads.rx.recv_timeout(Duration::from_secs(2)).unwrap();
        reads.invalidate();
        reads.request("rows", || 2);
        let fresh = reads.rx.recv_timeout(Duration::from_secs(2)).unwrap();
        reads.tx.send(stale).unwrap();
        reads.tx.send(fresh).unwrap();
        assert_eq!(reads.poll(), vec![("rows", 2)]);
        reads.request("rows", || panic!("fresh value read again"));
        assert!(reads.pending.is_empty());
    }
}
