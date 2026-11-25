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
        let count_value = match view {
            View::Active => self.cards.iter().filter(|character| !character.archived()).count(),
            View::Archived => self
                .cards
                .iter()
                .filter(|character_value| character_value.archived())
                .count(),
            View::Worktrees => self.extra[0].0,
            View::Claude => self.extra[1].0,
        };

        format!("{} {count_value}", view.label())
    }
    /// The repo tabs: the first repo (the thread in view's, else the start screen's), the other
    /// repos (most threads first, then A→Z), outside of git repos (when a row is), all.
    pub(crate) fn repo_tabs(&self) -> Vec<RepoTab> {
        repo_tabs_thread_list(self)
    }
    /// The repo tab in use: the pick, else the thread in view's repo (the menu), else all.
    pub(crate) fn repo_tab(&self) -> RepoTab {
        let tabs = self.repo_tabs();

        if let Some(path_value) = self.repo_pick.as_ref().filter(|path| tabs.contains(path)) {
            return path_value.clone();
        }

        if self.here.is_some()
            && let Some(thread) = tabs.first()
        {
            return thread.clone();
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
    pub(crate) fn step_repo(&mut self, directory: isize) {
        let tabs = self.repo_tabs();
        let cur = self.repo_tab();

        self.repos.i = tabs.iter().position(|thread| *thread == cur).unwrap_or(0);
        self.repos.step(directory, tabs.len());
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
    pub(crate) fn step_proj(&mut self, directory: isize) {
        let tabs = self.proj_tabs();
        let cur = self.proj_tab();

        self.projs.i = tabs.iter().position(|thread| *thread == cur).unwrap_or(0);
        self.projs.step(directory, tabs.len());
        self.proj_pick = tabs.get(self.projs.i).cloned().unwrap_or(ProjTab::All);
        self.cursor = 0;
        self.offset = 0;
    }
    pub(crate) fn step_view(&mut self, directory: isize) {
        let all = self.views_all();

        self.views.i = all.iter().position(|value| *value == self.view).unwrap_or(0);
        self.views.step(directory, all.len());
        self.set_view(all[self.views.i]);
    }
    pub(crate) fn set_view(&mut self, view_value: View) {
        if view_value != self.view {
            self.view = view_value;
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
        if let Some(path) = self.stat_pick {
            return path;
        }

        self.here
            .as_deref()
            .and_then(|id| {
                self.cards
                    .iter()
                    .find(|character| character.id == id && !character.archived())
            })
            .map_or(StatusTab::Open, |character_value| {
                StatusTab::of(Self::section(character_value))
            })
    }
    pub(crate) fn step_stat(&mut self, directory: isize) {
        self.stats.i = StatusTab::ALL
            .iter()
            .position(|thread| *thread == self.stat_tab())
            .unwrap_or(0);
        self.stats.step(directory, StatusTab::ALL.len());
        self.stat_pick = Some(StatusTab::ALL[self.stats.i]);
        self.cursor = 0;
        self.offset = 0;
    }
    /// Cards per status tab (filter, repo, project tabs applied).
    pub(super) fn stat_counts(&self) -> [usize; 4] {
        let mut count = [0; 4];

        for i in self.base() {
            count[Self::section(&self.cards[i]) as usize] += 1;
            count[3] += 1;
        }

        count
    }
}

fn repo_tabs_thread_list(state: &ThreadList) -> Vec<RepoTab> {
    let here = state
        .here
        .as_deref()
        .and_then(|id| state.cards.iter().find(|character| character.id == id))
        .and_then(|character_value| character_value.repo.clone())
        .or_else(|| state.home_repo.clone());
    let mut counts: Vec<(PathBuf, usize)> = Vec::new();
    let mut outside = false;
    let extra = state.extra.iter().flat_map(|error| error.1.iter().cloned());

    for result in state
        .cards
        .iter()
        .map(|character_2| character_2.repo.clone())
        .chain(extra)
    {
        match result {
            Some(path) => match counts.iter_mut().find(|(query, _)| *query == path) {
                Some(error_value) => error_value.1 += 1,
                None => counts.push((path, 1)),
            },
            None => outside = true,
        }
    }

    counts.sort_by(|agent, bytes| {
        (Some(&bytes.0) == here.as_ref())
            .cmp(&(Some(&agent.0) == here.as_ref()))
            .then(bytes.1.cmp(&agent.1))
            .then_with(|| repo_name(&agent.0).cmp(&repo_name(&bytes.0)))
    });

    let mut value: Vec<RepoTab> = counts
        .into_iter()
        .map(|(path_value, _)| RepoTab::Repo(path_value))
        .collect();

    if outside {
        value.push(RepoTab::Outside);
    }

    value.push(RepoTab::All);

    value
}

fn proj_tabs_thread_list(state: &ThreadList) -> Vec<ProjTab> {
    let tab = state.repo_tab();
    let archived = state.view == View::Archived;
    let mut counts: Vec<(String, usize)> = Vec::new();
    let mut none = false;

    for character_value in state
        .cards
        .iter()
        .filter(|character| character.archived() == archived && tab.keeps(character.repo.as_deref()))
    {
        none |= character_value.projects.is_empty();

        for path in &character_value.projects {
            match counts.iter_mut().find(|x| &x.0 == path) {
                Some(x) => x.1 += 1,
                None => counts.push((path.clone(), 1)),
            }
        }
    }

    counts.sort_by(|agent, bytes| bytes.1.cmp(&agent.1).then_with(|| agent.0.cmp(&bytes.0)));
    let mut value = vec![ProjTab::All];
    let any = !counts.is_empty();

    value.extend(counts.into_iter().map(|(path_value, _)| ProjTab::Project(path_value)));

    if none && any {
        value.push(ProjTab::None);
    }

    value
}
