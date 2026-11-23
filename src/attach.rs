//! Paste like Claude Code, shared by the task prompt and the thread view: a clipboard image (ctrl+v)
//! or a dropped image path → `[Image #n]`, text over 1024 chars (code: never) → `[Pasted text #n +L lines]`.
//! Both are files in a paste dir; the caller attaches them (compose: the path, a new task: a list).

use std::path::{Path, PathBuf};

use crate::clip;

/// Pasted text longer than this (chars) goes in as a file attachment plus a marker.
pub const PASTE_INLINE_MAX: usize = 1024;
const IMAGE_EXT: [&str; 6] = ["png", "jpg", "jpeg", "gif", "webp", "heic"];

/// Paste of 1 dropped file path: `'/a b.png'`, `/a\ b.png`, `file:///a.png`. None: not 1 existing file.
pub fn dropped_file(text: &str) -> Option<PathBuf> {
    let t = text.trim();
    if t.contains('\n') {
        return None;
    }
    let t = t.strip_prefix("file://").unwrap_or(t);
    let t = t.strip_prefix('\'').and_then(|x| x.strip_suffix('\'')).or_else(|| t.strip_prefix('"').and_then(|x| x.strip_suffix('"'))).map(String::from).unwrap_or_else(|| t.replace("\\ ", " "));
    let p = PathBuf::from(t);
    (p.is_absolute() && p.is_file()).then_some(p)
}

pub fn is_image(p: &Path) -> bool {
    p.extension().is_some_and(|e| IMAGE_EXT.contains(&e.to_string_lossy().to_lowercase().as_str()))
}

/// Marker for a long paste, like Claude Code: `[Pasted text #1 +12 lines]`.
pub fn text_marker(n: usize, text: &str) -> String {
    format!("[Pasted text #{n} +{} lines]", text.lines().count())
}

/// What a paste turns into.
#[derive(Debug, PartialEq)]
pub enum Pasted {
    /// Goes into the input as is.
    Text(String),
    /// A file to attach; MARKER goes into the input.
    File { path: PathBuf, marker: String },
    /// Empty paste, 0 clipboard image, 0 clipboard text.
    Nothing,
}

/// Counters + the dir of the pasted files.
pub struct Paster {
    pub dir: PathBuf,
    pub images: usize,
    pub texts: usize,
}

impl Paster {
    /// The counters go on after the files already in DIR (`image-N.png`, `pasted-text-N.txt`):
    /// a new paste never overwrites the file of an earlier message.
    pub fn new(dir: PathBuf) -> Paster {
        let mut p = Paster { dir, images: 0, texts: 0 };
        for e in std::fs::read_dir(&p.dir).into_iter().flatten().flatten() {
            let name = e.file_name();
            let name = name.to_string_lossy();
            let n = |pre: &str, ext: &str| name.strip_prefix(pre)?.strip_suffix(ext)?.parse::<usize>().ok();
            if let Some(n) = n("image-", ".png") {
                p.images = p.images.max(n);
            } else if let Some(n) = n("pasted-text-", ".txt") {
                p.texts = p.texts.max(n);
            }
        }
        p
    }

    /// MARKER (`[Image #3]`, `[Pasted text #2 +5 lines]`) is in use: new pastes number after it.
    pub fn seen(&mut self, marker: &str) {
        let n = |pre: &str| marker.strip_prefix(pre)?.split([']', ' ']).next()?.parse::<usize>().ok();
        if let Some(n) = n("[Image #") {
            self.images = self.images.max(n);
        } else if let Some(n) = n("[Pasted text #") {
            self.texts = self.texts.max(n);
        }
    }

    /// A paste event's text. Empty (cmd+v on an image in some terminals): the clipboard image.
    pub fn paste(&mut self, text: &str) -> Pasted {
        if text.is_empty() {
            return self.clipboard();
        }
        if let Some(f) = dropped_file(text) {
            if is_image(&f) {
                self.images += 1;
                return Pasted::File { path: f, marker: format!("[Image #{}]", self.images) };
            }
            return Pasted::Text(f.display().to_string());
        }
        if text.chars().count() <= PASTE_INLINE_MAX || crate::code::looks_code(text) {
            return Pasted::Text(text.to_string());
        }
        // terminals paste line breaks as \r: count and store them as \n
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        let f = self.dir.join(format!("pasted-text-{}.txt", self.texts + 1));
        let _ = std::fs::create_dir_all(&self.dir);
        if std::fs::write(&f, &text).is_err() {
            return Pasted::Text(text);
        }
        self.texts += 1;
        Pasted::File { marker: text_marker(self.texts, &text), path: f }
    }

    /// ctrl+v (Claude Code's image key): the clipboard image, else the clipboard text.
    pub fn clipboard(&mut self) -> Pasted {
        let f = self.dir.join(format!("image-{}.png", self.images + 1));
        if clip::paste_image(&f) {
            self.images += 1;
            return Pasted::File { path: f, marker: format!("[Image #{}]", self.images) };
        }
        match clip::paste() {
            Some(t) if !t.is_empty() => self.paste(&t),
            _ => Pasted::Nothing,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paste_rules() {
        let dir = std::env::temp_dir().join(format!("codehead-paste-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let img = dir.join("a b.png");
        std::fs::write(&img, b"\x89PNG").unwrap();
        let s = img.display().to_string();
        assert_eq!(dropped_file(&s), Some(img.clone()));
        assert_eq!(dropped_file(&format!("'{s}'")), Some(img.clone()));
        assert_eq!(dropped_file(&s.replace(' ', "\\ ")), Some(img.clone()));
        assert_eq!(dropped_file(&format!("file://{s}")), Some(img.clone()));
        assert_eq!(dropped_file("hello world"), None);
        assert!(is_image(&img));
        assert_eq!(text_marker(2, "a\nb\nc"), "[Pasted text #2 +3 lines]");
        let mut p = Paster::new(dir.join("out"));
        assert_eq!(p.paste(&s), Pasted::File { path: img.clone(), marker: "[Image #1]".into() });
        assert_eq!(p.paste("short"), Pasted::Text("short".into()));
        let long = "x\r".repeat(600);
        let code = "let a = 1;\n".repeat(200);
        assert_eq!(p.paste(&code), Pasted::Text(code.clone()));
        match p.paste(&long) {
            Pasted::File { path, marker } => {
                assert_eq!(marker, "[Pasted text #1 +600 lines]");
                assert!(std::fs::read_to_string(path).unwrap().starts_with("x\nx\n"));
            }
            other => panic!("{other:?}"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_new_paster_numbers_after_the_files_and_markers_in_use() {
        let dir = std::env::temp_dir().join(format!("codehead-paste-again-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("image-1.png"), b"first").unwrap();
        std::fs::write(dir.join("image-3.png"), b"third").unwrap();
        std::fs::write(dir.join("pasted-text-2.txt"), b"x").unwrap();
        std::fs::write(dir.join("image-x.png"), b"x").unwrap();
        let mut p = Paster::new(dir.clone());
        assert_eq!((p.images, p.texts), (3, 2));
        p.seen("[Image #5]");
        p.seen("[Pasted text #4 +9 lines]");
        p.seen("[Image #2]");
        assert_eq!((p.images, p.texts), (5, 4));
        let long = "y\n".repeat(600);
        assert!(matches!(p.paste(&long), Pasted::File { marker, .. } if marker == "[Pasted text #5 +600 lines]"));
        assert_eq!(std::fs::read(dir.join("image-1.png")).unwrap(), b"first");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
