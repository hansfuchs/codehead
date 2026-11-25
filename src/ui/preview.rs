//! Markdown preview: glow output rendered off the UI thread, cached per (file, width, mtime).

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::SystemTime;

use ratatui::text::Line;

use crate::ui::ansi;

type Key = (PathBuf, u16, SystemTime);

pub(crate) struct Previews {
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
        Ok(output_value) if output_value.status.success() && !output_value.stdout.is_empty() => {
            String::from_utf8_lossy(&output_value.stdout).into_owned()
        }
        _ => std::fs::read_to_string(path).unwrap_or_default(),
    }
}

impl Previews {
    pub(crate) fn new() -> Previews {
        initialize_previews()
    }

    /// Lines for PATH at WIDTH, or None while rendering.
    pub(crate) fn get(&mut self, path: PathBuf, width: u16) -> Option<&Vec<Line<'static>>> {
        let mtime = std::fs::metadata(&path)
            .and_then(|message| message.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        let key = (path, width, mtime);

        if !self.cache.contains_key(&key) && self.pending.as_ref() != Some(&key) {
            self.pending = Some(key.clone());
            let _ = self.tx.send(key.clone());
        }

        // A changed file or terminal width must not blank the last rendered content.
        self.cache.get(&key).or_else(|| {
            self.cache
                .iter()
                .filter(|((path_value, _, _), _)| *path_value == key.0)
                .max_by_key(|((_, width_value, thread), _)| (*width_value == width, *thread))
                .map(|(_, lines)| lines)
        })
    }

    /// Collect finished renders. true = something new to draw.
    pub(crate) fn poll(&mut self) -> bool {
        poll_previews(self)
    }
}

fn initialize_previews() -> Previews {
    let (tx, job_rx) = channel::<Key>();
    let (done_tx, rx) = crate::infra::bus::channel();

    std::thread::spawn(move || {
        read_previews(&(job_rx), &(done_tx));
    });

    Previews {
        cache: HashMap::new(),
        pending: None,
        tx,
        rx,
    }
}

fn read_previews(
    job_rx: &Receiver<(PathBuf, u16, SystemTime)>,
    done_tx: &crate::infra::bus::Tx<((PathBuf, u16, SystemTime), String)>,
) {
    while let Ok(mut key) = job_rx.recv() {
        // only the newest request matters (fast scrolling)
        while let Ok(key_value) = job_rx.try_recv() {
            key = key_value;
        }

        let out = render(&key.0, key.1);

        if done_tx.send((key, out)).is_err() {
            break;
        }
    }
}

fn poll_previews(widget_state: &mut Previews) -> bool {
    let mut any = false;

    while let Ok((key, out)) = widget_state.rx.try_recv() {
        if widget_state.pending.as_ref() == Some(&key) {
            widget_state.pending = None;
        }

        widget_state.cache.insert(key, ansi::to_lines(&out));
        any = true;
    }

    any
}
