//! The files view of a thread: the files its agents wrote or edited (their tool calls), and the
//! home repo's notes of the thread (`thread: <id>` on line 1), newest first; a preview right
//! (Markdown rendered, code in syntax colors). yazi (full screen) browses anything else.

use crate::ui::dissolve::Dissolve;
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
use crate::ui::graphics::Preview;
use crate::ui::theme;

/// An image preview: path, mtime, cols, rows.
type ImageKey = (PathBuf, u64, u16, u16);

/// Image encodes kept (back and forth between files: 0 new encode).
const MAX_IMAGES: usize = 4;
/// The cursor (or a resize) rests this long before an encode starts: fast j k over 20 photos
/// starts 1 encode, not 20.
const IMAGE_REST: Duration = if cfg!(test) {
    Duration::ZERO
} else {
    Duration::from_millis(80)
};

/// What a key asks the thread view to do.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Act {
    None,
    Toast(String),
    /// The IDE at line 1 of this file.
    Open(PathBuf),
    /// yazi with this file revealed.
    Yazi(PathBuf),
}

/// A section of the list: where a file sits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Sec {
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
    pub(crate) fn label(self) -> &'static str {
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

pub(crate) struct View {
    id: String,
    dir: PathBuf,
    pub files: Vec<Entry>,
    /// The files in the list, in order (index into `files`), with the marks of the filter.
    visible: Vec<(usize, Vec<usize>)>,
    /// Cursor: position in `visible`.
    at: usize,
    /// Files messages named (clicked links): they stay in the list.
    named: Vec<PathBuf>,
    list: Rect,
    /// The drawn rows of the list: (y, row).
    drawn: Vec<(u16, Row)>,
    preview: Rect,
    image_cache: ImageCacheState,
    text_preview: TextPreviewState,
    source_reads: SourceReadsState,
    share_task: ShareTaskState,
    filter_input: FilterInputState,
}

impl View {
    pub(crate) fn new(id: &str, dir: &Path) -> View {
        let mut filter = crate::ui::textarea::TextArea::new(true, "f: fuzzy search names and content");

        filter.style = Some(theme::text());

        // the work dir (agent files) and the home repo (notes): their changes read the list again
        let roots: Vec<PathBuf> = std::iter::once(dir.to_path_buf())
            .chain(crate::infra::config::home_repo())
            .filter(|result| result.is_dir())
            .collect();

        Self {
            id: id.to_string(),
            dir: dir.to_path_buf(),
            files: Vec::new(),
            visible: Vec::new(),
            at: 0,
            named: Vec::new(),
            list: Rect::default(),
            drawn: Vec::new(),
            preview: Rect::default(),
            image_cache: ImageCacheState {
                images: HashMap::new(),
                image_reads: crate::infra::background::Reads::default(),
                picker: crate::ui::graphics::picker,
                image_order: Vec::new(),
                image_want: None,
                image_kick: false,
            },
            text_preview: TextPreviewState {
                cache: HashMap::new(),
                texts: HashMap::new(),
                scroll: 0,
                renders: crate::infra::background::Reads::default(),
                preview_version: 0,
                preview_fx: Dissolve::default(),
            },
            source_reads: SourceReadsState {
                loaded: None,
                again: false,
                topics: crate::infra::bus::subscribe(),
                roots: roots
                    .iter()
                    .map(|result_value| crate::infra::watch::canon(result_value))
                    .collect(),
                _shown: roots
                    .iter()
                    .map(|result_2| crate::infra::watch::show(result_2))
                    .collect(),
                reads: crate::infra::background::Reads::default(),
                list_version: 0,
                list_fx: Dissolve::default(),
            },
            share_task: ShareTaskState {
                share_armed: None,
                shared: None,
            },
            filter_input: FilterInputState {
                filter,
                filtering: false,
            },
        }
    }

    /// A finished read of the list: in. True: the list changed.
    /// The S S share ended: its toast.
    pub(crate) fn take_toast(&mut self) -> Option<String> {
        let message = self.share_task.shared.as_ref()?.try_recv().ok()?;

        self.share_task.shared = None;
        Some(message)
    }

    pub(crate) fn count(&self) -> usize {
        self.files.len()
    }

    /// E's name in the list: an attachment's marker + file name (its paste dir says 0), else
    /// its path as shown.
    fn label(&self, error: &Entry) -> String {
        match &error.source {
            Source::Attached(message) => label_source_attached_message(error, message.as_ref()),
            _ => self.shown(&error.path),
        }
    }
}

fn label_source_attached_message(error: &Entry, message: Option<&String>) -> String {
    let name = error.path.file_name().map_or_else(
        || error.path.display().to_string(),
        |count| count.to_string_lossy().into_owned(),
    );

    // `[Pasted text #1 +40 lines]`: `[Pasted text #1]`
    message.as_ref().map_or(name.clone(), |message_value| {
        format!(
            "{}] {name}",
            message_value
                .split(" +")
                .next()
                .unwrap_or(message_value)
                .trim_end_matches(']')
        )
    })
}

#[cfg(test)]
#[path = "files_tests.rs"]
mod tests;

/// The list: sections with headings and gaps; the fuzzy filter (f) with ranked, marked matches.
#[cfg(test)]
#[path = "files_view_tests.rs"]
mod view_tests;

#[path = "files_draw.rs"]
mod draw;
#[path = "files_draw_helpers.rs"]
mod draw_helpers;
#[path = "files_key.rs"]
mod key;
#[path = "files_mouse.rs"]
mod mouse;
#[path = "files_poll.rs"]
mod poll;
#[path = "files_updates.rs"]
mod updates;

mod sources;
pub(crate) use sources::Entry;
pub(crate) use sources::Source;
#[cfg(test)]
pub(crate) use sources::agent_files;
pub(crate) use sources::load;
use sources::mtime;

mod selection;

mod preview;

mod context;

mod external;
pub(crate) use external::share;
pub(crate) use external::yazi_cmd;
pub(crate) use external::yazi_config;

struct ImageCacheState {
    /// Image + pdf previews, encoded for the terminal per (path, mtime, cols, rows): off the UI
    /// thread; the last `MAX_IMAGES` kept. Err: a line why 0 image.
    images: HashMap<ImageKey, Result<Preview, String>>,
    image_reads: crate::infra::background::Reads<ImageKey, Result<Preview, String>>,
    /// The terminal's image picker (tests: a fixed one).
    picker: fn() -> Option<crate::ui::graphics::Picker>,
    /// The cached encodes, oldest first.
    image_order: Vec<ImageKey>,
    /// The image the preview shows now, since when (an encode starts once it rests).
    image_want: Option<(ImageKey, Instant)>,
    /// A wanted image not polled yet: the 1st poll after its rest wakes a draw (1x, not per tick).
    image_kick: bool,
}
struct TextPreviewState {
    /// Preview: rows per (path, width, mtime), scroll.
    cache: HashMap<(PathBuf, u16, u64), Vec<Line<'static>>>,
    /// File text for the filter: per (path, mtime), the first 256 KiB.
    texts: HashMap<(PathBuf, u64), String>,
    scroll: usize,
    /// Preview renders (markdown, code highlight): off the UI thread.
    renders: crate::infra::background::Reads<(PathBuf, u16, u64), Vec<Line<'static>>>,
    preview_version: u64,
    preview_fx: crate::ui::dissolve::Dissolve,
}
struct SourceReadsState {
    loaded: Option<Instant>,
    /// A change came: 1 more read (after the one under way).
    again: bool,
    /// Bus topics: the thread's log, its work tree, the home repo → 1 read (0 timer).
    topics: std::sync::mpsc::Receiver<crate::infra::bus::Topic>,
    /// The work trees the list comes from (canonical), watched while the view lives.
    roots: Vec<PathBuf>,
    _shown: Vec<crate::infra::watch::Shown>,
    /// The list's read (the thread log, the home repo's notes): off the UI thread.
    reads: crate::infra::background::Reads<(), Vec<Entry>>,
    /// Data versions (a list read in, a preview in): their areas dissolve to the new rows.
    list_version: u64,
    list_fx: crate::ui::dissolve::Dissolve,
}
struct ShareTaskState {
    /// A 1st S (share: commit + push) at: a 2nd within 3 s shares.
    share_armed: Option<std::time::Instant>,
    /// S S: the share (git add, commit) off the UI thread; its toast.
    shared: Option<std::sync::mpsc::Receiver<String>>,
}
struct FilterInputState {
    /// Fuzzy filter (f): name and content; typing while focused.
    filter: crate::ui::textarea::TextArea,
    filtering: bool,
}
