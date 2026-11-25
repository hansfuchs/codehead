use super::{HashMap, Line, Span, Style, View, theme};
use std::fmt::Write as _;

impl View {
    /// The files in the model's order (its groups first, the rest after in path-rule order);
    /// a rank of another set of paths: a new call.
    pub(super) fn apply_rank(&mut self, force: bool) {
        apply_rank_view(self, force);
    }

    /// The model's group of PATH and the why of PATH, if the rank is current (as of `build`).
    pub(super) fn ranked(&self, path: &str) -> Option<(usize, &crate::domain::changes::rank::Group, &str)> {
        let rank = self.classifier.rank.as_ref().filter(|_| self.classifier.fresh_rank)?;

        rank.groups.iter().enumerate().find_map(|(index, group)| {
            group
                .files
                .iter()
                .find(|file| file.path == path)
                .map(|file| (index, group, file.why.as_str()))
        })
    }

    /// Level of each file (by index): the agent's (an older rank too), else path rules.
    pub(super) fn levels(&self) -> Vec<crate::domain::changes::rank::Level> {
        levels_view(self)
    }

    /// The cursor's file and its row in that file's rows (0: the head).
    pub(super) fn cursor_spot(&self) -> Option<(String, usize)> {
        let row = self.layout.rows.get(self.at)?;
        let head = self.layout.rows[..=self.at]
            .iter()
            .rposition(|above| above.head && above.file == row.file)?;

        Some((self.changes.files.get(row.file)?.path.clone(), self.at - head))
    }

    /// A finished rank call applies. True: the order changed.
    pub(super) fn poll_rank(&mut self) -> bool {
        poll_rank_view(self)
    }

    /// The row above the boxes: files, lines, base, the order's source.
    pub(super) fn summary(&self) -> Line<'static> {
        summary_view(self)
    }

    /// What the diff is against: `uncommitted`, `all changes against origin/main · esc: back`.
    pub(super) fn against(&self) -> String {
        if self.all {
            format!("all changes against {} · esc: back", self.changes.base)
        } else {
            "uncommitted".into()
        }
    }
}

/// ` · 2 staged · ↑1 ↓3 · git…`: the git side of the summary.
fn git_state(view: &View) -> String {
    let mut text = String::new();
    let staged = view.changes.staged.len();

    if staged > 0 {
        let _ = write!(text, " · {staged} staged");
    }

    if view.changes.ahead + view.changes.behind > 0 {
        let _ = write!(text, " · ↑{} ↓{}", view.changes.ahead, view.changes.behind);
    }

    if view.operation_state.op.is_some() {
        text.push_str(" · git…");
    }

    if let Some(pr) = &view.pull_request.pr {
        let _ = write!(text, " · PR #{} {}", pr.number, pr.state.to_lowercase());

        if pr.pass + pr.fail + pr.pending > 0 {
            let _ = write!(text, " ✓{} ✗{} ●{}", pr.pass, pr.fail, pr.pending);
        }
    }

    if view.reviewer.reviewing.is_some() {
        text.push_str(" · reviewing…");
    } else if let Some(error) = &view.reviewer.review_error {
        let _ = write!(text, " · {error}");
    } else if let Some(review) = view
        .reviewer
        .review
        .as_ref()
        .filter(|review| !review.findings.is_empty())
    {
        let _ = write!(text, " · {} findings ({})", review.findings.len(), review.model);
    }

    text
}

fn apply_rank_view(view: &mut View, force: bool) {
    let key = crate::domain::changes::rank::key(&view.changes.files);

    // a rank of an older codehead (0 levels): a new call too
    let fresh = view.classifier.rank.as_ref().is_some_and(|rank| {
        rank.key == key
            && rank
                .groups
                .iter()
                .flat_map(|group| &group.files)
                .any(|file| !file.level.is_empty())
    });

    if (force || !fresh)
        && view.classifier.ranking.is_none()
        && !view.changes.files.is_empty()
        && let Some(by) = view.classifier.classifier.clone()
    {
        let (tx, rx) = crate::infra::bus::channel();
        let (id, dir, files) = (view.id.clone(), view.dir.clone(), view.changes.files.clone());

        std::thread::spawn(move || {
            let _ = tx.send(crate::domain::changes::rank::run(&id, &dir, &files, &by));
        });

        view.classifier.ranking = Some(rx);
        view.classifier.rank_error = None;
    }

    let Some(rank) = view.classifier.rank.as_ref().filter(|rank| rank.key == key) else {
        return;
    };

    let pos: HashMap<&str, (usize, usize)> = rank
        .groups
        .iter()
        .enumerate()
        .flat_map(|(group, ranked)| {
            ranked
                .files
                .iter()
                .enumerate()
                .map(move |(index, file)| (file.path.as_str(), (group, index)))
        })
        .collect();

    let pos: HashMap<String, (usize, usize)> = pos.into_iter().map(|(path, place)| (path.to_string(), place)).collect();

    // a stable sort: unranked files keep the path-rule order, after the groups
    view.changes
        .files
        .sort_by_key(|file| pos.get(&file.path).copied().unwrap_or((usize::MAX, 0)));
    view.layout.built = None;
}

fn levels_view(view: &View) -> Vec<crate::domain::changes::rank::Level> {
    let by: HashMap<&str, crate::domain::changes::rank::Level> = view
        .classifier
        .rank
        .iter()
        .flat_map(|rank| &rank.groups)
        .flat_map(|group| &group.files)
        .filter_map(|file| {
            crate::domain::changes::rank::Level::parse(&file.level).map(|level| (file.path.as_str(), level))
        })
        .collect();

    view.changes
        .files
        .iter()
        .map(|file| {
            by.get(file.path.as_str())
                .copied()
                .unwrap_or_else(|| crate::domain::changes::rank::guess(file))
        })
        .collect()
}

fn poll_rank_view(view: &mut View) -> bool {
    let Some(rx) = &view.classifier.ranking else {
        return false;
    };

    let Ok(read) = rx.try_recv() else { return false };

    view.classifier.ranking = None;

    match read {
        Ok(rank) => {
            let cur = view.cursor_spot();

            view.classifier.rank = Some(rank);
            view.apply_rank(false);
            view.pending_cursor = cur;
        }
        Err(error) => view.classifier.rank_error = Some(error),
    }

    true
}

fn summary_view(view: &View) -> Line<'static> {
    let (add, del) = view
        .changes
        .files
        .iter()
        .fold((0, 0), |(added, deleted), file| (added + file.add, deleted + file.del));

    let (done, all) = view.progress();

    if let Some(commit) = &view.commit_src {
        let state = if commit.pushed { "pushed" } else { "unpushed" };

        return Line::from(vec![
            Span::styled(format!(" ◆ {} ", commit.short), theme::accent_bold()),
            Span::styled(
                format!("{} · ", crate::ui::thread_card::fit(&commit.subject, 60)),
                theme::text(),
            ),
            Span::styled(format!("{all} files · "), theme::muted()),
            Span::styled(format!("+{add}"), Style::default().fg(theme::pal().success())),
            Span::styled(format!(" −{del}"), Style::default().fg(theme::pal().error())),
            Span::styled(format!(" · {state} · read only · esc: back"), theme::muted()),
        ]);
    }

    let order = if view.classifier.ranking.is_some() {
        "classifying…".to_string()
    } else if let Some(error) = &view.classifier.rank_error {
        format!("path order · {error}")
    } else if let Some(rank) = view.classifier.rank.as_ref().filter(|_| view.classifier.fresh_rank) {
        format!("order: {}", rank.model)
    } else {
        "path order".to_string()
    };

    Line::from(vec![
        Span::styled(format!(" {all} files · "), theme::muted()),
        Span::styled(format!("+{add}"), Style::default().fg(theme::pal().success())),
        Span::styled(format!(" −{del}"), Style::default().fg(theme::pal().error())),
        Span::styled(
            format!(" · {done}/{all} reviewed · {} · {order}", view.against()),
            theme::muted(),
        ),
        Span::styled(git_state(view), theme::accent()),
    ])
}
