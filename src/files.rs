//! The files view of a thread: the files its agents wrote or edited (their tool calls), and the
//! home repo's notes of the thread (`thread: <id>` on line 1), newest first; a preview right
//! (Markdown rendered, code in syntax colors). yazi (full screen) browses anything else.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::engine::store::{self, Kind};
use crate::graphics::Preview;
use crate::theme;

/// Where a file comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// An agent's Write / Edit call.
    Agent,
    /// A home repo note of this thread.
    Note,
    /// A file a message named (a click on its link).
    Named,
    /// A file the owner attached to a message (a paste, a drop), with its marker (`[Image #1]`).
    Attached(Option<String>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub path: PathBuf,
    pub source: Source,
    /// Last change, unix s.
    pub at: u64,
}

/// Tool inputs that name the file they write.
const WRITE_TOOLS: [&str; 5] = ["Write", "Edit", "MultiEdit", "NotebookEdit", "str_replace_based_edit_tool"];

/// The files of thread ID's log, 1 read: the agents wrote or edited (absolute; relative ones
/// from DIR), the owner attached to messages (with markers); each once.
pub fn log_files(id: &str, dir: &Path) -> (Vec<PathBuf>, Vec<(PathBuf, Option<String>)>) {
    let mut wrote: Vec<PathBuf> = Vec::new();
    let mut attached: Vec<(PathBuf, Option<String>)> = Vec::new();

    for e in store::events(id) {
        match e.kind {
            Kind::Tool { name, input, .. }
                if WRITE_TOOLS.contains(&name.as_str()) || input["_kind"] == "edit" =>
            {
                if let Some(p) = input["file_path"].as_str().or_else(|| input["path"].as_str()).or_else(|| input["notebook_path"].as_str()) {
                    let p = if Path::new(p).is_absolute() { PathBuf::from(p) } else { dir.join(p) };
                    if !wrote.contains(&p) {
                        wrote.push(p);
                    }
                }
            }
            Kind::Prompt { from, text, .. } if from == "owner" => {
                for a in crate::stream::attachments(&text).1 {
                    let p = PathBuf::from(&a.path);
                    if !attached.iter().any(|(q, _)| *q == p) {
                        attached.push((p, a.marker));
                    }
                }
            }
            _ => {}
        }
    }

    (wrote, attached)
}

/// Files the agents of thread ID wrote or edited (absolute; relative ones from DIR), each once.
#[cfg(test)]
pub fn agent_files(id: &str, dir: &Path) -> Vec<PathBuf> {
    log_files(id, dir).0
}

/// Markdown files under ROOT whose line 1 names thread ID (`thread: <id>`), at most 4 dirs deep.
pub fn notes_of(root: &Path, id: &str) -> Vec<PathBuf> {
    fn walk(d: &Path, depth: usize, id: &str, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(d) else { return };
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name();
            if name.to_string_lossy().starts_with('.') {
                continue;
            }
            if p.is_dir() && depth < 4 {
                walk(&p, depth + 1, id, out);
            } else if p.extension().is_some_and(|x| x == "md") {
                use std::io::{BufRead, BufReader};
                let first = std::fs::File::open(&p).ok().and_then(|f| BufReader::new(f).lines().next()).and_then(Result::ok).unwrap_or_default();
                if first.starts_with("thread:") && first.contains(id) {
                    out.push(p);
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(root, 0, id, &mut out);
    out
}

fn mtime(p: &Path) -> u64 {
    std::fs::metadata(p).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs())
}

/// The files of thread ID (DIR: its work dir): existing ones, newest first.
pub fn load(id: &str, dir: &Path) -> Vec<Entry> {
    let (wrote, attached) = log_files(id, dir);
    let mut v: Vec<Entry> = wrote.into_iter().filter(|p| p.is_file()).map(|p| Entry { at: mtime(&p), path: p, source: Source::Agent }).collect();
    if let Some(home) = crate::config::home_repo() {
        for p in notes_of(&home, id) {
            if !v.iter().any(|e| e.path == p) {
                v.push(Entry { at: mtime(&p), path: p, source: Source::Note });
            }
        }
    }
    // attachments: a file in another section too stays there (1 row per file)
    for (p, marker) in attached {
        if p.is_file() && !v.iter().any(|e| e.path == p) {
            v.push(Entry { at: mtime(&p), path: p, source: Source::Attached(marker) });
        }
    }
    v.sort_by_key(|e| std::cmp::Reverse(e.at));
    v
}

/// An image preview: path, mtime, cols, rows.
type ImageKey = (PathBuf, u64, u16, u16);

/// Image encodes kept (back and forth between files: 0 new encode).
const MAX_IMAGES: usize = 4;
/// The cursor (or a resize) rests this long before an encode starts: fast j k over 20 photos
/// starts 1 encode, not 20.
const IMAGE_REST: Duration = if cfg!(test) { Duration::ZERO } else { Duration::from_millis(80) };

/// What a key asks the thread view to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    None,
    Toast(String),
    /// The IDE at line 1 of this file.
    Open(PathBuf),
    /// yazi with this file revealed.
    Yazi(PathBuf),
}

/// A section of the list: where a file sits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Sec {
    /// An agent's file in the thread's work dir.
    Repo,
    /// A home repo note of the thread.
    Notes,
    /// An agent's file in the home repo (not a note of the thread).
    Home,
    /// An agent's file anywhere else.
    Other,
    /// A file a message named.
    Named,
    /// A file the owner attached to a message: last.
    Attached,
}

impl Sec {
    pub fn label(self) -> &'static str {
        match self {
            Sec::Repo => "repo changes",
            Sec::Notes => "agent notes",
            Sec::Home => "home repo",
            Sec::Other => "other files",
            Sec::Named => "named in messages",
            Sec::Attached => "attachments",
        }
    }
}

/// A row of the list: a section heading, a blank row, a file (position in `visible`).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Row {
    Head(Sec),
    Gap,
    File(usize),
}

pub struct View {
    id: String,
    dir: PathBuf,
    pub files: Vec<Entry>,
    /// The files in the list, in order (index into `files`), with the marks of the filter.
    visible: Vec<(usize, Vec<usize>)>,
    /// Cursor: position in `visible`.
    at: usize,
    /// Preview: rows per (path, width, mtime), scroll.
    cache: HashMap<(PathBuf, u16, u64), Vec<Line<'static>>>,
    /// File text for the filter: per (path, mtime), the first 256 KiB.
    texts: HashMap<(PathBuf, u64), String>,
    scroll: usize,
    loaded: Option<Instant>,
    /// A change came: 1 more read (after the one under way).
    again: bool,
    /// Bus topics: the thread's log, its work tree, the home repo → 1 read (0 timer).
    topics: std::sync::mpsc::Receiver<crate::bus::Topic>,
    /// The work trees the list comes from (canonical), watched while the view lives.
    roots: Vec<PathBuf>,
    _shown: Vec<crate::watch::Shown>,
    /// The list's read (the thread log, the home repo's notes): off the UI thread.
    reads: crate::background::Reads<(), Vec<Entry>>,
    /// Preview renders (markdown, code highlight): off the UI thread.
    renders: crate::background::Reads<(PathBuf, u16, u64), Vec<Line<'static>>>,
    /// Image + pdf previews, encoded for the terminal per (path, mtime, cols, rows): off the UI
    /// thread; the last MAX_IMAGES kept. Err: a line why 0 image.
    images: HashMap<ImageKey, Result<Preview, String>>,
    image_reads: crate::background::Reads<ImageKey, Result<Preview, String>>,
    /// The terminal's image picker (tests: a fixed one).
    picker: fn() -> Option<ratatui_image::picker::Picker>,
    /// The cached encodes, oldest first.
    image_order: Vec<ImageKey>,
    /// The image the preview shows now, since when (an encode starts once it rests).
    image_want: Option<(ImageKey, Instant)>,
    /// A wanted image not polled yet: the 1st poll after its rest wakes a draw (1x, not per tick).
    image_kick: bool,
    /// Data versions (a list read in, a preview in): their areas dissolve to the new rows.
    list_version: u64,
    preview_version: u64,
    list_fx: crate::dissolve::Dissolve,
    preview_fx: crate::dissolve::Dissolve,
    /// Files messages named (clicked links): they stay in the list.
    named: Vec<PathBuf>,
    /// Fuzzy filter (f): name and content; typing while focused.
    filter: crate::textarea::TextArea,
    filtering: bool,
    /// A 1st S (share: commit + push) at: a 2nd within 3 s shares.
    share_armed: Option<std::time::Instant>,
    /// S S: the share (git add, commit) off the UI thread; its toast.
    shared: Option<std::sync::mpsc::Receiver<String>>,
    list: Rect,
    /// The drawn rows of the list: (y, row).
    drawn: Vec<(u16, Row)>,
    preview: Rect,
}

impl View {
    pub fn new(id: &str, dir: &Path) -> View {
        let mut filter = crate::textarea::TextArea::new(true, "f: fuzzy search names and content");
        filter.style = Some(theme::text());
        // the work dir (agent files) and the home repo (notes): their changes read the list again
        let roots: Vec<PathBuf> = std::iter::once(dir.to_path_buf()).chain(crate::config::home_repo()).filter(|r| r.is_dir()).collect();
        View {
            id: id.to_string(),
            dir: dir.to_path_buf(),
            files: Vec::new(),
            visible: Vec::new(),
            at: 0,
            cache: HashMap::new(),
            texts: HashMap::new(),
            scroll: 0,
            loaded: None,
            again: false,
            topics: crate::bus::subscribe(),
            roots: roots.iter().map(|r| crate::watch::canon(r)).collect(),
            _shown: roots.iter().map(|r| crate::watch::show(r)).collect(),
            reads: crate::background::Reads::default(),
            renders: crate::background::Reads::default(),
            images: HashMap::new(),
            image_reads: crate::background::Reads::default(),
            picker: crate::graphics::picker,
            image_order: Vec::new(),
            image_want: None,
            image_kick: false,
            list_version: 0,
            preview_version: 0,
            list_fx: Default::default(),
            preview_fx: Default::default(),
            named: Vec::new(),
            filter,
            filtering: false,
            share_armed: None,
            shared: None,
            list: Rect::default(),
            drawn: Vec::new(),
            preview: Rect::default(),
        }
    }

    /// The section of E.
    fn sec(&self, e: &Entry) -> Sec {
        match e.source {
            Source::Note => Sec::Notes,
            Source::Named => Sec::Named,
            Source::Attached(_) => Sec::Attached,
            Source::Agent if e.path.starts_with(&self.dir) => Sec::Repo,
            Source::Agent if crate::config::home_repo().is_some_and(|h| e.path.starts_with(h)) => Sec::Home,
            Source::Agent => Sec::Other,
        }
    }

    /// The file under the cursor.
    fn cur(&self) -> Option<&Entry> {
        self.visible.get(self.at).and_then(|(i, _)| self.files.get(*i))
    }

    /// Keys go to the filter input.
    pub fn typing(&self) -> bool {
        self.filtering
    }

    /// The list again: sections in order, newest first in each; a filter: best match first.
    fn rebuild(&mut self) {
        let cur = self.cur().map(|e| e.path.clone());
        let mut order: Vec<usize> = (0..self.files.len()).collect();
        order.sort_by_key(|&i| (self.sec(&self.files[i]), std::cmp::Reverse(self.files[i].at)));
        let q = self.filter.text();
        self.visible = if q.trim().is_empty() {
            order.into_iter().map(|i| (i, Vec::new())).collect()
        } else {
            let names: Vec<String> = order.iter().map(|&i| self.label(&self.files[i])).collect();
            let bodies: Vec<String> = order.iter().map(|&i| self.text_of(i)).collect();
            let idx: Vec<usize> = (0..order.len()).collect();
            crate::fuzzy::rank(&idx, &q, |&k| names[k].clone(), |&k| bodies[k].clone()).into_iter().map(|(k, marks)| (order[k], marks)).collect()
        };
        self.at = cur.and_then(|p| self.visible.iter().position(|(i, _)| self.files[*i].path == p)).unwrap_or(0);
    }

    /// The text of file I for the filter (cached per mtime).
    fn text_of(&mut self, i: usize) -> String {
        let e = &self.files[i];
        let key = (e.path.clone(), e.at);
        if let Some(t) = self.texts.get(&key) {
            return t.clone();
        }
        use std::io::Read;
        let mut buf = Vec::new();
        if let Ok(f) = std::fs::File::open(&e.path) {
            let _ = f.take(256 << 10).read_to_end(&mut buf);
        }
        // binary files: 0 text
        let t = if buf.contains(&0) { String::new() } else { String::from_utf8_lossy(&buf).into_owned() };
        if self.texts.len() > 512 {
            self.texts.clear();
        }
        self.texts.insert(key, t.clone());
        t
    }

    /// Read the list: the 1st time, and after a change (NOW: a bus topic named this list).
    pub fn refresh(&mut self, now: bool) {
        self.again |= now;
        if !self.again && self.loaded.is_some() {
            return;
        }
        self.loaded = Some(Instant::now());
        // tests: at once
        if cfg!(test) {
            self.again = false;
            let files = load(&self.id, &self.dir);
            let _ = self.apply(files);
            return;
        }
        // a read under way: 1 more after it (the change came after its start)
        if !self.reads.waiting(&()) {
            self.again = false;
            self.reads.forget(&());
            let (id, dir) = (self.id.clone(), self.dir.clone());
            self.reads.request((), move || {
                let v = load(&id, &dir);
                crate::bus::wake();
                v
            });
        }
    }

    /// A finished read of the list: in. True: the list changed.
    /// The S S share ended: its toast.
    pub fn take_toast(&mut self) -> Option<String> {
        let m = self.shared.as_ref()?.try_recv().ok()?;
        self.shared = None;
        Some(m)
    }

    pub fn poll(&mut self) -> bool {
        let reads = self.reads.poll();
        let mut changed = false;
        for (_, files) in reads {
            changed |= self.apply(files);
        }
        if changed {
            self.list_version += 1;
        }
        let renders = self.renders.poll();
        if !renders.is_empty() {
            if self.cache.len() > 64 {
                self.cache.clear();
            }
            self.cache.extend(renders);
            self.preview_version += 1;
            changed = true;
        }
        // encodes in: only the wanted one (fast j k, a resize drag: the others are stale)
        for (key, pre) in self.image_reads.poll() {
            if self.image_want.as_ref().is_some_and(|(k, _)| *k == key) {
                self.images.insert(key.clone(), pre);
                self.image_order.retain(|k| *k != key);
                self.image_order.push(key);
                changed = true;
            } else {
                // stale: 0 kept, 0 sent; back on it later: a new read at once
                self.image_reads.forget(&key);
            }
        }

        // the last 4 encodes stay (back and forth between files: 0 new encode); the oldest out
        while self.image_order.len() > MAX_IMAGES {
            let old = self.image_order.remove(0);
            self.drop_image(&old);
        }

        // the cursor rested on an image: 1 draw, which requests its encode (1x: 0 redraw per tick
        // after yazi, a pane change, `previews = off`)
        if self.image_kick && self.image_want.as_ref().is_some_and(|(_, at)| at.elapsed() >= IMAGE_REST) {
            self.image_kick = false;
            changed |= self.image_want.as_ref().is_some_and(|(k, _)| !self.images.contains_key(k) && !self.image_reads.waiting(k));
        }

        changed
    }

    /// The encode of KEY out (the cache, the terminal's kitty store); a new request runs at once.
    fn drop_image(&mut self, key: &ImageKey) {
        if let Some(Ok(Preview { kitty: Some(id), transmit: None, .. })) = self.images.remove(key) {
            crate::graphics::delete_kitty(id);
        }

        self.image_order.retain(|k| k != key);
        self.image_reads.forget(key);
    }

    /// The encoded images out: the next draw encodes (and sends) them again. After another
    /// program had the terminal (yazi, an editor): it may have deleted them.
    pub fn forget_images(&mut self) {
        for key in std::mem::take(&mut self.image_order) {
            self.drop_image(&key);
        }

        self.images.clear();
        self.image_reads.invalidate();
    }

    /// The image preview of P in BODY (cols × rows): drawn (true), or a line while it encodes or
    /// why 0 image. False: 0 image preview for P (a text file, 0 graphics terminal, 0 room).
    fn draw_image(&mut self, f: &mut Frame, p: &Path, head: Rect, body: Rect) -> bool {
        let kind = crate::graphics::kind(p);
        let picker = if kind.is_some() && body.width > 0 && body.height > 0 { (self.picker)() } else { None };
        let (Some(kind), Some(picker)) = (kind, picker) else {
            // 0 image wanted: 0 wake-ups for one
            self.image_want = None;
            self.image_kick = false;
            return false;
        };

        let key: ImageKey = (p.to_path_buf(), mtime(p), body.width, body.height);
        let hints = "y: yazi · o: IDE · c: copy path · ctrl+f: filter";
        let shown = self.shown(p);
        let line = |info: &str| Line::from(vec![Span::styled(format!(" {shown} "), theme::bold()), Span::styled(format!("{info} · {hints}"), theme::muted())]);

        // a new file or area: wait for the cursor (or the resize) to rest
        if self.image_want.as_ref().is_none_or(|(k, _)| *k != key) {
            self.image_want = Some((key.clone(), Instant::now()));
            self.image_kick = true;
        }

        match self.images.get_mut(&key) {
            Some(Ok(_)) => {
                // in use: the newest of the kept encodes (LRU)
                if self.image_order.last() != Some(&key) {
                    self.image_order.retain(|k| *k != key);
                    self.image_order.push(key.clone());
                }

                let Some(Ok(pre)) = self.images.get_mut(&key) else { return true };

                // kitty: the image data 1x straight to the terminal, the cells: placeholders
                if let Some(t) = pre.transmit.take() {
                    crate::graphics::send(&t);
                }

                f.render_widget(Paragraph::new(line(&pre.info)), head);
                f.render_widget(ratatui_image::Image::new(&pre.protocol), body);
            }
            Some(Err(why)) => f.render_widget(Paragraph::new(line(why)), head),
            None => {
                let rested = self.image_want.as_ref().is_some_and(|(_, at)| at.elapsed() >= IMAGE_REST);
                if rested && !self.image_reads.waiting(&key) {
                    let (path, cols, rows) = (p.to_path_buf(), body.width, body.height);
                    self.image_reads.request(key, move || crate::graphics::build(&path, kind, &picker, cols, rows));
                }

                f.render_widget(Paragraph::new(line("reading…")), head);
            }
        }

        true
    }

    /// FILES (a read of the list) + the named files: the list; the cursor stays on its file.
    /// True: the list changed.
    fn apply(&mut self, mut files: Vec<Entry>) -> bool {
        // named files (clicked links) stay
        for p in self.named.iter().rev() {
            if !files.iter().any(|e| e.path == *p) {
                files.insert(0, Entry { path: p.clone(), source: Source::Named, at: mtime(p) });
            }
        }
        if files != self.files || self.visible.len() != self.files.len() || !self.filter.text().trim().is_empty() {
            let cur = self.cur().map(|e| e.path.clone());
            let changed = files != self.files;
            self.files = files;
            self.rebuild();
            if let Some(p) = cur {
                self.at = self.visible.iter().position(|(i, _)| self.files[*i].path == p).unwrap_or(self.at);
            }
            return changed;
        }
        false
    }

    /// Pick PATH (a file a message named): in the list, else added.
    pub fn show(&mut self, path: &Path) {
        if !self.named.iter().any(|p| p == path) && !self.files.iter().any(|e| e.path == path) {
            self.named.push(path.to_path_buf());
        }
        self.filter.set_text("");
        self.filtering = false;
        // the named file at once; the rest of the list from the read
        if !self.files.iter().any(|e| e.path == path) {
            self.files.insert(0, Entry { path: path.to_path_buf(), source: Source::Named, at: mtime(path) });
        }
        self.refresh(true);
        self.rebuild();
        self.at = self.visible.iter().position(|(i, _)| self.files[*i].path == path).unwrap_or(0);
        self.scroll = 0;
    }

    pub fn count(&self) -> usize {
        self.files.len()
    }

    /// E's name in the list: an attachment's marker + file name (its paste dir says 0), else
    /// its path as shown.
    fn label(&self, e: &Entry) -> String {
        match &e.source {
            Source::Attached(m) => {
                let name = e.path.file_name().map_or_else(|| e.path.display().to_string(), |n| n.to_string_lossy().into_owned());
                // `[Pasted text #1 +40 lines]`: `[Pasted text #1]`
                m.as_ref().map_or(name.clone(), |m| format!("{}] {name}", m.split(" +").next().unwrap_or(m).trim_end_matches(']')))
            }
            _ => self.shown(&e.path),
        }
    }

    /// PATH as shown: relative to the work dir, else `~/…`.
    fn shown(&self, p: &Path) -> String {
        if let Ok(r) = p.strip_prefix(&self.dir) {
            return r.display().to_string();
        }
        let home = crate::config::home();
        match p.strip_prefix(&home) {
            Ok(r) => format!("~/{}", r.display()),
            Err(_) => p.display().to_string(),
        }
    }

    /// The preview of P at WIDTH: from the cache, else None and a render off the UI thread
    /// (tests: at once).
    fn preview_lines(&mut self, p: &Path, width: u16) -> Option<Vec<Line<'static>>> {
        let key = (p.to_path_buf(), width, mtime(p));
        if let Some(l) = self.cache.get(&key) {
            return Some(l.clone());
        }
        if cfg!(test) {
            let lines = render_preview(p, width);
            self.cache.insert(key, lines.clone());
            return Some(lines);
        }
        if !self.renders.waiting(&key) {
            let path = p.to_path_buf();
            self.renders.request(key, move || render_preview(&path, width));
        }
        None
    }

    /// The list rows: per section a heading, its files, a blank row between sections; a filter:
    /// the matches, best first, 0 headings.
    fn rows(&self) -> Vec<Row> {
        if !self.filter.text().trim().is_empty() {
            return (0..self.visible.len()).map(Row::File).collect();
        }
        let mut out = Vec::new();
        let mut last = None;
        for (k, (i, _)) in self.visible.iter().enumerate() {
            let s = self.sec(&self.files[*i]);
            if last != Some(s) {
                if last.is_some() {
                    out.push(Row::Gap);
                }
                out.push(Row::Head(s));
                last = Some(s);
            }
            out.push(Row::File(k));
        }
        out
    }

    pub fn draw(&mut self, f: &mut Frame, area: Rect) {
        self.poll();
        let mut hit = false;
        for t in self.topics.try_iter() {
            hit |= match &t {
                crate::bus::Topic::Thread(id, crate::bus::Part::Log | crate::bus::Part::Other) => *id == self.id,
                crate::bus::Topic::Work(w) | crate::bus::Topic::Rescan(w) => self.roots.iter().any(|r| r.starts_with(w) || w.starts_with(r)),
                _ => false,
            };
        }
        self.refresh(hit);
        if self.files.is_empty() {
            f.render_widget(Paragraph::new(Line::styled("  0 files yet: the files the agents write, and the home repo's notes of this thread · menu: yazi", theme::muted())), area);
            return;
        }
        let lw = (area.width * 2 / 5).clamp(24, 60).min(area.width);
        // the filter row on top of the list, a blank row below it
        let fr = Rect { width: lw, height: 1, ..area };
        let list = Rect { y: area.y + 2, height: area.height.saturating_sub(2), width: lw, ..area };
        let preview = Rect { x: area.x + lw + 1, width: area.width.saturating_sub(lw + 1), ..area };
        f.render_widget(Paragraph::new(Span::styled(" › ", if self.filtering { theme::accent_bold() } else { theme::muted() })), Rect { width: 3, ..fr });
        let q = self.filter.text();
        let count = if q.trim().is_empty() { String::new() } else { format!(" {} of {}", self.visible.len(), self.files.len()) };
        let cw = count.chars().count() as u16;
        self.filter.render(f, Rect { x: fr.x + 3, width: fr.width.saturating_sub(3 + cw), ..fr }, self.filtering);
        if cw > 0 {
            f.render_widget(Paragraph::new(Span::styled(count, theme::muted())), Rect { x: fr.right().saturating_sub(cw), width: cw, ..fr });
        }
        self.list = list;
        self.preview = preview;
        // the divider
        for y in area.y..area.bottom() {
            f.buffer_mut()[(area.x + lw, y)].set_symbol("│").set_style(theme::line());
        }
        self.drawn.clear();
        if self.visible.is_empty() {
            f.render_widget(Paragraph::new(Span::styled("  0 matches", theme::muted())), Rect { height: 1, ..list });
            return;
        }
        self.at = self.at.min(self.visible.len() - 1);
        let rows = self.rows();
        let h = usize::from(list.height);
        // the cursor row in view; the heading above the 1st file in view where it fits
        let cur_row = rows.iter().position(|r| *r == Row::File(self.at)).unwrap_or(0);
        let top = cur_row.saturating_sub(h.saturating_sub(1));
        let top = if top > 0 && matches!(rows.get(top.saturating_sub(1)), Some(Row::Head(_))) && cur_row - (top - 1) < h { top - 1 } else { top };
        let now = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let hit = Style::default().fg(theme::pal().accent).add_modifier(ratatui::style::Modifier::BOLD | ratatui::style::Modifier::UNDERLINED);
        for (n, row) in rows.iter().enumerate().skip(top).take(h) {
            let y = list.y + (n - top) as u16;
            let r = Rect { y, height: 1, ..list };
            self.drawn.push((y, *row));
            match *row {
                Row::Gap => {}
                Row::Head(s) => {
                    f.render_widget(Paragraph::new(Span::styled(format!(" {}", s.label()), theme::muted().add_modifier(ratatui::style::Modifier::BOLD))), r);
                }
                Row::File(k) => {
                    let (i, marks) = &self.visible[k];
                    let e = &self.files[*i];
                    let tag = match e.source {
                        Source::Agent => Span::styled("✎ ", theme::fg(theme::pal().info())),
                        Source::Note => Span::styled("◆ ", theme::muted()),
                        Source::Named => Span::styled("↗ ", theme::muted()),
                        Source::Attached(_) => Span::styled("⧉ ", theme::muted()),
                    };
                    let age = crate::notes::ago(SystemTime::UNIX_EPOCH + Duration::from_secs(e.at.min(now)));
                    let name = self.label(e);
                    let room = usize::from(lw).saturating_sub(age.chars().count() + 5);
                    let sel = k == self.at;
                    let st = if sel { theme::pill() } else { Style::default() };
                    // a long path: its end shown (`…` + the tail), the marks moved along
                    let n = name.chars().count();
                    let (fitted, marks): (String, Vec<usize>) = if n <= room || room < 2 {
                        (name.clone(), marks.clone())
                    } else {
                        let cut = n - (room - 1);
                        (format!("…{}", name.chars().skip(cut).collect::<String>()), marks.iter().filter(|&&m| m >= cut).map(|m| m - cut + 1).collect())
                    };
                    let marks = &marks[..];
                    let mut spans = vec![Span::raw(" "), tag];
                    spans.extend(crate::fuzzy::spans(&fitted, marks, if sel { st } else { theme::text() }, if sel { st.add_modifier(ratatui::style::Modifier::UNDERLINED) } else { hit }));
                    spans.push(Span::styled(format!(" {age}"), theme::muted()));
                    f.render_widget(Paragraph::new(Line::from(spans)).style(st), r);
                }
            }
        }
        self.list_fx.apply(f, list, self.list_version);
        let Some(p) = self.cur().map(|e| e.path.clone()) else { return };
        // images + pdfs: drawn by the terminal (0 dissolve over graphics)
        let body = Rect { x: preview.x + 1, y: preview.y + 2, height: preview.height.saturating_sub(2), width: preview.width.saturating_sub(1) };
        if self.draw_image(f, &p, Rect { height: 1, ..preview }, body) {
            return;
        }
        let Some(lines) = self.preview_lines(&p, preview.width.saturating_sub(1)) else {
            f.render_widget(Paragraph::new(Line::styled(format!(" {} · reading…", self.shown(&p)), theme::muted())), Rect { height: 1, ..preview });
            return;
        };
        self.scroll = self.scroll.min(lines.len().saturating_sub(1));
        let head = Line::from(vec![Span::styled(format!(" {} ", self.shown(&p)), theme::bold()), Span::styled(format!("{} lines · o: IDE · y: yazi · c: copy path · ctrl+f: filter", lines.len()), theme::muted())]);
        f.render_widget(Paragraph::new(head), Rect { height: 1, ..preview });
        let body: Vec<Line> = lines.into_iter().skip(self.scroll).take(usize::from(preview.height.saturating_sub(2))).collect();
        f.render_widget(Paragraph::new(body), Rect { x: preview.x + 1, y: preview.y + 2, height: preview.height.saturating_sub(2), width: preview.width.saturating_sub(1) });
        self.preview_fx.apply(f, preview, self.preview_version);
    }

    fn step(&mut self, d: isize) {
        let n = self.visible.len();
        if n == 0 {
            return;
        }
        self.at = (self.at as isize + d).clamp(0, n as isize - 1) as usize;
        self.scroll = 0;
    }

    pub fn key(&mut self, k: KeyEvent) -> Act {
        // the filter input: typing filters; ↑↓ move; enter keeps the matches; esc clears
        if self.filtering {
            match k.code {
                KeyCode::Esc => {
                    self.filtering = false;
                    self.filter.set_text("");
                    self.rebuild();
                }
                KeyCode::Enter => self.filtering = false,
                KeyCode::Down => self.step(1),
                KeyCode::Up => self.step(-1),
                _ => {
                    let before = self.filter.text();
                    self.filter.key(k);
                    if self.filter.text() != before {
                        self.rebuild();
                        self.at = 0;
                        self.scroll = 0;
                    }
                }
            }
            return Act::None;
        }
        // ctrl+f: the filter (the only key to it, in every list)
        if crate::textarea::is_filter_key(&k) {
            self.filtering = true;
            return Act::None;
        }

        // a letter with ctrl / alt / ⌘: 0 view key (alt+S never shares)
        if matches!(k.code, KeyCode::Char(_)) && k.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER) {
            return Act::None;
        }

        let armed = self.share_armed.take().filter(|t| t.elapsed() < std::time::Duration::from_secs(3));
        let page = usize::from(self.preview.height.saturating_sub(3)).max(1);
        match k.code {
            KeyCode::Down | KeyCode::Char('j') => self.step(1),
            KeyCode::Up | KeyCode::Char('k') => self.step(-1),
            KeyCode::PageDown | KeyCode::Char(' ') => self.scroll += page,
            KeyCode::PageUp => self.scroll = self.scroll.saturating_sub(page),
            KeyCode::Char('g') | KeyCode::Home => self.scroll = 0,
            KeyCode::Char('o' | 'e' | 'i') => return self.cur().map_or(Act::None, |e| Act::Open(e.path.clone())),
            // y: yazi on the file (0 files: on the work dir)
            KeyCode::Char('y') => return Act::Yazi(self.cur().map_or_else(|| self.dir.clone(), |e| e.path.clone())),
            KeyCode::Char('c') => {
                if let Some(e) = self.cur() {
                    let p = e.path.display().to_string();
                    let ok = crate::clip::copy(&p);
                    return Act::Toast(if ok { format!("copied {p}") } else { "0 clipboard tool".into() });
                }
            }
            KeyCode::Char('r') => self.refresh(true),
            // S S: share the thread's home repo files (commit those paths, push if tracked)
            KeyCode::Char('S') => {
                let Some(home) = crate::config::home_repo() else { return Act::Toast("0 home repo (setting home)".into()) };
                if armed.is_none() {
                    self.share_armed = Some(std::time::Instant::now());
                    return Act::Toast("S again: share (commit + push the thread's home repo files)".into());
                }
                let paths: Vec<PathBuf> = self.files.iter().map(|e| e.path.clone()).collect();
                let title = store::load(&self.id).map(|t| t.title).unwrap_or_default();
                let run = move || share(&home, &paths, &title).unwrap_or_else(|e| format!("share: {e}"));
                // 4 git runs: off the UI thread (tests: at once)
                if cfg!(test) {
                    return Act::Toast(run());
                }
                let (tx, rx) = crate::bus::channel();
                self.shared = Some(rx);
                std::thread::spawn(move || {
                    let _ = tx.send(run());
                });
                return Act::Toast("sharing…".into());
            }
            _ => {}
        }
        Act::None
    }

    pub fn mouse(&mut self, m: MouseEvent) {
        let at = ratatui::layout::Position { x: m.column, y: m.row };
        // the filter input: its own mouse (cursor, selection + copy)
        if !self.files.is_empty() && self.filter.mouse(m) {
            self.filter.copied = None;
            if matches!(m.kind, MouseEventKind::Down(_)) {
                self.filtering = true;
            }
            return;
        }
        match m.kind {
            MouseEventKind::ScrollDown if self.preview.contains(at) => self.scroll += 3,
            MouseEventKind::ScrollUp if self.preview.contains(at) => self.scroll = self.scroll.saturating_sub(3),
            MouseEventKind::ScrollDown if self.list.contains(at) => self.step(1),
            MouseEventKind::ScrollUp if self.list.contains(at) => self.step(-1),
            MouseEventKind::Down(_) if self.list.contains(at) => {
                if let Some((_, Row::File(k))) = self.drawn.iter().find(|(y, _)| *y == m.row) {
                    self.at = *k;
                    self.scroll = 0;
                    self.filtering = false;
                }
            }
            MouseEventKind::Down(_) if at.y + 2 == self.list.y && at.x < self.list.right() => self.filtering = true,
            _ => {}
        }
    }
}

/// Share the files of FILES under HOME (the home repo): `git add` + `git commit` of only those
/// paths (message: notes of TITLE), then a push in the background when the branch has an
/// upstream. Ok(what happened).
pub fn share(home: &Path, files: &[PathBuf], title: &str) -> Result<String, String> {
    let rel: Vec<String> = files.iter().filter_map(|p| p.strip_prefix(home).ok()).map(|p| p.display().to_string()).collect();
    if rel.is_empty() {
        return Err("0 home repo files in this thread".into());
    }
    let git = |args: &[&str]| -> Result<String, String> {
        let o = std::process::Command::new("git").arg("-C").arg(home).args(args).stdin(std::process::Stdio::null()).output().map_err(|e| format!("git: {e}"))?;
        if o.status.success() { Ok(String::from_utf8_lossy(&o.stdout).into_owned()) } else { Err(String::from_utf8_lossy(&o.stderr).lines().next().unwrap_or("git failed").to_string()) }
    };
    let mut add = vec!["add", "--"];
    add.extend(rel.iter().map(String::as_str));
    git(&add)?;
    let mut st = vec!["diff", "--cached", "--name-only", "--"];
    st.extend(rel.iter().map(String::as_str));
    let staged = git(&st)?;
    if staged.trim().is_empty() {
        return Ok("home repo: 0 changes to share".into());
    }
    let msg = format!("notes: {}", title.lines().next().unwrap_or("thread"));
    let mut commit = vec!["commit", "-q", "-m", &msg, "--"];
    commit.extend(rel.iter().map(String::as_str));
    git(&commit)?;
    let n = staged.lines().count();
    if git(&["rev-parse", "--abbrev-ref", "@{u}"]).is_ok() {
        let _ = std::process::Command::new("git").arg("-C").arg(home).args(["push", "-q"]).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn();
        Ok(format!("home repo: {n} files committed, pushing"))
    } else {
        Ok(format!("home repo: {n} files committed (0 upstream: 0 push)"))
    }
}

/// yazi's config dir owned by codehead: a theme from the palette (0 change to ~/.config/yazi).
pub fn yazi_config() -> PathBuf {
    let dir = crate::config::home().join(".cache/codehead/yazi");
    let _ = std::fs::create_dir_all(&dir);
    let hex = |c| theme::hex(c);
    let p = theme::pal();
    let theme_toml = format!(
        "[mgr]\ncwd = {{ fg = \"{acc}\" }}\nhovered = {{ fg = \"{on}\", bg = \"{acc}\" }}\npreview_hovered = {{ underline = true }}\nborder_style = {{ fg = \"{line}\" }}\n\n[status]\nsep_left = {{ open = \"\", close = \"\" }}\nsep_right = {{ open = \"\", close = \"\" }}\n\n[mode]\nnormal_main = {{ fg = \"{on}\", bg = \"{acc}\", bold = true }}\nnormal_alt = {{ fg = \"{acc}\", bg = \"{panel}\" }}\n",
        acc = hex(p.accent),
        on = hex(p.on_accent),
        line = hex(p.overlay0),
        panel = hex(p.surface0)
    );
    let _ = std::fs::write(dir.join("theme.toml"), theme_toml);
    // enter on a file: pick it (back to codehead), as yazi.nvim does
    let _ = std::fs::write(dir.join("keymap.toml"), "[[mgr.prepend_keymap]]\non = \"<Enter>\"\nrun = \"open\"\ndesc = \"pick: back to codehead\"\n");
    dir
}

/// The yazi command for TARGET (a dir, or a file to reveal): argv, env, the chooser file.
pub fn yazi_cmd(target: &Path) -> (Vec<String>, Vec<(String, String)>, PathBuf) {
    let chooser = std::env::temp_dir().join(format!("codehead-yazi-{}", std::process::id()));
    let _ = std::fs::remove_file(&chooser);
    let argv = vec!["yazi".to_string(), target.display().to_string(), format!("--chooser-file={}", chooser.display())];
    (argv, vec![("YAZI_CONFIG_HOME".into(), yazi_config().display().to_string())], chooser)
}

/// The preview rows of P at WIDTH: markdown, else highlighted code.
fn render_preview(p: &Path, width: u16) -> Vec<Line<'static>> {
    let text = std::fs::read(p).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_else(|e| format!("{e}"));
    let w = usize::from(width);
    if p.extension().is_some_and(|x| x == "md") {
        crate::render::markdown(&text, w, &[]).into_iter().map(|r| r.line).collect()
    } else {
        let lang = p.extension().map(|x| x.to_string_lossy().into_owned()).unwrap_or_else(|| "text".into());
        crate::render::code_rows(&text, &lang, w, &[], true, None).into_iter().skip(1).map(|r| r.line).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_files_come_from_write_and_edit_calls_once() {
        let _env = crate::engine::ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let t = store::Thread::new("f", "t", Path::new("/w"), vec!["ada".into()]);
        std::fs::create_dir_all(store::thread_dir(&t.id)).unwrap();
        t.save().unwrap();
        let tool = |name: &str, input: serde_json::Value| store::Event::new(Some("ada"), Kind::Tool { id: "1".into(), name: name.into(), input });
        for e in [
            tool("Write", serde_json::json!({"file_path": "/w/notes/plan.md"})),
            tool("Edit", serde_json::json!({"file_path": "src/a.rs"})),
            tool("Read", serde_json::json!({"file_path": "/w/README.md"})),
            tool("Edit", serde_json::json!({"file_path": "/w/notes/plan.md"})),
        ] {
            store::append(&t.id, &e).unwrap();
        }
        assert_eq!(agent_files(&t.id, Path::new("/w")), [PathBuf::from("/w/notes/plan.md"), PathBuf::from("/w/src/a.rs")]);
        let _ = std::fs::remove_dir_all(store::thread_dir(&t.id));
    }

    #[test]
    fn notes_of_a_thread_by_their_1st_line() {
        let d = std::env::temp_dir().join(format!("codehead-notes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("area/plans")).unwrap();
        std::fs::write(d.join("area/plans/a.md"), "thread: abc123 · branch: main · status: open\n# A").unwrap();
        std::fs::write(d.join("area/plans/b.md"), "thread: other · branch: main\n").unwrap();
        std::fs::write(d.join("area/INDEX.md"), "- plans/a.md\n").unwrap();
        assert_eq!(notes_of(&d, "abc123"), [d.join("area/plans/a.md")]);
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn share_commits_only_the_threads_home_files() {
        let d = std::env::temp_dir().join(format!("codehead-share-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("notes")).unwrap();
        let sh = |c: &str| assert!(std::process::Command::new("sh").arg("-c").arg(c).current_dir(&d).status().unwrap().success(), "{c}");
        sh("git init -q && git config user.email t@t && git config user.name t && echo x > other.md && git add -A && git commit -qm init");
        std::fs::write(d.join("notes/plan.md"), "thread: t1\n").unwrap();
        std::fs::write(d.join("other.md"), "changed\n").unwrap();
        let r = share(&d, &[d.join("notes/plan.md"), PathBuf::from("/elsewhere/x.rs")], "fix deploy").unwrap();
        assert_eq!(r, "home repo: 1 files committed (0 upstream: 0 push)");
        let log = std::process::Command::new("git").arg("-C").arg(&d).args(["show", "--stat", "--format=%s", "HEAD"]).output().unwrap();
        let log = String::from_utf8_lossy(&log.stdout);
        assert!(log.starts_with("notes: fix deploy") && log.contains("notes/plan.md") && !log.contains("other.md"), "{log}");
        assert_eq!(share(&d, &[d.join("notes/plan.md")], "x").unwrap(), "home repo: 0 changes to share");
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn yazi_runs_with_codeheads_config_and_a_chooser_file() {
        let (argv, env, chooser) = yazi_cmd(Path::new("/w/src"));
        assert_eq!(argv[..2], ["yazi".to_string(), "/w/src".to_string()]);
        assert!(argv[2].starts_with("--chooser-file=") && argv[2].ends_with(&chooser.display().to_string()));
        assert_eq!(env[0].0, "YAZI_CONFIG_HOME");
        let dir = PathBuf::from(&env[0].1);
        assert!(std::fs::read_to_string(dir.join("theme.toml")).unwrap().contains("[mgr]"));
    }
}

/// The list: sections with headings and gaps; the fuzzy filter (f) with ranked, marked matches.
#[cfg(test)]
mod view_tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::KeyModifiers;

    fn screen(v: &mut View) -> (Vec<String>, ratatui::buffer::Buffer) {
        let mut t = Terminal::new(TestBackend::new(100, 16)).unwrap();
        t.draw(|f| v.draw(f, f.area())).unwrap();
        let b = t.backend().buffer().clone();
        ((0..16).map(|y| (0..40).map(|x| b[(x, y)].symbol().to_string()).collect::<String>().trim_end().to_string()).collect(), b)
    }

    fn press(v: &mut View, keys: &str) {
        for c in keys.chars() {
            v.key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
    }

    #[test]
    fn letters_with_a_modifier_do_nothing_and_y_opens_yazi_on_the_dir_without_files() {
        let mut v = View::new("fileskeys-0000", Path::new("/tmp/fileskeys"));
        assert_eq!(v.key(KeyEvent::new(KeyCode::Char('S'), KeyModifiers::ALT | KeyModifiers::SHIFT)), Act::None);
        for c in ['f', '/'] {
            assert_eq!(v.key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)), Act::None);
            assert!(!v.filtering, "{c}: 0 filter (ctrl+f is)");
        }

        assert_eq!(v.key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL)), Act::None);
        assert!(v.filtering, "ctrl+f: the filter");
        v.filtering = false;

        assert_eq!(v.key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE)), Act::Yazi(PathBuf::from("/tmp/fileskeys")));
        assert_eq!(v.key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE)), Act::None, "0 file: 0 IDE");
    }

    #[test]
    fn attachments_get_their_own_section_last_once_per_file() {
        let _env = crate::engine::ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let d = std::env::temp_dir().join(format!("codehead-fattach-{}", std::process::id()));
        let paste = d.join("paste");
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("src")).unwrap();
        std::fs::create_dir_all(&paste).unwrap();
        std::fs::write(d.join("src/a.rs"), "fn a() {}").unwrap();
        std::fs::write(paste.join("image-1.png"), b"\x89PNG").unwrap();
        std::fs::write(paste.join("pasted-text-1.txt"), "long").unwrap();
        let t = store::Thread::new("f", "t", &d, vec!["ada".into()]);
        std::fs::create_dir_all(store::thread_dir(&t.id)).unwrap();
        t.save().unwrap();
        let (img, txt, code) = (paste.join("image-1.png"), paste.join("pasted-text-1.txt"), d.join("src/a.rs"));
        let prompt = |text: String| store::Event::new(None, Kind::Prompt { from: "owner".into(), to: vec!["ada".into()], text });
        store::append(&t.id, &prompt(format!("see [Image #1]\n\nAttachments (read these files):\n- [Image #1]: {}\n- {}", img.display(), code.display()))).unwrap();
        store::append(&t.id, &prompt(format!("again\n\nAttachments (read these files):\n- [Image #1]: {}\n- [Pasted text #1 +1 lines]: {}", img.display(), txt.display()))).unwrap();
        store::append(&t.id, &store::Event::new(Some("ada"), Kind::Tool { id: "1".into(), name: "Write".into(), input: serde_json::json!({"file_path": code}) })).unwrap();

        // the agent's file stays in its section; the 2 attachments once each, after it
        let files = load(&t.id, &d);
        assert_eq!(files.len(), 3, "{files:?}");
        assert_eq!(files.iter().find(|e| e.path == code).map(|e| &e.source), Some(&Source::Agent));
        assert_eq!(files.iter().find(|e| e.path == img).map(|e| &e.source), Some(&Source::Attached(Some("[Image #1]".into()))));

        // on screen: the section last, marker + name per row
        let mut v = View::new(&t.id, &d);
        let (s, _) = screen(&mut v);
        let at = |s: &[String], x: &str| s.iter().position(|r| r.contains(x)).unwrap_or_else(|| panic!("{x}: {s:#?}"));
        let (repo, att) = (at(&s, "repo changes"), at(&s, "attachments"));
        assert!(repo < att && at(&s, "src/a.rs") < att, "{s:#?}");
        assert!(at(&s, "[Image #1] image-1.png") > att && at(&s, "pasted-text-1.txt") > att, "{s:#?}");

        let _ = std::fs::remove_dir_all(store::thread_dir(&t.id));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// A thread in a temp dir whose agent wrote NAMES (png: an image of 2000×1000 px).
    fn image_thread(tag: &str, names: &[&str]) -> (store::Thread, PathBuf) {
        let d = std::env::temp_dir().join(format!("codehead-fimage-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let t = store::Thread::new("f", "t", &d, vec!["ada".into()]);
        std::fs::create_dir_all(store::thread_dir(&t.id)).unwrap();
        t.save().unwrap();

        for (i, name) in names.iter().enumerate() {
            let p = d.join(name);
            if name.ends_with(".png") {
                image::RgbaImage::from_pixel(2000, 1000, image::Rgba([10, 200, 10, 255])).save(&p).unwrap();
            } else {
                std::fs::write(&p, b"\x89PNG\0binary").unwrap();
            }
            store::append(&t.id, &store::Event::new(Some("ada"), Kind::Tool { id: i.to_string(), name: "Write".into(), input: serde_json::json!({"file_path": p}) })).unwrap();
        }

        (t, d)
    }

    fn iterm2() -> Option<ratatui_image::picker::Picker> {
        #[allow(deprecated)]
        let mut p = ratatui_image::picker::Picker::from_fontsize(ratatui_image::FontSize::new(10, 20));
        p.set_protocol_type(ratatui_image::picker::ProtocolType::Iterm2);
        Some(p)
    }

    /// All 100 cols (the `screen` helper: the list's 40).
    fn full(v: &mut View) -> (Vec<String>, ratatui::buffer::Buffer) {
        let (_, b) = screen(v);
        let s: Vec<String> = (0..16).map(|y| (0..100).map(|x| b[(x, y)].symbol().to_string()).collect::<String>()).collect();
        (s, b)
    }

    /// Polls until an encode is in (max 10 s).
    fn encoded(v: &mut View) {
        let end = Instant::now() + Duration::from_secs(10);
        while !v.poll() {
            assert!(Instant::now() < end, "0 encode in 10 s");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn an_image_previews_as_an_image_encoded_off_the_ui_thread_once() {
        let _env = crate::engine::ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (t, d) = image_thread("once", &["shot.png"]);
        let mut v = View::new(&t.id, &d);
        v.picker = iterm2;

        // 1st draw: the encode starts
        let (s, _) = full(&mut v);
        assert!(s.iter().any(|r| r.contains("shot.png") && r.contains("reading…")), "{s:#?}");

        // the encode in: the size line, 1 image (the iTerm2 sequence in the buffer)
        encoded(&mut v);
        let (s, b) = full(&mut v);
        assert!(s.iter().any(|r| r.contains("2000×1000 px") && r.contains("y: yazi")), "{s:#?}");
        assert!(b.content().iter().any(|c| c.symbol().contains("\x1b]1337;File=")), "an iTerm2 image");

        // a 2nd draw: from the cache, 0 new read
        screen(&mut v);
        assert_eq!(v.image_reads.pending(), 0);
        assert_eq!(v.images.len(), 1);

        // the image in: 0 wake-up per tick
        for _ in 0..3 {
            assert!(!v.poll());
        }

        // after yazi, before a draw (another pane): 0 wake-up per tick either
        v.forget_images();
        for _ in 0..3 {
            assert!(!v.poll());
        }

        // the next draw: encoded again, at once (0 wait for the 5 s read throttle)
        let (s, _) = full(&mut v);
        assert!(s.iter().any(|r| r.contains("reading…")), "{s:#?}");
        assert_eq!(v.image_reads.pending(), 1);

        let _ = std::fs::remove_dir_all(store::thread_dir(&t.id));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn an_image_without_a_graphics_terminal_keeps_the_text_preview() {
        let _env = crate::engine::ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (t, d) = image_thread("text", &["shot.png"]);
        let mut v = View::new(&t.id, &d);

        // the tests' default: 0 graphics terminal
        assert!((v.picker)().is_none());
        let (s, _) = full(&mut v);

        // the head of the text preview (`N lines`), 0 encode
        assert!(s.iter().any(|r| r.contains("shot.png") && r.contains("lines ·")), "{s:#?}");
        assert!(!s.iter().any(|r| r.contains("reading…") || r.contains(" px ·")), "{s:#?}");
        assert_eq!(v.image_reads.pending(), 0);

        let _ = std::fs::remove_dir_all(store::thread_dir(&t.id));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn at_most_4_encodes_stay_and_the_shown_one_is_never_dropped() {
        let _env = crate::engine::ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let names = ["a.png", "b.png", "c.png", "d.png", "e.png", "f.png"];
        let (t, d) = image_thread("lru", &names);
        let mut v = View::new(&t.id, &d);
        v.picker = iterm2;
        full(&mut v);

        // each file in turn, its encode in
        for _ in names {
            full(&mut v);
            encoded(&mut v);
            full(&mut v);
            v.key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        }

        // 4 kept; the one on screen among them
        assert_eq!(v.images.len(), MAX_IMAGES);
        assert_eq!(v.image_order.len(), MAX_IMAGES);
        let shown = v.image_want.as_ref().map(|(k, _)| k.clone()).unwrap();
        assert!(v.images.contains_key(&shown));

        // a cached one shown again: the newest of the kept (LRU, not FIFO)
        v.key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE));
        full(&mut v);
        let back = v.image_want.as_ref().map(|(k, _)| k.clone()).unwrap();
        assert_ne!(back, shown);
        assert_eq!(v.image_order.last(), Some(&back));
        assert_eq!(v.image_reads.pending(), 0, "from the cache");

        // back to the 1st file: dropped, a new read at once
        v.key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE));
        for _ in names {
            v.key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE));
        }
        full(&mut v);
        assert_eq!(v.image_reads.pending(), 1);

        let _ = std::fs::remove_dir_all(store::thread_dir(&t.id));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn sections_with_gaps_then_a_ranked_filter() {
        let _env = crate::engine::ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let d = std::env::temp_dir().join(format!("codehead-fview-{}", std::process::id()));
        let other = std::env::temp_dir().join(format!("codehead-fother-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("src")).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(d.join("src/view.rs"), "fn draw() {}").unwrap();
        std::fs::write(d.join("src/app.rs"), "// opens the view\nfn main() {}").unwrap();
        std::fs::write(other.join("scratch.txt"), "nothing").unwrap();
        let t = store::Thread::new("f", "t", &d, vec!["ada".into()]);
        std::fs::create_dir_all(store::thread_dir(&t.id)).unwrap();
        t.save().unwrap();
        for p in [d.join("src/view.rs"), d.join("src/app.rs"), other.join("scratch.txt")] {
            store::append(&t.id, &store::Event::new(Some("ada"), Kind::Tool { id: "1".into(), name: "Write".into(), input: serde_json::json!({"file_path": p}) })).unwrap();
        }
        let mut v = View::new(&t.id, &d);
        let (s, _) = screen(&mut v);
        let at = |s: &[String], x: &str| s.iter().position(|r| r.contains(x)).unwrap_or_else(|| panic!("{x}: {s:#?}"));
        let (repo, others) = (at(&s, "repo changes"), at(&s, "other files"));
        assert!(at(&s, "src/view.rs") > repo && at(&s, "src/app.rs") > repo && at(&s, "scratch.txt") > others, "{s:#?}");
        assert_eq!(s[others - 1], "", "a blank row between sections: {s:#?}");
        // ctrl+f: the filter; `view`: the name match first, the content match after, 0 headings
        v.key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL));
        assert!(v.typing());
        press(&mut v, "view");
        let (s, b) = screen(&mut v);
        let (a, c) = (at(&s, "src/view.rs"), at(&s, "src/app.rs"));
        assert!(a < c && !s.iter().any(|r| r.contains("repo changes") || r.contains("scratch")), "{s:#?}");
        // the marked chars: `view` of view.rs
        let x = s[a].find("view.rs").map(|i| s[a][..i].chars().count()).unwrap() as u16;
        assert!(b[(x, a as u16)].modifier.contains(ratatui::style::Modifier::UNDERLINED), "marks");
        // esc: the filter off, all files again
        v.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!v.typing());
        let (s, _) = screen(&mut v);
        at(&s, "scratch.txt");
        let _ = std::fs::remove_dir_all(store::thread_dir(&t.id));
        let _ = std::fs::remove_dir_all(&d);
        let _ = std::fs::remove_dir_all(&other);
    }
}
