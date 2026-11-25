use super::{HashMap, Instant, Menu, Merge, MergeLayout, PathBuf, PrRef, Rect, Sources, Sub, Tab};

impl Merge {
    /// The merge tab of a thread: its PRs come from SOURCES (read in the background).
    /// The merge tab of thread ID: its PRs of the last read at once (a read follows).
    pub(crate) fn for_thread(id: &str) -> Merge {
        let prs = if cfg!(test) {
            Vec::new()
        } else {
            crate::screens::merge::pr_list::remembered(id)
        };

        let mut message = Merge::new(true, prs);

        message.thread_id = id.to_string();
        message
    }
    /// 1 PR (the start screen).
    pub(crate) fn one(pr: PrRef) -> Merge {
        Merge::new(false, vec![pr])
    }
    pub(super) fn new(thread: bool, prs: Vec<PrRef>) -> Merge {
        let (tx, rx) = crate::infra::bus::channel();

        Merge {
            thread,
            thread_id: String::new(),
            prs,
            cur: 0,
            tabs: HashMap::new(),
            tx,
            rx,
            sources: None,
            version: 0,
            fx: crate::ui::dissolve::Dissolve::default(),
            assoc_at: None,
            assoc_busy: false,
            assoc_error: None,
            active: Instant::now(),
            manual_at: None,
            calls: Vec::new(),
            labels: HashMap::new(),
            dirs: HashMap::new(),

            note: None,

            layout: MergeLayout {
                hits: Vec::new(),
                tab_hits: Vec::new(),
                sub_hits: Vec::new(),
                menu_hits: Vec::new(),
                area: Rect::default(),
                body_area: Rect::default(),
                edit_rect: Rect::default(),
            },
        }
    }
    /// The thread's sources changed (checkouts, log links): the PR list is read again when due.
    pub(crate) fn set_sources(&mut self, text: Sources) {
        let same = self
            .sources
            .as_ref()
            .is_some_and(|output| output.checkouts == text.checkouts && output.links == text.links);

        if !same {
            // new links: at once; new checkouts: at once
            self.assoc_at = None;
        }

        self.sources = Some(text);
    }
    /// The checkout DIR of REPO (the changes view's dir, the agents' hint).
    pub(crate) fn set_dir(&mut self, repo: &str, dir: PathBuf) {
        self.dirs.insert(repo.to_string(), dir);
    }
    /// Show PR (a tab of it: in front).
    pub(crate) fn show(&mut self, pr: &PrRef) {
        if !self.prs.contains(pr) {
            self.prs.insert(0, pr.clone());
        }

        self.cur = self.prs.iter().position(|path| path == pr).unwrap_or(0);
    }
    pub(crate) fn current(&self) -> Option<&PrRef> {
        self.prs.get(self.cur)
    }
    pub(super) fn tab(&mut self) -> Option<&mut Tab> {
        let pr = self.prs.get(self.cur)?.clone();

        Some(self.tabs.entry(pr).or_default())
    }
    pub(super) fn tab_ref(&self) -> Option<&Tab> {
        self.tabs.get(self.prs.get(self.cur)?)
    }
    /// The tab's label of the view bar: `merge 2`.
    pub(crate) fn count(&self) -> usize {
        self.prs.len()
    }
    /// Keys typed into an inline input / a dropdown filter.
    pub(crate) fn typing(&self) -> bool {
        self.tab_ref().is_some_and(|changes_value| {
            changes_value.input.edit.is_some()
                || matches!(changes_value.input.menu, Some(Menu::Labels { .. }))
                || (changes_value.sub == Sub::Changes
                    && changes_value
                        .changes
                        .as_ref()
                        .is_some_and(crate::features::changes::View::typing))
        })
    }
    /// Esc is the tab's: it closes an input, a dropdown, the checks list.
    pub(crate) fn takes_esc(&self) -> bool {
        self.tab_ref().is_some_and(|changes_value| {
            changes_value.input.edit.is_some()
                || changes_value.input.menu.is_some()
                || (changes_value.sub == Sub::Changes
                    && changes_value
                        .changes
                        .as_ref()
                        .is_some_and(crate::features::changes::View::takes_esc))
        })
    }
}
