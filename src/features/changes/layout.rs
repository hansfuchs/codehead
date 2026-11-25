use super::{HashMap, Painted, Rect, Row};

pub(super) struct Layout {
    /// Data version: up with each read that brings new diffs (they dissolve in).
    pub(super) version: u64,
    pub(super) fx: crate::ui::dissolve::Dissolve,
    /// The rows of the shown files (cheap: a row's line is made when it comes into view).
    pub(super) rows: Vec<Row>,
    pub(super) built: Option<(u16, bool)>,
    /// The width of the last build (a rebuild after `built = None` keeps it).
    pub(super) built_w: u16,
    /// Lines of the diff rows, per (file hash, hunk): syntax colors and word marks, made once.
    pub(super) paints: PaintCache,
    /// Width of the line numbers per file (by index), for the shown files.
    pub(super) num_w: HashMap<usize, usize>,
    /// The rows' area; the summary row and the filter row above it (0 height: too little room).
    pub(super) area: Rect,
    pub(super) summary_rect: Rect,
    pub(super) filter_rect: Rect,
    /// The row index on each screen row of the rows' area (pinned rows included).
    pub(super) shown_rows: Vec<usize>,
}

impl Layout {
    pub(super) fn new() -> Self {
        Self {
            version: 0,
            fx: crate::ui::dissolve::Dissolve::default(),
            rows: Vec::new(),
            built: None,
            built_w: 0,
            paints: std::cell::RefCell::default(),
            num_w: HashMap::new(),
            area: Rect::default(),
            summary_rect: Rect::default(),
            filter_rect: Rect::default(),
            shown_rows: Vec::new(),
        }
    }
}

type HunkPaint = std::rc::Rc<Vec<Painted>>;
type PaintCache = std::cell::RefCell<HashMap<(u64, usize), HunkPaint>>;
