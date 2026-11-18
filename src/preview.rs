//! Markdown preview: glow output rendered off the UI thread, cached per (file, width, mtime).

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::SystemTime;

use ratatui::text::Line;

use crate::ansi;

type Key = (PathBuf, u16, SystemTime);

pub struct Previews {
    cache: HashMap<Key, Vec<Line<'static>>>,
    pending: Option<Key>,
    tx: Sender<Key>,
    rx: Receiver<(Key, String)>,
}

fn render(path: &PathBuf, width: u16) -> String {
    let glow = Command::new("glow")
        .args(["-s", "dark", "-w", &width.to_string()])
        .arg(path)
        .env("CLICOLOR_FORCE", "1")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output();
    match glow {
        Ok(o) if o.status.success() && !o.stdout.is_empty() => String::from_utf8_lossy(&o.stdout).into_owned(),
        _ => std::fs::read_to_string(path).unwrap_or_default(),
    }
}

impl Previews {
    pub fn new() -> Previews {
        let (tx, job_rx) = channel::<Key>();
        let (done_tx, rx) = channel();
        std::thread::spawn(move || {
            while let Ok(mut key) = job_rx.recv() {
                // only the newest request matters (fast scrolling)
                while let Ok(k) = job_rx.try_recv() {
                    key = k;
                }
                let out = render(&key.0, key.1);
                if done_tx.send((key, out)).is_err() {
                    break;
                }
            }
        });
        Previews { cache: HashMap::new(), pending: None, tx, rx }
    }

    /// Lines for PATH at WIDTH, or None while rendering.
    pub fn get(&mut self, path: PathBuf, width: u16) -> Option<&Vec<Line<'static>>> {
        let mtime = std::fs::metadata(&path).and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH);
        let key = (path, width, mtime);
        if !self.cache.contains_key(&key) && self.pending.as_ref() != Some(&key) {
            self.pending = Some(key.clone());
            let _ = self.tx.send(key.clone());
        }
        // A changed file or terminal width must not blank the last rendered content.
        self.cache.get(&key).or_else(|| {
            self.cache.iter().filter(|((p, _, _), _)| *p == key.0).max_by_key(|((_, w, t), _)| (*w == width, *t)).map(|(_, lines)| lines)
        })
    }

    /// Collect finished renders. true = something new to draw.
    pub fn poll(&mut self) -> bool {
        let mut any = false;
        while let Ok((key, out)) = self.rx.try_recv() {
            if self.pending.as_ref() == Some(&key) {
                self.pending = None;
            }
            self.cache.insert(key, ansi::to_lines(&out));
            any = true;
        }
        any
    }
}
