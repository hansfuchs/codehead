//! The source control panel left of the review (ctrl+b): the commit message on top, then
//! sections as VS Code has them: staged, changes (by level), comments (by severity, then file), stash,
//! commits (unpushed, pushed; enter: the commit in the review's place, read only). Section
//! and level labels stick to the top / bottom edge while their rows are out of view; a click
//! on a stuck label scrolls to it.

use std::collections::{BTreeSet, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use unicode_width::UnicodeWidthStr;

use super::{Act, Modal, Op, Status, Then, View, patch};
use crate::rank::Level;
use crate::textarea::{Outcome, TextArea};
use crate::theme;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Sec {
    Staged,
    Changes,
    Comments,
    Stash,
    Commits,
}

impl Sec {
    fn label(self) -> &'static str {
        match self {
            Sec::Staged => "STAGED",
            Sec::Changes => "CHANGES",
            Sec::Comments => "COMMENTS",
            Sec::Stash => "STASH",
            Sec::Commits => "COMMITS",
        }
    }
}

/// A foldable label: a section, a level of the changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Fold {
    Sec(Sec),
    Level(Level),
    /// A severity of the comments.
    CLevel(Level),
    /// The commits' group: pushed (true) / unpushed.
    Push(bool),
    /// The comments' group: resolved (true) / open.
    CState(bool),
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum PRow {
    Section(Sec, usize),
    Level(Level, usize),
    /// A severity of the comments: its count.
    CLevel(Level, usize),
    /// File (index), in the staged section.
    File(usize, bool),
    /// A file of the comments: path, count.
    CommentFile(String, usize),
    Comment(usize),
    /// Open comments in another checkout: its root, their count (enter: the view goes there).
    Elsewhere(PathBuf, usize),
    Stash(usize),
    Empty(&'static str),
    /// PR mode: the commit picker.
    Pick,
    /// The commits' group (pushed: true): its count.
    Push(bool, usize),
    /// A commit (index in the read's commits).
    Commit(usize),
    /// The comments' group (resolved: true): its count.
    CState(bool, usize),
    /// The branch's changes against its base, as 1 view (commits + uncommitted).
    All,
}

impl PRow {
    fn label(&self) -> Option<Fold> {
        match self {
            PRow::Section(s, _) => Some(Fold::Sec(*s)),
            PRow::Level(l, _) => Some(Fold::Level(*l)),
            PRow::CLevel(l, _) => Some(Fold::CLevel(*l)),
            PRow::Push(p, _) => Some(Fold::Push(*p)),
            PRow::CState(r, _) => Some(Fold::CState(*r)),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Btn {
    Restore(String),
    Drop(String),
    Clear,
}

pub(super) struct Panel {
    pub shown: bool,
    /// The keys go to the panel (else the review).
    pub focus: bool,
    pub msg: TextArea,
    pub msg_focus: bool,
    drafting: Option<Receiver<Result<String, String>>>,
    rows: Vec<PRow>,
    at: usize,
    top: usize,
    folded: HashSet<Fold>,
    /// Picked files (space, shift+click): s / p take them.
    pub picked: BTreeSet<String>,
    /// Picked unpushed commits (space): u uncommits them, the ship menu pushes up to them.
    pub commit_picks: BTreeSet<String>,
    /// Picked comments (ids; space): the next prompt takes them along, then they clear.
    pub comment_picks: BTreeSet<String>,
    anchor: Option<usize>,
    pub rect: Rect,
    msg_rect: Rect,
    /// The list's screen rows: y, row index, a stuck label.
    lines: Vec<(u16, usize, bool)>,
    buttons: Vec<(Rect, Btn)>,
    /// PR mode: the commit dropdown open, its cursor (0: every commit, k: the k-th newest);
    /// its rows (clicks).
    pick: Option<usize>,
    pick_rows: Vec<(Rect, usize)>,
}

impl Panel {
    /// The file (index) of the panel's cursor row, with the keys on the panel.
    pub(super) fn file_at(&self) -> Option<usize> {
        match self.rows.get(self.at) {
            Some(PRow::File(f, _)) if self.focus => Some(*f),
            _ => None,
        }
    }

    /// PR mode: the commit dropdown is open.
    pub(super) fn picking(&self) -> bool {
        self.pick.is_some()
    }

    /// The cursor row is a commit.
    pub(super) fn on_commit(&self) -> bool {
        matches!(self.rows.get(self.at), Some(PRow::Commit(_)))
    }

    pub fn new() -> Panel {
        let mut msg = TextArea::new(false, "message");
        msg.style = Some(theme::panel());
        Panel {
            shown: true,
            focus: false,
            msg,
            msg_focus: false,
            drafting: None,
            rows: Vec::new(),
            at: 0,
            top: 0,
            folded: HashSet::new(),
            picked: BTreeSet::new(),
            commit_picks: BTreeSet::new(),
            comment_picks: BTreeSet::new(),
            anchor: None,
            rect: Rect::default(),
            msg_rect: Rect::default(),
            lines: Vec::new(),
            buttons: Vec::new(),
            pick: None,
            pick_rows: Vec::new(),
        }
    }
}

/// The color of level L.
pub(super) fn level_color(l: Level) -> Color {
    match l {
        Level::Critical => theme::pal().error(),
        // orange, yellow
        Level::High => theme::pal().caution(),
        Level::Medium => theme::pal().attention(),
        Level::Low => theme::pal().subtext0,
    }
}

/// `name` and `dir/` of PATH.
fn split_path(p: &str) -> (&str, &str) {
    match p.rsplit_once('/') {
        Some((d, n)) => (n, d),
        None => (p, ""),
    }
}

impl View {
    fn level_of(&self, i: usize) -> Level {
        self.lv.get(i).copied().unwrap_or(Level::Medium)
    }

    /// The comments of this view's checkout: (index, path in it); the open comments of other
    /// checkouts: (root, count), in order of the 1st comment.
    pub(super) fn comments_here(&self) -> (Vec<(usize, String)>, Vec<(PathBuf, usize)>) {
        let mut here = Vec::new();
        let mut away: Vec<(PathBuf, usize)> = Vec::new();
        for (i, c) in self.comments.iter().enumerate() {
            match super::place(c) {
                (Some(r), _) if !super::same_dir(&r, &self.dir) => {
                    if !c.open() {
                        continue;
                    }
                    match away.iter_mut().find(|(a, _)| *a == r) {
                        Some((_, n)) => *n += 1,
                        None => away.push((r, 1)),
                    }
                }
                (_, rel) => here.push((i, rel)),
            }
        }
        (here, away)
    }

    /// Comment CI's path in this view's checkout (None: another checkout's).
    pub(super) fn comment_path(&self, ci: usize) -> Option<String> {
        match super::place(self.comments.get(ci)?) {
            (Some(r), _) if !super::same_dir(&r, &self.dir) => None,
            (_, rel) => Some(rel),
        }
    }

    /// The panel's rows now.
    fn panel_rows(&self) -> Vec<PRow> {
        let p = &self.scm;
        let mut order: Vec<usize> = (0..self.changes.files.len()).collect();
        order.sort_by_key(|&i| self.level_of(i) as usize);
        let mut v = Vec::new();
        let pr = self.pr_src.is_some();
        if pr {
            v.push(PRow::Pick);
        }
        let staged: Vec<usize> = order.iter().copied().filter(|&i| self.changes.staged.contains_key(&self.changes.files[i].path)).collect();
        if !pr {
            v.push(PRow::Section(Sec::Staged, staged.len()));
        }
        if !pr && !p.folded.contains(&Fold::Sec(Sec::Staged)) {
            v.extend(staged.iter().map(|&i| PRow::File(i, true)));
        }
        let open: Vec<usize> = order.iter().copied().filter(|&i| self.changes.staged.get(&self.changes.files[i].path) != Some(&true)).collect();
        v.push(PRow::Section(Sec::Changes, open.len()));
        if !p.folded.contains(&Fold::Sec(Sec::Changes)) {
            for l in Level::ALL {
                let fs: Vec<usize> = open.iter().copied().filter(|&i| self.level_of(i) == l).collect();
                if fs.is_empty() {
                    continue;
                }
                v.push(PRow::Level(l, fs.len()));
                if !p.folded.contains(&Fold::Level(l)) {
                    v.extend(fs.iter().map(|&i| PRow::File(i, false)));
                }
            }
        }
        // the comments of this checkout (their path in it); the open ones of other checkouts:
        // 1 row per checkout
        let (here, away) = self.comments_here();
        let open = here.iter().filter(|(i, _)| self.comments[*i].open()).count();
        v.push(PRow::Section(Sec::Comments, open));
        if !p.folded.contains(&Fold::Sec(Sec::Comments)) {
            if here.is_empty() && away.is_empty() {
                v.push(PRow::Empty("c on lines: a comment"));
            }

            // OPEN (per severity, as the changes' levels: its files, their comments), RESOLVED
            // (its files, their comments)
            let with_files = |v: &mut Vec<PRow>, at: &[(usize, &str)]| {
                let mut paths: Vec<&str> = Vec::new();
                for &(_, path) in at {
                    if !paths.contains(&path) {
                        paths.push(path);
                    }
                }
                for path in paths {
                    let of: Vec<usize> = at.iter().filter(|(_, p)| *p == path).map(|(i, _)| *i).collect();
                    v.push(PRow::CommentFile(path.to_string(), of.len()));
                    v.extend(of.into_iter().map(PRow::Comment));
                }
            };
            for resolved in [false, true] {
                let at: Vec<(usize, &str)> = here.iter().filter(|(i, _)| self.comments[*i].open() != resolved).map(|(i, p)| (*i, p.as_str())).collect();
                if at.is_empty() {
                    continue;
                }
                v.push(PRow::CState(resolved, at.len()));
                if p.folded.contains(&Fold::CState(resolved)) {
                    continue;
                }

                if resolved {
                    with_files(&mut v, &at);
                    continue;
                }
                for l in Level::ALL {
                    let of: Vec<(usize, &str)> = at.iter().copied().filter(|(i, _)| self.comments[*i].level == l).collect();
                    if of.is_empty() {
                        continue;
                    }
                    v.push(PRow::CLevel(l, of.len()));
                    if !p.folded.contains(&Fold::CLevel(l)) {
                        with_files(&mut v, &of);
                    }
                }
            }
            v.extend(away.into_iter().map(|(root, n)| PRow::Elsewhere(root, n)));
        }
        if pr {
            return v;
        }
        v.push(PRow::Section(Sec::Stash, self.stash.len()));
        if !p.folded.contains(&Fold::Sec(Sec::Stash)) {
            if self.stash.is_empty() {
                v.push(PRow::Empty("p on lines / files: stash them"));
            }
            v.extend((0..self.stash.len()).map(PRow::Stash));
        }

        // the branch's commits: unpushed, then pushed (newest first in each)
        let cs = &self.changes.commits;
        v.push(PRow::Section(Sec::Commits, cs.list.len()));
        if p.folded.contains(&Fold::Sec(Sec::Commits)) {
            return v;
        }

        // a branch: its changes against the base (commits + uncommitted) as 1 view
        if self.changes.base != "HEAD" && !self.changes.base.is_empty() {
            v.push(PRow::All);
        }

        if cs.list.is_empty() {
            v.push(PRow::Empty("0 commits on the branch"));
        }
        for pushed in [false, true] {
            let at: Vec<usize> = (0..cs.list.len()).filter(|&i| cs.list[i].pushed == pushed).collect();
            if at.is_empty() {
                continue;
            }
            v.push(PRow::Push(pushed, at.len()));
            if !p.folded.contains(&Fold::Push(pushed)) {
                v.extend(at.into_iter().map(PRow::Commit));
            }
        }
        if cs.more {
            v.push(PRow::Empty("older commits: not listed"));
        }
        v
    }

    /// After a read: picks and the shown commit of commits that left the list go.
    pub(super) fn sync_commits(&mut self) {
        let cs = &self.changes.commits;
        self.scm.commit_picks.retain(|o| cs.find(o).is_some_and(|c| !c.pushed));
        if let Some(o) = self.peeked()
            && cs.find(o).is_none()
        {
            self.peek = None;
        }
    }

    /// The review shows commit I of the list (STAY: the keys stay on the panel).
    fn open_commit(&mut self, i: usize, stay: bool) {
        let Some(c) = self.changes.commits.list.get(i).cloned() else { return };
        if self.peeked() != Some(c.oid.as_str()) {
            let mut p = View::for_commit(&self.dir, &c);
            p.refresh(true);
            self.peek = Some(Box::new(p));
        }
        self.scm.focus = stay;
    }

    /// The review shows the branch's changes against its base (STAY: the keys stay on the panel).
    fn open_all(&mut self, stay: bool) {
        if !self.peek.as_ref().is_some_and(|p| p.all) {
            let mut p = View::for_branch(&self.id, &self.dir);
            p.refresh(true);
            self.peek = Some(Box::new(p));
        }
        self.scm.focus = stay;
    }

    /// The picked commits, else the cursor's commit: unpushed ones only (oids, list order).
    fn commit_targets(&self) -> Vec<String> {
        let cs = &self.changes.commits;
        let picked: Vec<String> = cs.list.iter().filter(|c| !c.pushed && self.scm.commit_picks.contains(&c.oid)).map(|c| c.oid.clone()).collect();
        if !picked.is_empty() {
            return picked;
        }
        match self.scm.rows.get(self.scm.at) {
            Some(PRow::Commit(i)) => cs.list.get(*i).filter(|c| !c.pushed).map(|c| vec![c.oid.clone()]).unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    /// `abc1234 subject` per commit (oids), list order, at most 8 lines.
    fn commit_lines(&self, oids: &[String]) -> String {
        let cs = &self.changes.commits;
        let mut v: Vec<String> = cs.list.iter().filter(|c| oids.contains(&c.oid)).map(|c| format!("{} {}", c.short, c.subject)).collect();
        if v.len() > 8 {
            let n = v.len() - 7;
            v.truncate(7);
            v.push(format!("… {n} more"));
        }
        v.join("\n")
    }

    /// u: the question, then the picked commits leave the history (their changes: uncommitted).
    fn ask_uncommit(&mut self) -> Act {
        if self.changes.stopped.is_some() {
            return Act::Toast("a rebase stopped: finish it first (S)".into());
        }

        let oids = self.commit_targets();
        if oids.is_empty() {
            let pushed = matches!(self.scm.rows.get(self.scm.at), Some(PRow::Commit(i)) if self.changes.commits.list.get(*i).is_some_and(|c| c.pushed));
            return Act::Toast(if pushed { "pushed: 0 uncommit".into() } else { "0 commits · space on a commit: pick it".into() });
        }

        let n = oids.len();
        let question = format!("uncommit {n} commit{}? files stay, changes become uncommitted\n{}", if n == 1 { "" } else { "s" }, self.commit_lines(&oids));
        self.modal = Some(Modal::Confirm { question, yes: false, then: Then::Uncommit(oids, self.changes.commits.head.clone()) });
        Act::None
    }

    /// The uncommit runs (asked): the messages of the commits go to the commit message box.
    pub(super) fn start_uncommit(&mut self, oids: Vec<String>, head: String) -> Act {
        let cs = &self.changes.commits;
        // oldest first, as they were made
        let msgs: Vec<String> = cs.list.iter().rev().filter(|c| oids.contains(&c.oid)).map(|c| c.body.clone()).collect();
        let op = Op::Uncommit { thread: self.id.clone(), head, picks: oids };
        let act = self.start_op(op);
        if self.op.is_some() {
            self.prefill = Some(msgs.join("\n\n"));
            self.scm.commit_picks.clear();
        }
        act
    }

    /// The newest picked unpushed commit, and how many unpushed commits a push up to it takes.
    pub(super) fn push_pick(&self) -> Option<(&super::commits::Commit, usize)> {
        let cs = &self.changes.commits;
        let un: Vec<&super::commits::Commit> = cs.list.iter().filter(|c| !c.pushed).collect();
        let k = un.iter().position(|c| self.scm.commit_picks.contains(&c.oid))?;
        Some((un[k], un.len() - k))
    }

    /// The ship menu's "push up to": the question lists every commit that goes.
    pub(super) fn ask_push_to(&mut self) -> Act {
        let Some((top, _)) = self.push_pick() else { return Act::Toast("0 commits picked".into()) };
        let oid = top.oid.clone();
        let cs = &self.changes.commits;
        let un: Vec<String> = cs.list.iter().filter(|c| !c.pushed).map(|c| c.oid.clone()).collect();
        let k = un.iter().position(|o| *o == oid).unwrap_or(0);
        let going = un[k..].to_vec();
        let to = cs.upstream.clone().unwrap_or_else(|| "origin".into());
        let n = going.len();

        // the newer unpushed commits (above the pick) stay local
        let mut question = format!("push {n} commit{} to {to}?", if n == 1 { "" } else { "s" });
        if k > 0 {
            question.push_str(&format!(" {k} newer stay local"));
        }

        question.push('\n');
        question.push_str(&self.commit_lines(&going));

        // the list stops at 50: older unpushed commits past it go too
        if cs.more && cs.list.last().is_some_and(|c| !c.pushed) {
            question.push_str("\n+ older unpushed commits (not listed)");
        }
        self.modal = Some(Modal::Confirm { question, yes: false, then: Then::PushTo(oid) });
        Act::None
    }

    /// The previous / next section label from the cursor (DOWN: next).
    fn jump_section(&mut self, down: bool) {
        let secs: Vec<usize> = (0..self.scm.rows.len()).filter(|&i| matches!(self.scm.rows[i], PRow::Section(..))).collect();
        let at = self.scm.at;
        let to = if down { secs.iter().copied().find(|&i| i > at) } else { secs.iter().rev().copied().find(|&i| i < at) };
        if let Some(i) = to {
            self.scm.at = i;
        }
    }

    /// The labels stuck to the top and to the bottom of a list of H rows from TOP.
    fn stuck(&self, top: usize, h: usize) -> (Vec<usize>, Vec<usize>) {
        let labels: Vec<usize> = (0..self.scm.rows.len()).filter(|&i| self.scm.rows[i].label().is_some()).collect();
        let (mut above, mut below): (Vec<usize>, Vec<usize>) = (Vec::new(), Vec::new());
        for _ in 0..labels.len() + 1 {
            let a: Vec<usize> = labels.iter().copied().filter(|&i| i < top + above.len()).collect();
            let end = (top + h).saturating_sub(below.len());
            // rows top + A .. END show in place; a label at or after END: the bottom stack
            let b: Vec<usize> = labels.iter().copied().filter(|&i| i >= end && i >= top + a.len()).collect();
            if a == above && b == below {
                break;
            }
            above = a;
            below = b;
            if above.len() + below.len() >= h {
                break;
            }
        }
        (above, below)
    }

    /// The cursor row in view (labels: always in view, stuck or not).
    fn panel_scroll(&mut self, h: usize) {
        let n = self.scm.rows.len();
        self.scm.at = self.scm.at.min(n.saturating_sub(1));
        self.scm.top = self.scm.top.min(n.saturating_sub(1));
        for _ in 0..4 {
            let (a, b) = self.stuck(self.scm.top, h);
            let at = self.scm.at;
            if self.scm.rows.get(at).is_some_and(|r| r.label().is_some()) && (a.contains(&at) || b.contains(&at)) {
                break;
            }
            let (lo, hi) = (self.scm.top + a.len(), (self.scm.top + h).saturating_sub(b.len()));
            if at < lo {
                self.scm.top = at.saturating_sub(a.len());
            } else if at >= hi {
                self.scm.top = (at + 1 + b.len()).saturating_sub(h);
            } else {
                break;
            }
        }
        // 0 empty room below the last row
        self.scm.top = self.scm.top.min(n.saturating_sub(h));
    }

    pub(super) fn draw_panel(&mut self, f: &mut Frame, area: Rect) {
        self.scm.rect = area;
        self.scm.buttons.clear();
        self.scm.lines.clear();
        if let Some(rx) = &self.scm.drafting
            && let Ok(r) = rx.try_recv()
        {
            self.scm.drafting = None;
            match r {
                Ok(m) => self.scm.msg.set_text(&m),
                Err(e) => self.note = Some(format!("draft: {e}")),
            }
        }
        if area.height < 6 || area.width < 12 {
            return;
        }
        if self.pr_src.is_some() {
            self.scm.msg_rect = Rect::default();
            self.draw_rows(f, area);
            self.draw_pick(f, area);
            return;
        }
        // the commit message box: 1-5 text rows, a hint row under it
        let inner_w = area.width.saturating_sub(2);
        let rows = (self.scm.msg.rows_for(inner_w) as u16).clamp(1, 5);
        let boxr = Rect { height: rows + 2, ..area };
        let edge = theme::border(self.scm.msg_focus);
        let n = self.changes.staged.len();
        let title = if self.scm.drafting.is_some() { " drafting… ".to_string() } else { format!(" commit · {n} staged ") };
        f.render_widget(Block::bordered().border_style(edge).title(Span::styled(title, theme::muted())), boxr);
        self.scm.msg_rect = Rect { x: boxr.x + 1, y: boxr.y + 1, width: inner_w, height: rows };
        let focused = self.scm.msg_focus && self.modal.is_none();
        self.scm.msg.render(f, self.scm.msg_rect, focused);
        let hint = Rect { y: boxr.bottom(), height: 1, ..area };
        f.render_widget(Paragraph::new(Line::styled(crate::nav::fit(" ctrl+⏎ commit · ctrl+g draft", area.width as usize), theme::muted())), hint);
        let list = Rect { y: hint.bottom() + 1, height: area.bottom().saturating_sub(hint.bottom() + 1), ..area };
        self.draw_rows(f, list);
    }

    /// PR mode: the commit picker's label (0: every commit).
    fn pick_label(&self, k: usize) -> (String, String) {
        let Some(src) = &self.pr_src else { return (String::new(), String::new()) };
        if k == 0 {
            return ("all commits".into(), format!("{} · vs {}", src.commits.len(), if src.base.is_empty() { "base" } else { &src.base }));
        }
        match src.commits.iter().rev().nth(k - 1) {
            Some(c) => (format!("{} {}", c.short, c.headline), crate::gh::stamp(c.date)),
            None => (String::new(), String::new()),
        }
    }

    /// PR mode: the picked commit as the picker's index.
    fn pick_index(&self) -> usize {
        let Some(src) = &self.pr_src else { return 0 };
        src.commit.as_ref().and_then(|c| src.commits.iter().rev().position(|x| &x.oid == c)).map_or(0, |p| p + 1)
    }

    /// PR mode: pick K of the picker (0: every commit).
    fn pick(&mut self, k: usize) {
        let oid = if k == 0 { None } else { self.pr_src.as_ref().and_then(|s| s.commits.iter().rev().nth(k - 1)).map(|c| c.oid.clone()) };
        self.scm.pick = None;
        self.pick_commit(oid);
    }

    /// PR mode: the open commit dropdown under the picker row.
    fn draw_pick(&mut self, f: &mut Frame, area: Rect) {
        self.scm.pick_rows.clear();
        let Some(cur) = self.scm.pick else { return };
        let n = self.pr_src.as_ref().map_or(0, |s| s.commits.len()) + 1;
        let rows: Vec<crate::dropdown::Row> = (0..n)
            .map(|k| {
                let (text, detail) = self.pick_label(k);
                crate::dropdown::Row { text, detail, now: k == self.pick_index(), marks: Vec::new() }
            })
            .collect();
        let anchor = Rect { y: area.y, height: 1, ..area };
        let w = area.width.max(50);
        let bounds = Rect { width: w, ..f.area() };
        let Some(r) = crate::dropdown::place(anchor, w, n, bounds, false) else { return };
        let (_, hits) = crate::dropdown::list(f, r, "commits", "enter: show · esc", &rows, cur, 0, "0 commits");
        self.scm.pick_rows = hits;
    }

    /// The panel's list in LIST.
    fn draw_rows(&mut self, f: &mut Frame, list: Rect) {
        self.scm.rows = self.panel_rows();
        let h = usize::from(list.height);
        if h == 0 {
            return;
        }
        self.panel_scroll(h);
        let (above, below) = self.stuck(self.scm.top, h);
        let mut shown: Vec<(usize, bool)> = above.iter().map(|&i| (i, true)).collect();
        let body = h.saturating_sub(above.len() + below.len());
        shown.extend((self.scm.top + above.len()..self.scm.rows.len()).take(body).map(|i| (i, false)));
        shown.truncate(h.saturating_sub(below.len()));
        shown.extend(below.iter().map(|&i| (i, true)));
        let w = usize::from(list.width);
        for (k, (i, stuck)) in shown.into_iter().enumerate() {
            let y = list.y + k as u16;
            let rect = Rect { y, height: 1, ..list };
            let row = self.scm.rows[i].clone();
            let line = self.panel_line(&row, w, rect);
            f.render_widget(Paragraph::new(line), rect);
            let buf = f.buffer_mut();
            if stuck {
                for x in rect.x..rect.right() {
                    buf[(x, y)].set_bg(theme::pal().surface0);
                }
            }
            let picked = match row {
                PRow::File(fi, _) => self.changes.files.get(fi).is_some_and(|x| self.scm.picked.contains(&x.path)),
                PRow::Commit(ci) => self.changes.commits.list.get(ci).is_some_and(|c| self.scm.commit_picks.contains(&c.oid)),
                PRow::Comment(ci) => self.comments.get(ci).is_some_and(|c| self.scm.comment_picks.contains(&c.id)),
                _ => false,
            };
            if picked {
                theme::picked(buf, rect, Some(rect.x), false);
            }
            if i == self.scm.at {
                let r = Rect { x: rect.x + 1, width: rect.width.saturating_sub(1), ..rect };
                if self.scm.focus { theme::cursor_row(buf, r) } else { theme::hover(buf, r) }
                if self.scm.focus {
                    buf[(rect.x, y)].set_symbol("▌").set_fg(theme::pal().accent);
                }
            }
            self.scm.lines.push((y, i, stuck));
        }
    }

    /// The line of panel row R in W columns (its buttons' click targets in RECT).
    fn panel_line(&mut self, r: &PRow, w: usize, rect: Rect) -> Line<'static> {
        let fold = |f: Fold| if self.scm.folded.contains(&f) { "▸" } else { "▾" };
        let right_fill = |left: &[Span<'static>], right: &[Span<'static>]| -> Line<'static> {
            let lw: usize = left.iter().map(|s| s.content.width()).sum();
            let rw: usize = right.iter().map(|s| s.content.width()).sum();
            let mut v = left.to_vec();
            v.push(Span::raw(" ".repeat(w.saturating_sub(lw + rw).max(1))));
            v.extend(right.iter().cloned());
            Line::from(v)
        };
        match r {
            PRow::Section(s, n) => {
                let left = vec![Span::styled(format!(" {} {}", fold(Fold::Sec(*s)), s.label()), theme::bold()), Span::styled(format!("  {n}"), theme::muted())];
                let mut right = Vec::new();
                if *s == Sec::Stash && *n > 0 {
                    let label = "✕ clear ";
                    let bw = label.width() as u16;
                    self.scm.buttons.push((Rect { x: rect.right().saturating_sub(bw), width: bw, ..rect }, Btn::Clear));
                    right.push(Span::styled(label, theme::muted()));
                }
                right_fill(&left, &right)
            }
            PRow::Level(l, n) => {
                let (done, all) = self.counts[*l as usize];
                let st = Style::default().fg(level_color(*l)).add_modifier(Modifier::BOLD);
                right_fill(&[Span::styled(format!("   {} {}", fold(Fold::Level(*l)), l.label()), st)], &[Span::styled(format!("{n} · {done}/{all} ✓ "), theme::muted())])
            }
            PRow::CLevel(l, n) => {
                let st = Style::default().fg(level_color(*l)).add_modifier(Modifier::BOLD);
                right_fill(&[Span::styled(format!("   {} {}", fold(Fold::CLevel(*l)), l.label()), st)], &[Span::styled(format!("{n} "), theme::muted())])
            }
            PRow::File(i, staged) => {
                let Some(f) = self.changes.files.get(*i) else { return Line::raw("") };
                let (letter, color) = self.letter(f, *staged);
                let reviewed = self.is_reviewed(f);
                let (name, dir) = split_path(&f.path);
                let indent = if *staged { "   " } else { "     " };
                let right = vec![Span::styled(if reviewed { "✓ " } else { "  " }, Style::default().fg(theme::pal().success())), Span::styled(format!("{letter} "), Style::default().fg(color).add_modifier(Modifier::BOLD))];
                let room = w.saturating_sub(indent.len() + 5);
                let name_st = if reviewed { theme::muted() } else { theme::text() };
                let name = crate::nav::fit(name, room);
                let dir_room = room.saturating_sub(name.width() + 1);
                let mut left = vec![Span::raw(indent), Span::styled(name, name_st)];
                if dir_room > 3 && !dir.is_empty() {
                    left.push(Span::styled(format!(" {}", crate::nav::fit(dir, dir_room)), theme::muted()));
                }
                right_fill(&left, &right)
            }
            PRow::CommentFile(path, n) => {
                let (name, dir) = split_path(path);
                let room = w.saturating_sub(8);
                let name = crate::nav::fit(name, room);
                let dir_room = room.saturating_sub(name.width() + 1);
                let mut left = vec![Span::raw("   "), Span::styled(name, theme::text())];
                if dir_room > 3 && !dir.is_empty() {
                    left.push(Span::styled(format!(" {}", crate::nav::fit(dir, dir_room)), theme::muted()));
                }
                right_fill(&left, &[Span::styled(format!("{n} "), theme::muted())])
            }
            PRow::CState(resolved, n) => {
                let (label, color) = if *resolved { ("RESOLVED", theme::pal().subtext0) } else { ("OPEN", theme::pal().progress()) };
                let st = Style::default().fg(color).add_modifier(Modifier::BOLD);
                right_fill(&[Span::styled(format!("   {} {label}", fold(Fold::CState(*resolved))), st)], &[Span::styled(format!("{n} "), theme::muted())])
            }
            // open: ✎ line, text, author right; resolved: ✓, muted, who resolved it
            PRow::Comment(ci) => {
                let Some(c) = self.comments.get(*ci) else { return Line::raw("") };
                let (mark, st) = if c.open() { ("✎", theme::fg(theme::pal().progress())) } else { ("✓", theme::muted()) };
                let who = if c.open() || c.resolved_by.is_empty() { c.author() } else { format!("{} ✓ @{}", c.author(), c.resolved_by) };
                // replies: ↳ count; unseen agent replies: ● count (accent)
                let new = super::threads::unseen(c);
                let replies = if c.replies.is_empty() { String::new() } else { format!("↳{} ", c.replies.len()) };
                let fresh = if new > 0 { format!("●{new} ") } else { String::new() };
                let right = vec![Span::styled(fresh.clone(), theme::accent_bold()), Span::styled(replies.clone(), theme::muted()), Span::styled(format!("{who} "), theme::muted())];
                let head = format!("     {mark} {}: ", c.at());
                let text = c.text.lines().next().unwrap_or_default();
                let room = w.saturating_sub(head.width() + who.width() + replies.width() + fresh.width() + 2);
                right_fill(&[Span::styled(head, st.add_modifier(Modifier::BOLD)), Span::styled(crate::nav::fit(text, room), st)], &right)
            }
            PRow::Stash(si) => {
                let Some(e) = self.stash.get(*si).cloned() else { return Line::raw("") };
                let mut right = Vec::new();
                if e.restoring {
                    right.push(Span::styled("restoring… ", theme::muted()));
                } else {
                    let (rl, dl) = ("↺ ", "✕ ");
                    let x0 = rect.right().saturating_sub(5);
                    self.scm.buttons.push((Rect { x: x0, width: 2, ..rect }, Btn::Restore(e.id.clone())));
                    self.scm.buttons.push((Rect { x: x0 + 3, width: 2, ..rect }, Btn::Drop(e.id.clone())));
                    right.push(Span::styled(rl, theme::accent()));
                    right.push(Span::raw(" "));
                    right.push(Span::styled(dl, theme::muted()));
                }
                let room = w.saturating_sub(5 + 5 + if e.restoring { 6 } else { 0 });
                let title = crate::nav::fit(&e.title(), room);
                let rest = room.saturating_sub(title.width() + 1);
                let mut left = vec![Span::styled("   ⧉ ", theme::muted()), Span::styled(title, if e.name.is_empty() { theme::muted().add_modifier(Modifier::ITALIC) } else { theme::text() })];
                if !e.name.is_empty() && rest > 4 {
                    left.push(Span::styled(format!(" {}", crate::nav::fit(&e.spot(), rest)), theme::muted()));
                }
                right_fill(&left, &right)
            }
            PRow::Empty(t) => Line::styled(crate::nav::fit(&format!("     {t}"), w), theme::muted().add_modifier(Modifier::ITALIC)),
            PRow::Elsewhere(root, n) => {
                let name = root.file_name().map_or_else(|| root.to_string_lossy().into_owned(), |n| n.to_string_lossy().into_owned());
                let text = format!("   ↗ {n} open in {name}");
                right_fill(&[Span::styled(crate::nav::fit(&text, w.saturating_sub(8)), theme::muted())], &[Span::styled("enter ", theme::muted())])
            }
            PRow::Push(pushed, n) => {
                let (label, color) = if *pushed { ("PUSHED", theme::pal().subtext0) } else { ("UNPUSHED", theme::pal().attention()) };
                let st = Style::default().fg(color).add_modifier(Modifier::BOLD);
                let cs = &self.changes.commits;
                let mut right = format!("{n} ");
                if *pushed && let Some(u) = &cs.upstream {
                    right = format!("{n} · {u} ");
                }
                if !*pushed && cs.remote_only > 0 {
                    right = format!("{n} · ↓{} on the remote ", cs.remote_only);
                }
                right_fill(&[Span::styled(format!("   {} {label}", fold(Fold::Push(*pushed))), st)], &[Span::styled(crate::nav::fit(&right, w / 2), theme::muted())])
            }
            PRow::All => {
                let on = self.peek.as_ref().is_some_and(|p| p.all);
                let (dot, st) = if on { ("◆", theme::accent()) } else { ("◇", theme::muted()) };
                let n = self.changes.commits.list.len();
                let left = vec![Span::styled(format!("   {dot} "), st), Span::styled("all changes", theme::text())];
                right_fill(&left, &[Span::styled(crate::nav::fit(&format!("{n} commits + uncommitted · vs {} ", self.changes.base), w.saturating_sub(16)), theme::muted())])
            }
            PRow::Commit(ci) => {
                let Some(c) = self.changes.commits.list.get(*ci) else { return Line::raw("") };
                let shown = self.peeked() == Some(c.oid.as_str());
                let (dot, dot_st) = match (shown, c.pushed) {
                    (true, _) => ("◆", theme::accent()),
                    (false, false) => ("●", theme::fg(theme::pal().attention())),
                    (false, true) => ("○", theme::muted()),
                };
                let age = crate::nav::ago(c.time.max(0) as u64 * 1000).replace(" ago", "");
                let right = vec![Span::styled(format!("{age} "), theme::muted())];
                let room = w.saturating_sub(5 + c.short.len() + 1 + age.width() + 2);
                let subject = crate::nav::fit(&c.subject, room);
                let sub_st = if c.pushed { theme::muted() } else { theme::text() };
                let left = vec![Span::raw("     "), Span::styled(format!("{dot} "), dot_st), Span::styled(format!("{} ", c.short), theme::muted()), Span::styled(subject, sub_st)];
                right_fill(&left, &right)
            }
            PRow::Pick => {
                let (text, detail) = self.pick_label(self.pick_index());
                let left = vec![Span::styled(" ◆ ", theme::accent()), Span::styled(crate::nav::fit(&text, w.saturating_sub(detail.width() + 8)), theme::bold())];
                right_fill(&left, &[Span::styled(format!("{detail} ▾ "), theme::muted())])
            }
        }
    }

    /// VS Code's 1-letter status of F: U untracked, A added, M modified, D deleted, R renamed.
    fn letter(&self, f: &super::FileDiff, staged: bool) -> (char, Color) {
        if !staged && self.changes.untracked.contains(&f.path) {
            return ('U', theme::pal().success());
        }
        match f.status {
            Status::New => ('A', theme::pal().success()),
            Status::Deleted => ('D', theme::pal().error()),
            Status::Renamed => ('R', theme::pal().renamed()),
            Status::Modified => ('M', theme::pal().info()),
        }
    }

    /// The review's cursor on file I's head (the filter clears when it hides the file).
    pub(super) fn jump_file(&mut self, i: usize) {
        // a commit in the review's place: the changes come back
        self.peek = None;
        if !self.rows.iter().any(|r| r.head && r.file == i) && !self.filter.is_empty() {
            self.filter.clear();
            self.built = None;
        }
        self.rebuild_here();
        if let Some(h) = self.rows.iter().position(|r| r.head && r.file == i) {
            self.at = h;
            self.top = h.saturating_sub(1);
        }
    }

    /// The review's cursor on comment CI (its box opens).
    pub(super) fn jump_comment(&mut self, ci: usize) {
        let Some(path) = self.comment_path(ci) else { return };
        let Some(i) = self.changes.files.iter().position(|f| f.path == path) else { return };
        if self.folded(&self.changes.files[i]) && !self.toggled.remove(&path) {
            self.toggled.insert(path);
        }
        self.built = None;
        self.jump_file(i);
        if let Some(r) = self.rows.iter().position(|r| r.comment == Some(ci)) {
            self.at = r;
            self.top = r.saturating_sub(3);
        }
    }

    /// The panel's picked files, else the cursor's file.
    fn targets(&self) -> Vec<String> {
        let known: Vec<String> = self.changes.files.iter().map(|f| f.path.clone()).filter(|p| self.scm.picked.contains(p)).collect();
        if !known.is_empty() {
            return known;
        }
        match self.scm.rows.get(self.scm.at) {
            Some(PRow::File(i, _)) => self.changes.files.get(*i).map(|f| vec![f.path.clone()]).unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    /// s: the staged section's rows unstage, the others stage.
    fn panel_stage(&mut self) -> Act {
        let ps = self.targets();
        if ps.is_empty() {
            return Act::Toast("0 files · space: pick files".into());
        }
        let unstage = matches!(self.scm.rows.get(self.scm.at), Some(PRow::File(_, true)));
        self.scm.picked.clear();
        self.start_op(if unstage { Op::UnstageFiles(ps) } else { Op::StageFiles(ps) })
    }

    /// p: whole files into the stash.
    fn panel_stash(&mut self) -> Act {
        let ps = self.targets();
        if ps.is_empty() {
            return Act::Toast("0 files · space: pick files".into());
        }
        self.scm.picked.clear();
        let items = ps.into_iter().map(|p| (p, String::new(), patch::Pick { all: true, ..Default::default() })).collect();
        self.stash_op(items)
    }

    /// Restore stash entry ID: its agent gets the patch.
    pub(super) fn restore(&mut self, id: &str) -> Act {
        self.stash = super::stash::read(&self.id);
        let Some(e) = self.stash.iter_mut().find(|e| e.id == id) else { return Act::None };
        e.restoring = true;
        let msg = super::stash::restore_ask(e);
        let who = e.agent.clone().unwrap_or_else(|| "the lead".into());
        super::stash::write(&self.id, &self.stash);
        self.stash_at = super::stash::stamp(&self.id);
        self.outbox.push(msg);
        Act::Toast(format!("restore → {who}"))
    }

    fn commit(&mut self) -> Act {
        let msg = self.scm.msg.text().trim().to_string();
        if msg.is_empty() {
            return Act::Toast("0 message · ctrl+g: draft one".into());
        }
        if !self.has_staged() {
            return Act::Toast("0 staged files · s: stage".into());
        }
        self.scm.msg.set_text("");
        self.scm.msg_focus = false;
        // the keys back to the review: P (push), O (PR) work at once
        self.scm.focus = false;
        self.scm.drafting = None;
        self.start_op(Op::Commit(msg))
    }

    fn draft(&mut self) -> Act {
        if self.scm.drafting.is_some() {
            return Act::None;
        }
        if !self.has_staged() {
            return Act::Toast("0 staged files: 0 draft".into());
        }
        let (tx, rx) = mpsc::channel();
        let dir = self.dir.clone();
        let model = crate::config::rank_model().unwrap_or_else(|| "haiku".into());
        std::thread::spawn(move || {
            let _ = tx.send(super::draft_message(&dir, &model));
        });
        self.scm.drafting = Some(rx);
        Act::Toast("drafting the commit message…".into())
    }

    /// m: the commit message takes the keys (the panel opens).
    pub(super) fn focus_message(&mut self) {
        self.scm.shown = true;
        self.scm.focus = true;
        self.scm.msg_focus = true;
    }

    /// A key for the commit message.
    fn msg_key(&mut self, k: KeyEvent) -> Act {
        let send = k.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER);
        match k.code {
            KeyCode::Esc => self.scm.msg_focus = false,
            KeyCode::Enter if send => return self.commit(),
            KeyCode::Char('g') if k.modifiers.contains(KeyModifiers::CONTROL) => return self.draft(),
            _ => {
                if !matches!(self.scm.msg.key(k), Outcome::Handled) && k.code == KeyCode::Enter {
                    self.scm.msg.newline();
                }
            }
        }
        Act::None
    }

    fn toggle_fold(&mut self, f: Fold, on: Option<bool>) {
        let now = self.scm.folded.contains(&f);
        if on.unwrap_or(!now) {
            self.scm.folded.insert(f);
        } else {
            self.scm.folded.remove(&f);
        }
        self.scm.rows = self.panel_rows();
    }

    /// The label row above panel row I.
    fn parent(&self, i: usize) -> Option<usize> {
        let mine = &self.scm.rows.get(i)?;
        let is_file_of_level = matches!(mine, PRow::File(_, false));
        let is_comment = matches!(mine, PRow::CommentFile(..) | PRow::Comment(_));
        let is_commit = matches!(mine, PRow::Commit(_));
        let is_clevel = matches!(mine, PRow::CLevel(..));
        (0..i).rev().find(|&j| match &self.scm.rows[j] {
            PRow::Level(..) => is_file_of_level,
            PRow::CLevel(..) => is_comment,
            PRow::CState(..) => is_comment || is_clevel,
            PRow::Push(..) => is_commit,
            PRow::Section(..) => true,
            _ => false,
        })
    }

    /// A key while the panel has the focus.
    pub(super) fn panel_key(&mut self, k: KeyEvent) -> Act {
        if self.scm.msg_focus {
            return self.msg_key(k);
        }
        // PR mode: the commit dropdown's keys while it is open
        if let Some(cur) = self.scm.pick {
            let n = self.pr_src.as_ref().map_or(0, |s| s.commits.len()) + 1;
            match k.code {
                KeyCode::Up | KeyCode::Char('k') => self.scm.pick = Some(cur.saturating_sub(1)),
                KeyCode::Down | KeyCode::Char('j') => self.scm.pick = Some((cur + 1).min(n - 1)),
                KeyCode::Home => self.scm.pick = Some(0),
                KeyCode::End => self.scm.pick = Some(n - 1),
                KeyCode::Enter | KeyCode::Right => self.pick(cur),
                KeyCode::Esc | KeyCode::Left => self.scm.pick = None,
                _ => {}
            }
            return Act::None;
        }
        self.scm.rows = self.panel_rows();
        let last = self.scm.rows.len().saturating_sub(1);
        let row = self.scm.rows.get(self.scm.at).cloned();
        match k.code {
            KeyCode::Up | KeyCode::Char('k') => self.scm.at = self.scm.at.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => self.scm.at = (self.scm.at + 1).min(last),
            KeyCode::PageUp => self.scm.at = self.scm.at.saturating_sub(10),
            KeyCode::PageDown => self.scm.at = (self.scm.at + 10).min(last),
            KeyCode::Home | KeyCode::Char('g') => self.scm.at = 0,
            KeyCode::End | KeyCode::Char('G') => self.scm.at = last,
            KeyCode::Left => match row.as_ref().and_then(PRow::label) {
                Some(f) => self.toggle_fold(f, Some(true)),
                None => {
                    if let Some(p) = self.parent(self.scm.at) {
                        self.scm.at = p;
                    }
                }
            },
            KeyCode::Enter | KeyCode::Right if row == Some(PRow::Pick) => self.scm.pick = Some(self.pick_index()),
            KeyCode::Right => match row {
                Some(r) if r.label().is_some() => {
                    let f = r.label().unwrap_or(Fold::Sec(Sec::Staged));
                    if self.scm.folded.contains(&f) {
                        self.toggle_fold(f, Some(false));
                    } else {
                        self.scm.at = (self.scm.at + 1).min(last);
                    }
                }
                Some(PRow::File(i, _)) => self.jump_file(i),
                Some(PRow::Comment(ci)) => self.jump_comment(ci),
                Some(PRow::Commit(ci)) => self.open_commit(ci, true),
                Some(PRow::All) => self.open_all(true),
                Some(PRow::Stash(si)) => {
                    if let Some(i) = self.stash.get(si).and_then(|e| self.changes.files.iter().position(|f| f.path == e.path)) {
                        self.jump_file(i);
                    }
                }
                _ => {}
            },
            KeyCode::Enter => match row {
                Some(r) if r.label().is_some() => self.toggle_fold(r.label().unwrap_or(Fold::Sec(Sec::Staged)), None),
                Some(PRow::File(i, _)) => {
                    self.jump_file(i);
                    self.scm.focus = false;
                }
                // enter on a comment: its whole thread at its line, the keys there
                Some(PRow::Comment(ci)) => {
                    if let Some(id) = self.comments.get(ci).map(|c| c.id.clone()) {
                        self.unfold_comment(&id);
                    }
                    self.jump_comment(ci);
                    self.scm.focus = false;
                }
                // enter on a commit: its files in the review, the keys there
                Some(PRow::Commit(ci)) => self.open_commit(ci, false),
                Some(PRow::All) => self.open_all(false),
                // enter on another checkout's comments: the view goes there
                Some(PRow::Elsewhere(root, _)) => return Act::Checkout(root),
                // enter on a stash entry: its agent restores it
                Some(PRow::Stash(si)) => {
                    if let Some(id) = self.stash.get(si).map(|e| e.id.clone()) {
                        return self.restore(&id);
                    }
                }
                _ => {}
            },
            KeyCode::Char(' ') => {
                if let Some(PRow::File(i, _)) = row
                    && let Some(p) = self.changes.files.get(i).map(|f| f.path.clone())
                    && !self.scm.picked.remove(&p)
                {
                    self.scm.picked.insert(p);
                }

                // a comment: picked for the next prompt
                if let Some(PRow::Comment(ci)) = row
                    && let Some(id) = self.comments.get(ci).map(|c| c.id.clone())
                    && !self.scm.comment_picks.remove(&id)
                {
                    self.scm.comment_picks.insert(id);
                }

                // a commit: unpushed ones only (a pushed one stays as it is)
                if let Some(PRow::Commit(i)) = row
                    && let Some(c) = self.changes.commits.list.get(i)
                {
                    if c.pushed {
                        return Act::Toast(format!("{}: pushed, read only", c.short));
                    }
                    if !self.scm.commit_picks.remove(&c.oid) {
                        self.scm.commit_picks.insert(c.oid.clone());
                    }
                }
                self.scm.at = (self.scm.at + 1).min(last);
            }
            KeyCode::Char('[') => self.jump_section(false),
            KeyCode::Char(']') => self.jump_section(true),
            KeyCode::Char('u') => return self.ask_uncommit(),
            // d on a comment: resolved / open again
            KeyCode::Char('d') => {
                if let Some(PRow::Comment(ci)) = row {
                    let act = self.toggle_resolved(ci);
                    self.scm.rows = self.panel_rows();
                    return act;
                }
            }
            KeyCode::Char('s') => return self.panel_stage(),
            KeyCode::Char('p') => return self.panel_stash(),
            // r: read again, as on every screen
            KeyCode::Char('r') => {
                self.refresh(true);
                return Act::Toast("changes: reading git".into());
            }
            KeyCode::Char('x') | KeyCode::Delete => match row {
                Some(PRow::Comment(ci)) => {
                    self.comments.remove(ci);
                    self.save_comments();
                    self.rebuild_here();
                    return Act::Toast(format!("comment deleted · {} left", self.comments.len()));
                }
                Some(PRow::Stash(si)) => {
                    if let Some(e) = self.stash.get(si) {
                        self.modal = Some(Modal::Confirm { question: format!("remove “{}” from the stash? its lines are gone for good", e.title()), yes: false, then: Then::DropStash(e.id.clone()) });
                    }
                }
                _ => {}
            },
            KeyCode::Char('X') if !self.stash.is_empty() => {
                self.modal = Some(Modal::Confirm { question: format!("clear the stash? {} entries, their lines gone for good", self.stash.len()), yes: false, then: Then::ClearStash });
            }
            KeyCode::Char('m') => self.scm.msg_focus = true,
            // S: the ship menu, as in the review
            KeyCode::Char('S') => return self.open_ship(),
            // e on a comment: edit it
            KeyCode::Char('e') if matches!(row, Some(PRow::Comment(_))) => {
                if let Some(PRow::Comment(ci)) = row {
                    self.edit_comment(ci, None);
                }
            }
            // o / e / i: the file in the IDE (its 1st changed line)
            KeyCode::Char('o') | KeyCode::Char('e') | KeyCode::Char('i') => {
                if let Some(PRow::File(i, _)) = row
                    && let Some(f) = self.changes.files.get(i)
                {
                    return Act::Open(f.path.clone(), f.first_line());
                }
            }
            KeyCode::Esc => {
                self.scm.picked.clear();
                self.scm.commit_picks.clear();
                self.scm.comment_picks.clear();
                self.built = None;
            }
            _ => {}
        }
        Act::None
    }

    /// A mouse event over the panel.
    pub(super) fn panel_mouse(&mut self, m: MouseEvent) -> Act {
        let at = Position { x: m.column, y: m.row };
        match m.kind {
            MouseEventKind::ScrollDown => {
                let n = self.scm.rows.len();
                self.scm.top = (self.scm.top + 3).min(n.saturating_sub(1));
                self.scm.at = self.scm.at.max(self.scm.top);
                return Act::None;
            }
            MouseEventKind::ScrollUp => {
                self.scm.top = self.scm.top.saturating_sub(3);
                return Act::None;
            }
            MouseEventKind::Down(MouseButton::Left) => {
                // PR mode: a click in the open commit dropdown picks, outside it closes it
                if self.scm.pick.is_some() {
                    if let Some(&(_, k)) = self.scm.pick_rows.iter().find(|(r, _)| r.contains(at)) {
                        self.pick(k);
                    } else {
                        self.scm.pick = None;
                    }
                    return Act::None;
                }
            }
            _ => {
                if self.scm.msg_rect.contains(at) || self.scm.msg_focus {
                    self.scm.msg.mouse(m);
                }
                return Act::None;
            }
        }
        self.scm.focus = true;
        if let Some((_, b)) = self.scm.buttons.iter().find(|(r, _)| r.contains(at)).cloned() {
            self.scm.msg_focus = false;
            match b {
                Btn::Restore(id) => return self.restore(&id),
                Btn::Drop(id) => {
                    let t = self.stash.iter().find(|e| e.id == id).map(|e| e.title()).unwrap_or_default();
                    self.modal = Some(Modal::Confirm { question: format!("remove “{t}” from the stash? its lines are gone for good"), yes: false, then: Then::DropStash(id) });
                }
                Btn::Clear => self.modal = Some(Modal::Confirm { question: format!("clear the stash? {} entries, their lines gone for good", self.stash.len()), yes: false, then: Then::ClearStash }),
            }
            return Act::None;
        }
        if (Rect { y: self.scm.msg_rect.y.saturating_sub(1), height: self.scm.msg_rect.height + 2, ..self.scm.rect }).contains(at) {
            self.scm.msg_focus = true;
            self.scm.msg.mouse(m);
            return Act::None;
        }
        self.scm.msg_focus = false;
        let Some(&(_, i, stuck)) = self.scm.lines.iter().find(|(y, ..)| *y == m.row) else { return Act::None };
        let row = self.scm.rows.get(i).cloned();
        if let Some(f) = row.as_ref().and_then(PRow::label) {
            self.scm.at = i;
            if stuck {
                // scroll to it: right under the labels above it
                let before = (0..i).filter(|&j| self.scm.rows[j].label().is_some()).count();
                self.scm.top = i.saturating_sub(before);
            } else {
                self.toggle_fold(f, None);
            }
            return Act::None;
        }
        let shift = m.modifiers.contains(KeyModifiers::SHIFT);
        match row {
            Some(PRow::File(fi, _)) => {
                if shift && let Some(a) = self.scm.anchor {
                    for j in a.min(i)..=a.max(i) {
                        if let Some(PRow::File(x, _)) = self.scm.rows.get(j)
                            && let Some(p) = self.changes.files.get(*x)
                        {
                            self.scm.picked.insert(p.path.clone());
                        }
                    }
                    self.scm.at = i;
                    return Act::None;
                }
                self.scm.at = i;
                self.scm.anchor = Some(i);
                self.jump_file(fi);
            }
            Some(PRow::Comment(ci)) => {
                self.scm.at = i;
                self.jump_comment(ci);
            }
            Some(PRow::All) => {
                self.scm.at = i;
                self.open_all(true);
            }
            // a click on a commit: its files in the review (shift+click: pick / unpick it)
            Some(PRow::Commit(ci)) => {
                self.scm.at = i;
                if shift && let Some(c) = self.changes.commits.list.get(ci).filter(|c| !c.pushed) {
                    let oid = c.oid.clone();
                    if !self.scm.commit_picks.remove(&oid) {
                        self.scm.commit_picks.insert(oid);
                    }
                } else {
                    self.open_commit(ci, true);
                }
            }
            Some(PRow::Pick) => {
                self.scm.at = i;
                self.scm.pick = Some(self.pick_index());
            }
            _ => self.scm.at = i,
        }
        Act::None
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests_support::*;
    use super::*;

    fn alt_tap(v: &mut super::super::View) {
        use ratatui::crossterm::event::{KeyEventKind, KeyEventState, ModifierKeyCode};
        for kind in [KeyEventKind::Press, KeyEventKind::Release] {
            v.key(KeyEvent::new_with_kind_and_state(KeyCode::Modifier(ModifierKeyCode::LeftAlt), KeyModifiers::ALT, kind, KeyEventState::NONE));
        }
    }

    #[test]
    fn an_alt_tap_toggles_the_focus_alt_with_a_key_does_not() {
        use ratatui::crossterm::event::{KeyEventKind, KeyEventState, ModifierKeyCode};
        let mut v = test_view();
        draw(&mut v, 120, 30);
        alt_tap(&mut v);
        assert!(v.scm.focus);
        alt_tap(&mut v);
        assert!(!v.scm.focus);
        let alt = |kind| KeyEvent::new_with_kind_and_state(KeyCode::Modifier(ModifierKeyCode::LeftAlt), KeyModifiers::ALT, kind, KeyEventState::NONE);
        v.key(alt(KeyEventKind::Press));
        v.key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::ALT));
        v.key(alt(KeyEventKind::Release));
        assert!(!v.scm.focus, "alt+x: 0 tap");
    }

    #[test]
    fn the_panel_lists_sections_levels_and_letters() {
        let mut v = test_view();
        v.changes.untracked.insert("tests/a_test.rs".into());
        v.changes.staged.insert("src/a.rs".into(), false);
        let s = draw(&mut v, 120, 30);
        let all = s.join("\n");
        assert!(all.contains("▾ STAGED  1") && all.contains("▾ CHANGES  4") && all.contains("▾ COMMENTS  0") && all.contains("▾ STASH  0"), "{all}");
        assert!(all.contains("▾ medium") && all.contains("▾ low"), "{all}");
        let staged = s.iter().position(|l| l.contains("STAGED")).unwrap();
        assert!(s[staged + 1].contains("a.rs") && s[staged + 1].contains("M"), "{all}");
        let panel = |l: &String| l.chars().take(30).collect::<String>();
        assert!(s.iter().any(|l| panel(l).contains("a_test.rs") && panel(l).trim_end().ends_with('U')), "untracked: U: {all}");
        // an alt tap: the keys to the panel; left on a label folds it
        alt_tap(&mut v);
        assert!(v.scm.focus);
        v.scm.at = v.scm.rows.iter().position(|r| *r == PRow::Section(Sec::Changes, 4)).unwrap();
        v.key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        let all = draw(&mut v, 120, 30).join("\n");
        assert!(all.contains("▸ CHANGES") && !all.contains("▾ medium"), "{all}");
        // ctrl+b hides it
        v.key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL));
        let all = draw(&mut v, 120, 30).join("\n");
        assert!(!all.contains("STAGED") && !v.scm.focus, "{all}");
    }

    #[test]
    fn s_opens_the_ship_menu_outward_steps_ask_with_a_2nd_enter() {
        let mut v = test_view();
        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        v.key(KeyEvent::new(KeyCode::Char('S'), KeyModifiers::SHIFT));
        let all = draw(&mut v, 120, 30).join("\n");
        assert!(all.contains(" ship ") && all.contains("push") && all.contains("open the pull request"), "{all}");

        // push: a 1st enter asks (0 git step), esc closes
        assert_eq!(v.key(enter), Act::None);
        assert!(v.op.is_none() && draw(&mut v, 120, 30).join("\n").contains("enter again: push"));
        v.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(v.modal.is_none() && v.op.is_none());

        // a stopped rebase: its steps only; the agents resolve at once, abort asks
        v.changes.stopped = Some(vec!["src/a.rs".into()]);
        v.key(KeyEvent::new(KeyCode::Char('S'), KeyModifiers::SHIFT));
        let all = draw(&mut v, 120, 30).join("\n");
        assert!(all.contains("resolve the conflicts") && !all.contains("open the pull request"), "{all}");
        assert!(matches!(v.key(enter), Act::Send(t) if t.contains("src/a.rs")));

        v.key(KeyEvent::new(KeyCode::Char('S'), KeyModifiers::SHIFT));
        v.key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        v.key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        assert_eq!(v.key(enter), Act::None);
        assert!(v.op.is_none(), "abort: a 1st enter only asks");
    }

    #[test]
    fn ctrl_b_cycles_the_panel_and_letters_with_a_modifier_do_nothing() {
        let mut v = test_view();
        draw(&mut v, 120, 30);
        assert!(v.scm.shown && !v.scm.focus);

        // alt+P, ctrl+s: 0 push, 0 stage (only the plain letters act)
        assert_eq!(v.key(KeyEvent::new(KeyCode::Char('P'), KeyModifiers::ALT | KeyModifiers::SHIFT)), Act::None);
        assert_eq!(v.key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)), Act::None);
        assert!(v.op.is_none());

        // ctrl+b: shown → the keys to the panel → hidden → shown with the keys (0 kitty needed)
        let ctrl_b = KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL);
        v.key(ctrl_b);
        assert!(v.scm.shown && v.scm.focus);

        v.key(ctrl_b);
        assert!(!v.scm.shown && !v.scm.focus);

        v.key(ctrl_b);
        assert!(v.scm.shown && v.scm.focus);
    }

    #[test]
    fn space_picks_files_and_right_jumps_to_one() {
        let mut v = test_view();
        draw(&mut v, 120, 30);
        alt_tap(&mut v);
        let first = v.scm.rows.iter().position(|r| matches!(r, PRow::File(_, false))).unwrap();
        v.scm.at = first;
        v.key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
        assert_eq!(v.scm.picked.len(), 1);
        assert_eq!(v.scm.at, first + 1);
        v.key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        let PRow::File(i, _) = v.scm.rows[first + 1] else { panic!() };
        assert!(v.rows[v.at].head && v.rows[v.at].file == i);
    }

    /// A view of fixture repo W (read now), the panel with the keys.
    fn commit_view(w: &std::path::Path, id: &str) -> View {
        let mut v = View::new(id, w);
        v.marks.clear();
        v.comments.clear();
        v.stash.clear();
        v.changes = super::super::load(w).unwrap();
        v.loaded = Some(std::time::Instant::now());
        v.scm.focus = true;
        v
    }

    fn press(v: &mut View, c: KeyCode) -> Act {
        v.key(KeyEvent::new(c, KeyModifiers::NONE))
    }

    /// The panel's cursor on the commit with SUBJECT.
    fn on_commit(v: &mut View, subject: &str) {
        draw(v, 140, 40);
        v.scm.at = v.scm.rows.iter().position(|r| matches!(r, PRow::Commit(i) if v.changes.commits.list[*i].subject == subject)).unwrap();
    }

    /// The git step on its way ends: its note.
    fn wait_op(v: &mut View) -> String {
        for _ in 0..300 {
            if v.poll_op() {
                return v.take_note().unwrap_or_default();
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        panic!("git step: 0 end");
    }

    /// The read on its way ends.
    fn wait_read(v: &mut View) {
        for _ in 0..300 {
            if v.loading.is_none() || v.poll() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    #[test]
    fn the_commits_section_lists_unpushed_then_pushed_space_picks_brackets_jump() {
        use super::super::commits::fixture::*;
        let w = repo("panel");
        sh(&w, "echo draft > x.txt");
        let mut v = commit_view(&w, "commits-panel-1");

        let all = draw(&mut v, 140, 40).join("\n");

        assert!(all.contains("▾ COMMITS  4") && all.contains("▾ UNPUSHED") && all.contains("▾ PUSHED"), "{all}");
        let s = draw(&mut v, 140, 40);
        let row = |pat: &str| s.iter().position(|l| l.contains(pat)).unwrap_or_else(|| panic!("{pat}"));
        assert!(row("UNPUSHED") < row("c4 subject") && row("c4 subject") < row("c3 subject") && row("c3 subject") < row("▾ PUSHED") && row("▾ PUSHED") < row("c2 subject"));
        assert!(s[row("c4 subject")].contains("● ") && s[row("c2 subject")].contains("○ "), "{all}");

        // space: an unpushed commit picked, a pushed one refused
        on_commit(&mut v, "c4 subject");
        press(&mut v, KeyCode::Char(' '));
        assert_eq!(v.scm.commit_picks.len(), 1);
        assert!(v.hints().contains("u: uncommit"), "the cursor on c3: commit keys");

        on_commit(&mut v, "c2 subject");
        assert!(matches!(press(&mut v, KeyCode::Char(' ')), Act::Toast(t) if t.contains("pushed")));
        assert_eq!(v.scm.commit_picks.len(), 1);

        // [ ]: section labels; esc: 0 picks
        press(&mut v, KeyCode::Char('['));
        assert_eq!(v.scm.rows[v.scm.at], PRow::Section(Sec::Commits, 4));
        press(&mut v, KeyCode::Char('['));
        assert_eq!(v.scm.rows[v.scm.at], PRow::Section(Sec::Stash, 0));
        press(&mut v, KeyCode::Char(']'));
        assert_eq!(v.scm.rows[v.scm.at], PRow::Section(Sec::Commits, 4));
        assert!(v.takes_esc());
        press(&mut v, KeyCode::Esc);
        assert!(v.scm.commit_picks.is_empty());
        drop(&w);
    }

    #[test]
    fn enter_on_a_commit_shows_its_files_read_only_esc_brings_the_changes_back() {
        use super::super::commits::fixture::*;
        let w = repo("peek");
        sh(&w, "echo draft > x.txt");
        let mut v = commit_view(&w, "commits-peek-1");
        on_commit(&mut v, "c3 subject");

        press(&mut v, KeyCode::Enter);
        if let Some(p) = v.peek.as_mut() {
            wait_read(p);
        }
        let s = draw(&mut v, 140, 40);
        let all = s.join("\n");
        let review: String = s.iter().map(|l| l.chars().skip(36).collect::<String>()).collect::<Vec<_>>().join("\n");

        assert!(!v.scm.focus, "the keys to the review");
        assert!(review.contains("◆ ") && review.contains("c3 subject") && review.contains("read only"), "{all}");
        assert!(review.contains("c3.txt") && !review.contains("x.txt"), "the commit's files only: {all}");
        assert!(v.hints().contains("read only"));

        // c, v, s, space: refused; 0 comments / marks
        for c in ['c', 'v', 's', ' ', 'a'] {
            assert!(matches!(press(&mut v, KeyCode::Char(c)), Act::Toast(t) if t.contains("read only")), "{c}");
        }
        assert!(v.peek.as_ref().is_some_and(|p| p.comments.is_empty() && p.marks.is_empty() && p.modal.is_none()));

        // j k move in the commit's diff; esc: the changes back
        press(&mut v, KeyCode::Char('j'));
        assert!(v.takes_esc());
        press(&mut v, KeyCode::Esc);
        assert!(v.peek.is_none());
        let all = draw(&mut v, 140, 40).join("\n");
        assert!(all.contains("x.txt") && !all.contains("read only"), "{all}");

        // → on a commit: shown, the keys stay on the panel
        on_commit(&mut v, "c2 subject");
        v.scm.focus = true;
        press(&mut v, KeyCode::Right);
        assert!(v.peek.is_some() && v.scm.focus);
        assert!(v.peeked() == Some(v.changes.commits.list[2].oid.as_str()));
        drop(&w);
    }

    #[test]
    fn u_asks_then_uncommits_the_picked_commit_its_message_goes_to_the_box() {
        use super::super::commits::fixture::*;
        let w = repo("uncommit");
        let mut v = commit_view(&w, "commits-uncommit-1");
        on_commit(&mut v, "c4 subject");
        let before = oid(&w, "HEAD");

        press(&mut v, KeyCode::Char('u'));
        let all = draw(&mut v, 140, 40).join("\n");

        assert!(all.contains("uncommit 1 commit? files stay") && all.contains("c4 subject"), "{all}");
        assert!(v.op.is_none(), "a question first");

        // n: 0 uncommit; u again, y: it runs
        press(&mut v, KeyCode::Char('n'));
        assert!(v.modal.is_none() && v.op.is_none());
        assert_eq!(oid(&w, "HEAD"), before);

        press(&mut v, KeyCode::Char('u'));
        press(&mut v, KeyCode::Char('y'));
        let note = wait_op(&mut v);

        assert!(note.contains("uncommitted 1 commit"), "{note}");
        assert_eq!(oid(&w, "HEAD"), oid(&w, &format!("{before}^")));
        assert!(v.scm.msg.text().contains("c4 subject") && v.scm.msg.text().contains("c4 body"));

        // a pushed commit: 0 question
        wait_read(&mut v);
        on_commit(&mut v, "c2 subject");
        assert!(matches!(press(&mut v, KeyCode::Char('u')), Act::Toast(t) if t.contains("pushed")));
        assert!(v.modal.is_none());
        drop(&w);
    }

    #[test]
    fn the_ship_menu_pushes_up_to_the_picked_commit_after_a_question() {
        use super::super::commits::fixture::*;
        let w = repo("shipto");
        let mut v = commit_view(&w, "commits-shipto-1");
        let c3 = oid(&w, "HEAD^");

        // 0 picks: the ship menu as before
        press(&mut v, KeyCode::Char('S'));
        assert!(!draw(&mut v, 140, 40).join("\n").contains("push up to"));
        press(&mut v, KeyCode::Esc);

        on_commit(&mut v, "c3 subject");
        v.scm.focus = true;
        press(&mut v, KeyCode::Char(' '));
        press(&mut v, KeyCode::Char('S'));
        let all = draw(&mut v, 140, 40).join("\n");
        assert!(all.contains("push up to") && all.contains("1 commit"), "{all}");

        // the row: the question lists the commits, 0 push yet
        press(&mut v, KeyCode::Char('j'));
        press(&mut v, KeyCode::Enter);
        let all = draw(&mut v, 140, 40).join("\n");
        assert!(all.contains("push 1 commit to origin/feat?") && all.contains("1 newer stay local") && all.contains("c3 subject"), "{all}");
        assert!(v.op.is_none());

        press(&mut v, KeyCode::Char('y'));
        let note = wait_op(&mut v);

        assert!(note.contains("pushed up to"), "{note}");
        assert_eq!(oid(&w, "origin/feat"), c3);
        drop(&w);
    }

    #[test]
    fn comments_split_into_open_and_resolved_space_picks_d_resolves() {
        let mut v = test_view();
        v.id = format!("changes-cstate-{}", std::process::id());
        let c = |id: &str, by: &str, line: u32| super::super::Comment { path: "src/a.rs".into(), line, text: format!("{id} text"), id: id.into(), by: by.into(), ..Default::default() };
        v.comments = vec![c("c1", "", 11), c("c2", "REGI", 12)];
        v.comments[1].state = super::super::CState::Resolved;
        v.comments[1].resolved_by = "REGI".into();

        let all = draw(&mut v, 140, 40).join("\n");

        assert!(all.contains("▾ COMMENTS  1") && all.contains("▾ OPEN") && all.contains("▾ RESOLVED"), "{all}");
        assert!(all.contains("c1 text") && all.contains("you"), "author: you: {all}");
        assert!(all.contains("@REGI ✓ @REGI"), "resolved: author, who resolved it: {all}");
        let review: String = all.lines().map(|l| l.chars().skip(36).collect::<String>()).collect::<Vec<_>>().join("\n");
        assert!(review.contains("c1 text") && !review.contains("c2 text"), "resolved: out of the file view: {review}");

        // the panel: space picks c1 for the prompt, d resolves it, d again opens it
        alt_tap(&mut v);
        v.scm.at = v.scm.rows.iter().position(|r| *r == PRow::Comment(0)).unwrap();
        v.key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
        assert!(v.scm.comment_picks.contains("c1") && v.context().unwrap().contains("c1 [medium, open]"));

        v.scm.at = v.scm.rows.iter().position(|r| *r == PRow::Comment(0)).unwrap();
        v.key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
        assert!(!v.comments[0].open());
        let all = draw(&mut v, 140, 40).join("\n");
        assert!(all.contains("▾ COMMENTS  0") && !all.contains("▾ OPEN"), "{all}");

        v.scm.at = v.scm.rows.iter().position(|r| *r == PRow::Comment(0)).unwrap();
        v.key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
        assert!(v.comments[0].open());
        let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir(&v.id));
    }

    #[test]
    fn comments_resolve_on_0_uncommitted_changes_and_list_per_checkout() {
        use super::super::commits::fixture;
        let w = fixture::repo("comment-checkouts");
        let wt = w.parent().unwrap().join("wt");
        fixture::sh(&w, "git worktree add -q ../wt && echo dirty >> x.txt");
        fixture::sh(&wt, "echo dirty >> x.txt");
        let root = w.to_string_lossy().into_owned();
        let c = |id: &str, path: String, root: &str| super::super::Comment { path, line: 1, text: format!("{id} text"), id: id.into(), root: root.into(), ..Default::default() };
        let mut all = vec![
            c("c1", "x.txt".into(), &root),
            c("c2", "y.txt".into(), &root),
            // an older comment: absolute path in the linked worktree, 1 relative one
            c("c3", wt.join("x.txt").to_string_lossy().into_owned(), ""),
            c("c4", wt.join("y.txt").to_string_lossy().into_owned(), ""),
            c("c5", "c1.txt".into(), ""),
        ];

        // dirty x.txt (both checkouts): open; clean files: resolved (c5: in the work dir)
        let ids = super::super::committed(&all, &w);
        assert_eq!(ids, ["c2", "c4", "c5"]);
        assert!(super::super::resolve_committed(&mut all, &ids));
        assert!(all[1].resolved_by == "codehead" && !all[1].open() && all[0].open() && all[2].open());

        // the view on the main checkout: c3 of the linked worktree is 1 row, enter goes there
        let mut v = test_view();
        v.dir = w.clone();
        v.comments = all;
        let rows = v.panel_rows();
        assert!(rows.contains(&PRow::CommentFile("x.txt".into(), 1)) && rows.contains(&PRow::Elsewhere(wt.clone(), 1)), "{rows:?}");
        assert!(!rows.iter().any(|r| matches!(r, PRow::CommentFile(p, _) if p.contains("wt"))), "{rows:?}");
        let all = draw(&mut v, 140, 40).join("\n");
        assert!(all.contains("▾ COMMENTS  1") && all.contains("↗ 1 open in wt"), "{all}");
        alt_tap(&mut v);
        v.scm.at = v.scm.rows.iter().position(|r| matches!(r, PRow::Elsewhere(..))).unwrap();
        assert!(matches!(v.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)), Act::Checkout(r) if r == wt));
        fixture::drop(&w);
    }

    #[test]
    fn agents_list_resolve_and_reopen_comments_by_id() {
        let id = format!("changes-agent-cstate-{}", std::process::id());
        let ids = super::super::agent_comments(&id, "REV", None, &serde_json::json!([{"file": "a.rs", "line": 3, "severity": "high", "text": "off by 1"}, {"file": "b.rs", "line": 9, "severity": "low", "text": "nit"}])).unwrap();
        assert_eq!(ids, vec!["c1", "c2"]);

        let open: serde_json::Value = serde_json::from_str(&super::super::list_comments(&id, "open")).unwrap();
        assert_eq!(open.as_array().unwrap().len(), 2);
        assert_eq!((open[0]["id"].as_str(), open[0]["author"].as_str(), open[0]["state"].as_str()), (Some("c1"), Some("REV"), Some("open")));

        super::super::set_comment_state(&id, "LEAD", &["c1".into()], "resolved", "fixed in a.rs:3").unwrap();
        let c = super::super::read_comments(&id);
        assert_eq!((c[0].state, c[0].resolved_by.as_str(), c[0].note.as_str()), (super::super::CState::Resolved, "LEAD", "fixed in a.rs:3"));
        assert_eq!(serde_json::from_str::<serde_json::Value>(&super::super::list_comments(&id, "open")).unwrap().as_array().unwrap().len(), 1);

        // the reviewer re-opens it; an unknown id: an error, 0 change
        super::super::set_comment_state(&id, "REV", &["c1".into()], "open", "still off").unwrap();
        assert!(super::super::read_comments(&id)[0].open());
        assert!(super::super::set_comment_state(&id, "REV", &["c9".into()], "resolved", "x").unwrap_err().contains("c9"));
        let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir(&id));
    }

    #[test]
    fn an_older_comments_file_gets_ids_on_read() {
        let id = format!("changes-ids-{}", std::process::id());
        std::fs::create_dir_all(crate::engine::store::thread_dir(&id)).unwrap();
        std::fs::write(crate::engine::store::thread_dir(&id).join("comments.json"), r#"[{"path": "a.rs", "line": 1, "text": "old"}]"#).unwrap();

        let c = super::super::read_comments(&id);

        assert_eq!((c[0].id.as_str(), c[0].open()), ("c1", true));
        assert!(std::fs::read_to_string(crate::engine::store::thread_dir(&id).join("comments.json")).unwrap().contains("\"c1\""), "written back");
        let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir(&id));
    }

    #[test]
    fn changes_show_uncommitted_files_all_changes_shows_the_branch_with_comments() {
        use super::super::commits::fixture::*;
        let w = repo("all-changes");
        sh(&w, "echo draft > x.txt");
        let id = format!("commits-all-{}", std::process::id());
        let mut v = commit_view(&w, &id);

        let all = draw(&mut v, 140, 40).join("\n");

        // CHANGES: the uncommitted x.txt only (c1-c4.txt: committed)
        assert!(all.contains("▾ CHANGES  1") && !all.contains("c3.txt"), "{all}");
        assert!(all.contains("uncommitted"), "{all}");

        // the "all changes" row: the branch against origin/main, comments on, git steps off
        let at = v.scm.rows.iter().position(|r| *r == PRow::All).unwrap();
        v.scm.at = at;
        press(&mut v, KeyCode::Enter);
        if let Some(p) = v.peek.as_mut() {
            wait_read(p);
        }
        let all = draw(&mut v, 140, 40).join("\n");
        let review: String = all.lines().map(|l| l.chars().skip(36).collect::<String>()).collect::<Vec<_>>().join("\n");

        assert!(review.contains("all changes against origin/main") && review.contains("c3.txt") && review.contains("x.txt"), "{review}");
        assert!(matches!(press(&mut v, KeyCode::Char('s')), Act::Toast(t) if t.contains("all changes")));
        assert!(v.hints().contains("c: comment"));
        press(&mut v, KeyCode::Char('c'));
        assert!(v.peek.as_ref().is_some_and(|p| p.modal.is_some()), "the comment modal");

        press(&mut v, KeyCode::Esc);
        press(&mut v, KeyCode::Esc);
        assert!(v.peek.is_none());
        let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir(&id));
        drop(&w);
    }
}
