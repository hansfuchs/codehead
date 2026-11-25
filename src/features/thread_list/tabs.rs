use super::{Mode, Path, PathBuf, ProjTab, RepoTab, StatusTab, ThreadList, View, repo_name};
#[cfg(test)]
use ratatui::layout::Rect;

impl ThreadList {
    pub(super) fn views_all(&self) -> Vec<View> {
        match self.mode {
            Mode::Switch => vec![View::Active, View::Archived],
            Mode::Manage => vec![View::Active, View::Archived, View::Worktrees, View::Claude],
        }
    }
    pub(super) fn view_label(&self, view: View) -> String {
        let count = match view {
            View::Active => self.cards.iter().filter(|card| !card.archived()).count(),
            View::Archived => self.cards.iter().filter(|card| card.archived()).count(),
            View::Worktrees => self.extra[0].0,
            View::Claude => self.extra[1].0,
        };

        format!("{} {count}", view.label())
    }
    /// The repo tabs: the first repo (the thread in view's, else the start screen's), the other
    /// repos (most threads first, then A→Z), outside of git repos (when a row is), all.
    pub(crate) fn repo_tabs(&self) -> Vec<RepoTab> {
        repo_tabs_thread_list(self)
    }
    /// The repo tab in use: the pick, else the thread in view's repo (the menu), else all.
    pub(crate) fn repo_tab(&self) -> RepoTab {
        let tabs = self.repo_tabs();

        if let Some(pick) = self.repo_pick.as_ref().filter(|pick| tabs.contains(pick)) {
            return pick.clone();
        }

        if self.here.is_some()
            && let Some(first) = tabs.first()
        {
            return first.clone();
        }

        RepoTab::All
    }
    /// Repo tab I's area in the last draw.
    #[cfg(test)]
    pub(crate) fn repo_rect(&self, i: usize) -> Option<Rect> {
        self.repos.rect_of(i)
    }
    /// A row of REPO passes the repo tab (the host's rows too).
    pub(crate) fn keeps(&self, repo: Option<&Path>) -> bool {
        self.repo_tab().keeps(repo)
    }
    pub(crate) fn step_repo(&mut self, delta: isize) {
        let tabs = self.repo_tabs();
        let cur = self.repo_tab();

        self.repos.i = tabs.iter().position(|tab| *tab == cur).unwrap_or(0);
        self.repos.step(delta, tabs.len());
        self.repo_pick = tabs.get(self.repos.i).cloned();
        self.cursor = 0;
        self.offset = 0;
    }
    /// The project tabs: all, the projects of the cards of the repo tab and view (most threads
    /// first, then A→Z), 0 projects (when a card has 0 and another has 1+).
    pub(crate) fn proj_tabs(&self) -> Vec<ProjTab> {
        proj_tabs_thread_list(self)
    }
    /// The project tab in use: the pick (while the repo tab has it), else all.
    pub(crate) fn proj_tab(&self) -> ProjTab {
        if self.proj_tabs().contains(&self.proj_pick) {
            self.proj_pick.clone()
        } else {
            ProjTab::All
        }
    }
    pub(crate) fn step_proj(&mut self, delta: isize) {
        let tabs = self.proj_tabs();
        let cur = self.proj_tab();

        self.projs.i = tabs.iter().position(|tab| *tab == cur).unwrap_or(0);
        self.projs.step(delta, tabs.len());
        self.proj_pick = tabs.get(self.projs.i).cloned().unwrap_or(ProjTab::All);
        self.cursor = 0;
        self.offset = 0;
    }
    pub(crate) fn step_view(&mut self, delta: isize) {
        let all = self.views_all();

        self.views.i = all.iter().position(|view| *view == self.view).unwrap_or(0);
        self.views.step(delta, all.len());
        self.set_view(all[self.views.i]);
    }
    pub(crate) fn set_view(&mut self, view: View) {
        if view != self.view {
            self.view = view;
            self.cursor = 0;
            self.offset = 0;
            self.sel.clear();
        }
    }
    /// The status tabs show (the active view only).
    pub(crate) fn has_stats(&self) -> bool {
        self.view == View::Active
    }
    /// The status tab in use: the pick, else the section of the thread in view, else open.
    pub(crate) fn stat_tab(&self) -> StatusTab {
        if let Some(pick) = self.stat_pick {
            return pick;
        }

        self.here
            .as_deref()
            .and_then(|id| self.cards.iter().find(|card| card.id == id && !card.archived()))
            .map_or(StatusTab::Open, |card| StatusTab::of(Self::section(card)))
    }
    pub(crate) fn step_stat(&mut self, delta: isize) {
        self.stats.i = StatusTab::ALL
            .iter()
            .position(|tab| *tab == self.stat_tab())
            .unwrap_or(0);
        self.stats.step(delta, StatusTab::ALL.len());
        self.stat_pick = Some(StatusTab::ALL[self.stats.i]);
        self.cursor = 0;
        self.offset = 0;
    }
    /// Cards per status tab (filter, repo, project tabs applied).
    pub(super) fn stat_counts(&self) -> [usize; 4] {
        let mut counts = [0; 4];

        for i in self.base() {
            counts[Self::section(&self.cards[i]) as usize] += 1;
            counts[3] += 1;
        }

        counts
    }
}

fn repo_tabs_thread_list(state: &ThreadList) -> Vec<RepoTab> {
    let here = state
        .here
        .as_deref()
        .and_then(|id| state.cards.iter().find(|card| card.id == id))
        .and_then(|card| card.repo.clone())
        .or_else(|| state.home_repo.clone());
    let mut counts: Vec<(PathBuf, usize)> = Vec::new();
    let mut outside = false;
    let extra = state.extra.iter().flat_map(|rows| rows.1.iter().cloned());

    for repo in state.cards.iter().map(|card| card.repo.clone()).chain(extra) {
        match repo {
            Some(path) => match counts.iter_mut().find(|(counted, _)| *counted == path) {
                Some(count) => count.1 += 1,
                None => counts.push((path, 1)),
            },
            None => outside = true,
        }
    }

    counts.sort_by(|left, right| {
        (Some(&right.0) == here.as_ref())
            .cmp(&(Some(&left.0) == here.as_ref()))
            .then(right.1.cmp(&left.1))
            .then_with(|| repo_name(&left.0).cmp(&repo_name(&right.0)))
    });

    let mut tabs: Vec<RepoTab> = counts.into_iter().map(|(path, _)| RepoTab::Repo(path)).collect();

    if outside {
        tabs.push(RepoTab::Outside);
    }

    tabs.push(RepoTab::All);

    tabs
}

fn proj_tabs_thread_list(state: &ThreadList) -> Vec<ProjTab> {
    let tab = state.repo_tab();
    let archived = state.view == View::Archived;
    let mut counts: Vec<(String, usize)> = Vec::new();
    let mut none = false;

    for card in state
        .cards
        .iter()
        .filter(|card| card.archived() == archived && tab.keeps(card.repo.as_deref()))
    {
        none |= card.projects.is_empty();

        for slug in &card.projects {
            match counts.iter_mut().find(|count| &count.0 == slug) {
                Some(count) => count.1 += 1,
                None => counts.push((slug.clone(), 1)),
            }
        }
    }

    counts.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    let mut tabs = vec![ProjTab::All];
    let any = !counts.is_empty();

    tabs.extend(counts.into_iter().map(|(slug, _)| ProjTab::Project(slug)));

    if none && any {
        tabs.push(ProjTab::None);
    }

    tabs
}
