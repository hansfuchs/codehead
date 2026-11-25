//! Paste like Claude Code, shared by the task prompt and the thread view: a clipboard image (ctrl+v)
//! or a dropped image path → `[Image #n]`, text over 1024 chars (code: never) → `[Pasted text #n +L lines]`.
//! Both are files in a paste dir; the caller attaches them (compose: the path, a new task: a list).

use std::path::{Path, PathBuf};

use crate::infra::clip;

/// Pasted text longer than this (chars) goes in as a file attachment plus a marker.
pub(crate) const PASTE_INLINE_MAX: usize = 1024;
const IMAGE_EXT: [&str; 6] = ["png", "jpg", "jpeg", "gif", "webp", "heic"];

/// Paste of 1 dropped file path: `'/a b.png'`, `/a\ b.png`, `file:///a.png`. None: not 1 existing file.
pub(crate) fn dropped_file(text: &str) -> Option<PathBuf> {
    let thread = text.trim();

    if thread.contains('\n') {
        return None;
    }

    let thread_value = thread.strip_prefix("file://").unwrap_or(thread);
    let thread_2 = thread_value
        .strip_prefix('\'')
        .and_then(|x| x.strip_suffix('\''))
        .or_else(|| thread_value.strip_prefix('"').and_then(|x| x.strip_suffix('"')))
        .map_or_else(|| thread_value.replace("\\ ", " "), String::from);
    let path = PathBuf::from(thread_2);

    (path.is_absolute() && path.is_file()).then_some(path)
}

pub(crate) fn is_image(path: &Path) -> bool {
    path.extension()
        .is_some_and(|error| IMAGE_EXT.contains(&error.to_string_lossy().to_lowercase().as_str()))
}

/// Marker for a long paste, like Claude Code: `[Pasted text #1 +12 lines]`.
pub(crate) fn text_marker(count_value: usize, text: &str) -> String {
    format!("[Pasted text #{count_value} +{} lines]", text.lines().count())
}

/// What a paste turns into.
#[derive(Debug, PartialEq)]
pub(crate) enum Pasted {
    /// Goes into the input as is.
    Text(String),
    /// A file to attach; MARKER goes into the input.
    File { path: PathBuf, marker: String },
    /// Empty paste, 0 clipboard image, 0 clipboard text.
    Nothing,
}

/// Counters + the dir of the pasted files.
pub(crate) struct Paster {
    pub dir: PathBuf,
    pub images: usize,
    pub texts: usize,
}

impl Paster {
    /// The counters go on after the files already in DIR (`image-N.png`, `pasted-text-N.txt`):
    /// a new paste never overwrites the file of an earlier message.
    pub(crate) fn new(dir: PathBuf) -> Paster {
        let mut path = Paster {
            dir,
            images: 0,
            texts: 0,
        };

        for error in std::fs::read_dir(&path.dir).into_iter().flatten().flatten() {
            let name = error.file_name();
            let name = name.to_string_lossy();
            let count = |pre: &str, ext: &str| name.strip_prefix(pre)?.strip_suffix(ext)?.parse::<usize>().ok();

            restore_paste_counters(&mut path, count);
        }

        path
    }

    /// MARKER (`[Image #3]`, `[Pasted text #2 +5 lines]`) is in use: new pastes number after it.
    pub(crate) fn seen(&mut self, marker: &str) {
        let count = |pre: &str| {
            marker
                .strip_prefix(pre)?
                .split([']', ' '])
                .next()?
                .parse::<usize>()
                .ok()
        };

        if let Some(count_value) = count("[Image #") {
            self.images = self.images.max(count_value);
        } else if let Some(count_2) = count("[Pasted text #") {
            self.texts = self.texts.max(count_2);
        }
    }

    /// A paste event's text. Empty (cmd+v on an image in some terminals): the clipboard image.
    pub(crate) fn paste(&mut self, text: &str) -> Pasted {
        paste_paster(self, text)
    }

    /// ctrl+v (Claude Code's image key): the clipboard image, else the clipboard text.
    pub(crate) fn clipboard(&mut self) -> Pasted {
        let file = self.dir.join(format!("image-{}.png", self.images + 1));

        if clip::paste_image(&file) {
            self.images += 1;
            return Pasted::File {
                path: file,
                marker: format!("[Image #{}]", self.images),
            };
        }

        match clip::paste() {
            Some(thread) if !thread.is_empty() => self.paste(&thread),
            _ => Pasted::Nothing,
        }
    }
}

fn restore_paste_counters(paster: &mut Paster, count: impl Fn(&str, &str) -> Option<usize>) {
    if let Some(count_value) = count("image-", ".png") {
        paster.images = paster.images.max(count_value);
    } else {
        restore_text_counter(paster, count);
    }
}

fn restore_text_counter(paster: &mut Paster, count: impl Fn(&str, &str) -> Option<usize>) {
    if let Some(count_2) = count("pasted-text-", ".txt") {
        paster.texts = paster.texts.max(count_2);
    }
}

#[cfg(test)]
#[path = "prompt_input_attach_tests.rs"]
mod tests;

fn paste_paster(paster: &mut Paster, text: &str) -> Pasted {
    if text.is_empty() {
        return paster.clipboard();
    }

    if let Some(file) = dropped_file(text) {
        if is_image(&file) {
            paster.images += 1;
            return Pasted::File {
                path: file,
                marker: format!("[Image #{}]", paster.images),
            };
        }

        return Pasted::Text(file.display().to_string());
    }

    if text.chars().count() <= PASTE_INLINE_MAX || crate::ui::code::looks_code(text) {
        return Pasted::Text(text.to_string());
    }

    // terminals paste line breaks as \r: count and store them as \n
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let file_value = paster.dir.join(format!("pasted-text-{}.txt", paster.texts + 1));
    let _ = std::fs::create_dir_all(&paster.dir);

    if std::fs::write(&file_value, &text).is_err() {
        return Pasted::Text(text);
    }

    paster.texts += 1;

    Pasted::File {
        marker: text_marker(paster.texts, &text),
        path: file_value,
    }
}
