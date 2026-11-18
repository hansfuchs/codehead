//! What an agent learns of edits in its worktree that it did not make: the owner's (in the IDE,
//! between turns) and other agents' (their turns). Before a new turn of an agent, the host puts
//! a note in front of its message: files, new-side line ranges, the owner's diff.
//!
//! Records: git trees of the worktree (tracked + untracked, ignored files left out), written
//! through a temp index: 0 change to the real index, HEAD or a branch. State per thread in
//! `<thread dir>/edits.json`: the tree at each running turn's start, the tree when the last
//! agent at work stopped (`quiet`), each agent's last turn end, the changes since.
//!
//! - owner edits: from `quiet` to the next turn's start (0 agents at work in between). Edits of
//!   the owner while an agent works count as that agent's.
//! - agent edits: from its turn's start to its end.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

use super::{now_ms, store, write_atomic};

/// What an agent gets told (setting `edits`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    Off,
    /// Files and line ranges.
    Files,
    /// Files, line ranges and the owner's diff (max. MAX_DIFF_LINES).
    #[default]
    Diff,
}

impl Mode {
    pub fn parse(s: &str) -> Option<Mode> {
        match s.trim() {
            "off" => Some(Mode::Off),
            "files" => Some(Mode::Files),
            "diff" => Some(Mode::Diff),
            _ => None,
        }
    }
}

/// More untracked files (a build dir not in .gitignore): 0 tree, 0 note.
const MAX_UNTRACKED: usize = 2000;
/// Lines of the owner's diff in a note.
const MAX_DIFF_LINES: usize = 150;
/// Changes kept per thread.
const MAX_LOG: usize = 64;
/// Line ranges per file in a note.
const MAX_RANGES: usize = 4;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct State {
    /// Tree when the last agent at work ended its turn: owner edits start there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    quiet: Option<String>,
    /// Tree at the start of each running turn.
    #[serde(default)]
    start: BTreeMap<String, String>,
    /// End of each agent's last turn, unix ms.
    #[serde(default)]
    seen: BTreeMap<String, u64>,
    #[serde(default)]
    log: Vec<Change>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Change {
    ts: u64,
    /// The agent, None: the owner.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    by: Option<String>,
    from: String,
    to: String,
}

/// A note for 1 agent: SUMMARY for the thread view (1 line), TEXT for the agent.
#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    pub summary: String,
    pub text: String,
}

fn path(id: &str) -> PathBuf {
    store::thread_dir(id).join("edits.json")
}

fn load(id: &str) -> State {
    std::fs::read(path(id)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save(id: &str, s: &State) {
    if let Ok(b) = serde_json::to_vec_pretty(s) {
        let _ = write_atomic(&path(id), &b);
    }
}

/// Stdout of `git ARGS` in DIR (INDEX: its index file), None on a failure.
fn git(dir: &Path, args: &[&str], index: Option<&Path>) -> Option<String> {
    let mut c = Command::new("git");
    c.arg("-C").arg(dir).args(args).stdin(Stdio::null()).stderr(Stdio::null());
    if let Some(i) = index {
        c.env("GIT_INDEX_FILE", i);
    }
    let out = c.output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Tree of DIR's worktree now (tracked + untracked files, ignored ones left out). None: not
/// in a git worktree, or too many untracked files.
pub fn tree(dir: &Path) -> Option<String> {
    let untracked = git(dir, &["ls-files", "--others", "--exclude-standard", "-z"], None)?;
    if untracked.matches('\0').count() > MAX_UNTRACKED {
        return None;
    }
    let real = git(dir, &["rev-parse", "--path-format=absolute", "--git-path", "index"], None)?;
    // 1 name per call: 2 calls in 1 ms (threads, tests) never share a temp index
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = std::env::temp_dir().join(format!("codehead-index-{}-{}-{n}", std::process::id(), now_ms()));
    // a copy of the real index: its stat cache spares hashing unchanged files
    let _ = std::fs::copy(real.trim(), &tmp);
    let tree = git(dir, &["add", "-A"], Some(&tmp)).and_then(|_| git(dir, &["write-tree"], Some(&tmp)));
    let _ = std::fs::remove_file(&tmp);
    tree.map(|t| t.trim().to_string()).filter(|t| !t.is_empty())
}

/// A new turn of AGENT starts in DIR (OTHERS_BUSY: another agent is mid-turn). Records the
/// turn's start, closes the owner's edits; returns the note for AGENT (0 changes: None).
pub fn turn_start(id: &str, dir: &Path, agent: &str, others_busy: bool) -> Option<Note> {
    let mode = crate::config::edits();
    if mode == Mode::Off {
        return None;
    }
    let now = tree(dir)?;
    let mut st = load(id);
    st.start(now, now_ms(), agent, others_busy);
    let note = st.seen.get(agent).and_then(|since| note(dir, &st.log, *since, agent, mode));
    trim(&mut st);
    save(id, &st);
    note
}

/// AGENT's turn ended (ALL_IDLE: 0 agents at work now): its edits go in the log.
pub fn turn_end(id: &str, dir: &Path, agent: &str, all_idle: bool) {
    if crate::config::edits() == Mode::Off {
        return;
    }
    let Some(end) = tree(dir) else { return };
    let mut st = load(id);
    st.end(end, now_ms(), agent, all_idle);
    trim(&mut st);
    save(id, &st);
}

impl State {
    /// A turn of AGENT starts at tree NOW, time TS: the owner's edits since the last agent
    /// stopped (0 agents at work in between) go in the log.
    fn start(&mut self, now: String, ts: u64, agent: &str, others_busy: bool) {
        if !others_busy
            && let Some(q) = self.quiet.take()
            && q != now
        {
            self.log.push(Change { ts, by: None, from: q, to: now.clone() });
        }
        self.start.insert(agent.to_string(), now);
    }

    /// AGENT's turn ends at tree END, time TS: its edits go in the log.
    fn end(&mut self, end: String, ts: u64, agent: &str, all_idle: bool) {
        if let Some(s) = self.start.remove(agent)
            && s != end
        {
            self.log.push(Change { ts, by: Some(agent.to_string()), from: s, to: end.clone() });
        }
        self.seen.insert(agent.to_string(), ts);
        if all_idle {
            self.quiet = Some(end);
        }
    }
}

/// Changes no agent needs any more go; MAX_LOG at most.
fn trim(st: &mut State) {
    if let Some(oldest) = st.seen.values().min().copied() {
        st.log.retain(|c| c.ts > oldest);
    }
    let n = st.log.len();
    if n > MAX_LOG {
        st.log.drain(..n - MAX_LOG);
    }
}

/// A file in a change: path, lines added, removed, new-side line ranges, new / deleted.
#[derive(Clone, Debug, Default, PartialEq)]
struct File {
    path: String,
    add: usize,
    del: usize,
    ranges: Vec<(usize, usize)>,
    new: bool,
    gone: bool,
}

impl File {
    fn label(&self) -> String {
        let mut s = self.path.clone();
        if !self.new && !self.gone && !self.ranges.is_empty() {
            let mut r: Vec<String> = self.ranges.iter().take(MAX_RANGES).map(|&(a, b)| if a == b { a.to_string() } else { format!("{a}-{b}") }).collect();
            if self.ranges.len() > MAX_RANGES {
                r.push("…".into());
            }
            s.push(':');
            s.push_str(&r.join(", "));
        }
        let tag = if self.new {
            " (new)".to_string()
        } else if self.gone {
            " (deleted)".to_string()
        } else {
            format!(" (+{} −{})", self.add, self.del)
        };
        s + &tag
    }
}

/// Files of `git diff -U0 -w` output DIFF (whitespace-only changes left out).
fn files(diff: &str) -> Vec<File> {
    let mut out: Vec<File> = Vec::new();
    let mut old: Option<String> = None;
    for l in diff.lines() {
        if let Some(p) = l.strip_prefix("--- ") {
            old = (p != "/dev/null").then(|| p.trim_start_matches("a/").to_string());
        } else if let Some(p) = l.strip_prefix("+++ ") {
            let f = if p == "/dev/null" {
                File { path: old.clone().unwrap_or_default(), gone: true, ..Default::default() }
            } else {
                File { path: p.trim_start_matches("b/").to_string(), new: old.is_none(), ..Default::default() }
            };
            out.push(f);
        } else if let Some(h) = l.strip_prefix("@@ ")
            && let Some(f) = out.last_mut()
        {
            // `@@ -a,b +c,d @@`: new side c..c+d-1 (d 0: lines removed after c)
            if let Some(new) = h.split(' ').find(|w| w.starts_with('+')) {
                let mut it = new[1..].split(',');
                let c: usize = it.next().and_then(|x| x.parse().ok()).unwrap_or(0);
                let d: usize = it.next().and_then(|x| x.parse().ok()).unwrap_or(1);
                let (a, b) = if d == 0 { (c.max(1), c.max(1)) } else { (c, c + d - 1) };
                match f.ranges.last_mut() {
                    Some(last) if a <= last.1 + 1 => last.1 = last.1.max(b),
                    _ => f.ranges.push((a, b)),
                }
            }
        } else if l.starts_with('+') {
            if let Some(f) = out.last_mut() {
                f.add += 1;
            }
        } else if l.starts_with('-')
            && let Some(f) = out.last_mut()
        {
            f.del += 1;
        }
    }
    out
}

/// Files of the changes in LOG after SINCE by WHO (None: the owner), merged per path.
fn merged(dir: &Path, log: &[&Change]) -> Vec<File> {
    let mut by_path: BTreeMap<String, File> = BTreeMap::new();
    let mut order: Vec<String> = Vec::new();
    for c in log {
        let diff = git(dir, &["diff", "-U0", "-w", "--no-color", "--no-ext-diff", "--no-renames", &c.from, &c.to], None).unwrap_or_default();
        for f in files(&diff) {
            match by_path.get_mut(&f.path) {
                Some(m) => {
                    m.add += f.add;
                    m.del += f.del;
                    m.gone = f.gone;
                    m.ranges.extend(f.ranges);
                    m.ranges.sort_unstable();
                    m.ranges.dedup();
                }
                None => {
                    order.push(f.path.clone());
                    by_path.insert(f.path.clone(), f);
                }
            }
        }
    }
    order.into_iter().filter_map(|p| by_path.remove(&p)).filter(|f| f.new || f.gone || f.add + f.del > 0).collect()
}

/// The note for AGENT: changes in LOG after SINCE not by AGENT. None: 0 changes.
fn note(dir: &Path, log: &[Change], since: u64, agent: &str, mode: Mode) -> Option<Note> {
    let fresh: Vec<&Change> = log.iter().filter(|c| c.ts > since && c.by.as_deref() != Some(agent)).collect();
    // groups: the owner first, then the agents in the order of their 1st change
    let mut groups: Vec<Option<String>> = Vec::new();
    if fresh.iter().any(|c| c.by.is_none()) {
        groups.push(None);
    }
    for c in &fresh {
        if c.by.is_some() && !groups.contains(&c.by) {
            groups.push(c.by.clone());
        }
    }
    let mut text = String::from("[codehead: changes in this worktree since your last turn, not made by you. Read a file again before you edit it.]\n");
    let mut summary: Vec<String> = Vec::new();
    for g in &groups {
        let changes: Vec<&Change> = fresh.iter().copied().filter(|c| c.by == *g).collect();
        let fs = merged(dir, &changes);
        if fs.is_empty() {
            continue;
        }
        let who = g.as_ref().map_or_else(|| "owner".to_string(), |a| format!("@{a}"));
        text.push_str(&format!("{who}:\n"));
        for f in &fs {
            text.push_str(&format!("- {}\n", f.label()));
        }
        let mut shown: Vec<String> = fs.iter().take(3).map(File::label).collect();
        if fs.len() > 3 {
            shown.push(format!("+{} more", fs.len() - 3));
        }
        summary.push(format!("{who}: {}", shown.join(", ")));
        if g.is_none() && mode == Mode::Diff {
            let (from, to) = (&changes[0].from, &changes[changes.len() - 1].to);
            // 1 owner span: its diff; several (agents between): each in turn
            let mut lines: Vec<String> = Vec::new();
            for c in &changes {
                let d = git(dir, &["diff", "-w", "-U2", "--no-color", "--no-ext-diff", &c.from, &c.to], None).unwrap_or_default();
                lines.extend(d.lines().map(str::to_string));
            }
            let total = lines.len();
            text.push_str("owner diff:\n```diff\n");
            for l in lines.iter().take(MAX_DIFF_LINES) {
                text.push_str(l);
                text.push('\n');
            }
            text.push_str("```\n");
            if total > MAX_DIFF_LINES {
                text.push_str(&format!("… {} more lines", total - MAX_DIFF_LINES));
                if changes.len() == 1 {
                    text.push_str(&format!(": git diff {from} {to}"));
                }
                text.push('\n');
            }
        }
    }
    if summary.is_empty() {
        return None;
    }
    text.push_str("[end of changes]");
    Some(Note { summary: summary.join(" · "), text })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sh(dir: &Path, cmd: &str) {
        let ok = Command::new("sh").arg("-c").arg(cmd).current_dir(dir).stdout(Stdio::null()).stderr(Stdio::null()).status().unwrap().success();
        assert!(ok, "{cmd}");
    }

    fn repo(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("codehead-edits-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        sh(&d, "git init -q && git config user.email t@t && git config user.name t && printf 'a\\nb\\nc\\nd\\ne\\n' > x.txt && echo .env > .gitignore && git add -A && git commit -qm init");
        d
    }

    #[test]
    fn tree_leaves_the_index_alone_and_sees_untracked_files() {
        let d = repo("tree");
        let t0 = tree(&d).unwrap();
        std::fs::write(d.join("new.txt"), "hi\n").unwrap();
        std::fs::write(d.join(".env"), "SECRET=1\n").unwrap();
        let t1 = tree(&d).unwrap();
        assert_ne!(t0, t1);
        // 0 staged files: the real index did not change
        assert_eq!(git(&d, &["diff", "--cached", "--name-only"], None).unwrap().trim(), "");
        let names = git(&d, &["diff", "--name-only", &t0, &t1], None).unwrap();
        assert!(names.contains("new.txt") && !names.contains(".env"), "{names}");
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn files_have_new_side_ranges_and_tags() {
        let diff = "diff --git a/x.rs b/x.rs\n--- a/x.rs\n+++ b/x.rs\n@@ -3,2 +3,3 @@\n-a\n-b\n+a\n+b\n+c\n@@ -10 +11,0 @@\n-gone\ndiff --git a/n.rs b/n.rs\n--- /dev/null\n+++ b/n.rs\n@@ -0,0 +1,2 @@\n+1\n+2\n";
        let f = files(diff);
        assert_eq!(f.len(), 2);
        assert_eq!((f[0].ranges.clone(), f[0].add, f[0].del), (vec![(3, 5), (11, 11)], 3, 3));
        assert_eq!(f[0].label(), "x.rs:3-5, 11 (+3 −3)");
        assert_eq!(f[1].label(), "n.rs (new)");
    }

    #[test]
    fn owner_and_other_agent_edits_reach_the_agent_its_own_do_not() {
        let d = repo("note");
        let mut log = Vec::new();
        let t0 = tree(&d).unwrap();
        // ada's turn: edits line 2
        sh(&d, "printf 'a\\nB\\nc\\nd\\ne\\n' > x.txt");
        let t1 = tree(&d).unwrap();
        log.push(Change { ts: 10, by: Some("ada".into()), from: t0, to: t1.clone() });
        // the owner, between turns: line 4, a new file
        sh(&d, "printf 'a\\nB\\nc\\nD\\ne\\n' > x.txt && echo hi > y.txt");
        let t2 = tree(&d).unwrap();
        log.push(Change { ts: 20, by: None, from: t1, to: t2.clone() });
        // bob's turn: line 5
        sh(&d, "printf 'a\\nB\\nc\\nD\\nE\\n' > x.txt");
        let t3 = tree(&d).unwrap();
        log.push(Change { ts: 30, by: Some("bob".into()), from: t2, to: t3 });
        let n = note(&d, &log, 10, "ada", Mode::Diff).unwrap();
        assert!(n.text.contains("owner:\n- x.txt:4 (+1 −1)\n- y.txt (new)\n"), "{}", n.text);
        assert!(n.text.contains("@bob:\n- x.txt:5 (+1 −1)"), "{}", n.text);
        assert!(!n.text.contains("@ada"), "own edits: 0\n{}", n.text);
        assert!(n.text.contains("```diff") && n.text.contains("+D"), "{}", n.text);
        assert_eq!(n.summary, "owner: x.txt:4 (+1 −1), y.txt (new) · @bob: x.txt:5 (+1 −1)");
        // files mode: 0 diff
        let f = note(&d, &log, 10, "ada", Mode::Files).unwrap();
        assert!(!f.text.contains("```diff"), "{}", f.text);
        // 0 changes after its turn: 0 note
        assert!(note(&d, &log, 30, "ada", Mode::Diff).is_none());
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn owner_spans_are_the_gaps_with_0_agents_at_work() {
        let mut st = State::default();
        let by = |st: &State| st.log.iter().map(|c| (c.by.clone().unwrap_or_else(|| "owner".into()), c.from.clone(), c.to.clone())).collect::<Vec<_>>();
        // ada: t0 → t1; the owner: t1 → t2; ada again
        st.start("t0".into(), 1, "ada", false);
        st.end("t1".into(), 2, "ada", true);
        st.start("t2".into(), 3, "ada", false);
        assert_eq!(by(&st), [("ada".into(), "t0".into(), "t1".into()), ("owner".into(), "t1".into(), "t2".into())]);
        // bob starts while ada works: 0 owner span; ada ends while bob works: 0 quiet tree
        st.start("t3".into(), 4, "bob", true);
        st.end("t4".into(), 5, "ada", false);
        assert_eq!(st.quiet, None);
        // bob ends, all idle: the next start (same tree) adds 0 owner span
        st.end("t5".into(), 6, "bob", true);
        st.start("t5".into(), 7, "ada", false);
        assert_eq!(by(&st).len(), 4, "{:?}", by(&st));
        assert_eq!(st.seen.get("bob"), Some(&6));
    }

    #[test]
    fn whitespace_only_edits_make_no_note() {
        let d = repo("ws");
        let t0 = tree(&d).unwrap();
        sh(&d, "printf 'a  \\nb\\nc\\nd\\ne\\n' > x.txt");
        let t1 = tree(&d).unwrap();
        let log = vec![Change { ts: 20, by: None, from: t0, to: t1 }];
        assert!(note(&d, &log, 10, "ada", Mode::Diff).is_none());
        let _ = std::fs::remove_dir_all(d);
    }
}
