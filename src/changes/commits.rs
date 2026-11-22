//! The branch's commits for the panel's COMMITS section: unpushed (not on the upstream) and
//! pushed, newest first. Uncommit: picked unpushed commits leave the history, their changes stay
//! in the worktree as uncommitted changes (git plumbing: 0 worktree write); push up to a commit.

use std::collections::HashSet;
use std::path::Path;
use std::process::{Command, Stdio};

use super::git;

/// Commits listed at most (a long history: the newest ones).
pub const MAX: usize = 50;

/// A commit of the branch.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Commit {
    pub oid: String,
    pub short: String,
    pub subject: String,
    /// The whole message (subject + body).
    pub body: String,
    pub author: String,
    /// Commit time (unix seconds).
    pub time: i64,
    /// On the upstream (else: only local).
    pub pushed: bool,
    /// 2+ parents.
    pub merge: bool,
}

/// The branch's commits: newest first, the unpushed ones before the pushed ones.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Commits {
    pub list: Vec<Commit>,
    /// `origin/x`: the upstream (None: 0 upstream, every commit unpushed).
    pub upstream: Option<String>,
    /// Commits on the upstream, 0 on the branch (a pull / rebase needed before a push).
    pub remote_only: usize,
    /// More commits than `MAX`.
    pub more: bool,
    /// HEAD's oid at the read (an uncommit refuses when HEAD moved since).
    pub head: String,
}

impl Commits {
    pub fn find(&self, oid: &str) -> Option<&Commit> {
        self.list.iter().find(|c| c.oid == oid)
    }
}

/// DIR's branch (None: detached HEAD).
fn branch(dir: &Path) -> Option<String> {
    let b = git(dir, &["rev-parse", "--abbrev-ref", "HEAD"]).ok()?.trim().to_string();
    (!b.is_empty() && b != "HEAD").then_some(b)
}

/// The remote branch B of DIR tracks when that remote branch has B's name (`refs/heads/B`).
fn tracked_remote(dir: &Path, b: &str) -> Option<String> {
    let out = git(dir, &["for-each-ref", "--format=%(upstream:remotename)%00%(upstream:remoteref)", &format!("refs/heads/{b}")]).ok()?;
    let (remote, rref) = out.trim().split_once('\0')?;
    (!remote.is_empty() && rref == format!("refs/heads/{b}")).then(|| remote.to_string())
}

/// The upstream of DIR's branch `x`: the tracked branch when its name is `x` too (`origin/x`,
/// `fork/x`), else `origin/x` when it exists. A branch that tracks another name (`origin/main`,
/// `origin/hans/x`): `origin/x`, so a push never lands on that other branch.
pub fn upstream(dir: &Path) -> Option<String> {
    let b = branch(dir)?;
    // a remote branch deleted after the merge (`[gone]`): 0 upstream, every commit unpushed
    let exists = |r: &str| git(dir, &["rev-parse", "--verify", "--quiet", &format!("refs/remotes/{r}")]).is_ok();
    if let Some(r) = tracked_remote(dir, &b).map(|r| format!("{r}/{b}"))
        && exists(&r)
    {
        return Some(r);
    }

    let r = format!("origin/{b}");
    exists(&r).then_some(r)
}

/// The commits of DIR's branch since REV (the merge-base; `HEAD`: 0 base, the newest `MAX`).
pub fn list(dir: &Path, rev: &str) -> Commits {
    let up = upstream(dir);
    let n = format!("-n{}", MAX + 1);
    let mut args = vec!["log", "--topo-order", &n, "--format=%H%x1f%h%x1f%P%x1f%an%x1f%ct%x1f%s%x1f%B%x1e", "HEAD"];
    let not = format!("^{rev}");
    if rev != "HEAD" && !rev.is_empty() {
        args.push(&not);
    }
    let Ok(out) = git(dir, &args) else { return Commits { upstream: up, ..Default::default() } };

    // unpushed: on HEAD, 0 on the upstream (`<`; 0 upstream: all); `>`: on the upstream only
    let mut remote_only = 0;
    // a failed read: unknown, as 0 upstream (never "all pushed")
    let local: Option<HashSet<String>> = up.as_ref().and_then(|u| {
        let lr = git(dir, &["rev-list", "--left-right", &format!("HEAD...{u}")]).ok()?;
        remote_only = lr.lines().filter(|l| l.starts_with('>')).count();
        Some(lr.lines().filter_map(|l| l.strip_prefix('<')).map(str::to_string).collect())
    });
    let mut list: Vec<Commit> = out
        .split('\x1e')
        .filter_map(|rec| {
            let rec = rec.trim_start_matches('\n');
            let f: Vec<&str> = rec.splitn(7, '\x1f').collect();
            if f.len() < 7 {
                return None;
            }
            let pushed = local.as_ref().is_some_and(|l| !l.contains(f[0]));
            Some(Commit {
                oid: f[0].into(),
                short: f[1].into(),
                merge: f[2].split_whitespace().count() > 1,
                author: f[3].into(),
                time: f[4].parse().unwrap_or(0),
                subject: f[5].into(),
                body: f[6].trim_end().into(),
                pushed,
            })
        })
        .collect();

    // HEAD: the log's 1st commit (0 commits: 0 pick, 0 uncommit, HEAD unused)
    let head = list.first().map(|c| c.oid.clone()).unwrap_or_default();
    let more = list.len() > MAX;
    list.truncate(MAX);

    // unpushed first (stable: newest first within each)
    list.sort_by_key(|c| c.pushed);
    Commits { list, upstream: up, remote_only, more, head }
}

/// `git ARGS` in DIR with ENV and STDIN: Ok(stdout) / Err((exit code, stdout, 1st stderr line)).
fn run(dir: &Path, args: &[&str], env: &[(&str, String)], stdin: Option<&str>) -> Result<String, (i32, String, String)> {
    use std::io::Write;
    let mut c = Command::new("git");
    c.arg("-C").arg(dir).args(["-c", "core.quotePath=false", "-c", "log.showSignature=false"]).args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
    c.stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() });
    for (k, v) in env {
        c.env(k, v);
    }
    let mut ch = c.spawn().map_err(|e| (-1, String::new(), format!("git: {e}")))?;
    if let (Some(s), Some(mut i)) = (stdin, ch.stdin.take()) {
        let _ = i.write_all(s.as_bytes());
    }
    let out = ch.wait_with_output().map_err(|e| (-1, String::new(), format!("git: {e}")))?;
    let so = String::from_utf8_lossy(&out.stdout).into_owned();
    if out.status.success() {
        Ok(so)
    } else {
        let se = String::from_utf8_lossy(&out.stderr);
        Err((out.status.code().unwrap_or(-1), so, se.lines().find(|l| !l.trim().is_empty()).unwrap_or("git failed").to_string()))
    }
}

fn short(oid: &str) -> &str {
    &oid[..oid.len().min(7)]
}

/// What an uncommit did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Uncommitted {
    pub count: usize,
    /// A later commit's change of a file that conflicted: out of the history too (short, path).
    pub cascaded: Vec<(String, String)>,
    /// Later commits with 0 change left: dropped (short).
    pub dropped: Vec<String>,
    /// The staged changes did not fit the new HEAD: the index kept as it was.
    pub index_kept: bool,
}

impl Uncommitted {
    pub fn note(&self) -> String {
        let mut s = format!("uncommitted {} commit{}", self.count, if self.count == 1 { "" } else { "s" });
        if !self.cascaded.is_empty() {
            let v: Vec<String> = self.cascaded.iter().map(|(c, p)| format!("{p} ({c})")).collect();
            s.push_str(&format!(" · cascaded: {}", v.join(", ")));
        }
        if !self.dropped.is_empty() {
            s.push_str(&format!(" · emptied, dropped: {}", self.dropped.join(", ")));
        }
        if self.index_kept {
            s.push_str(" · index kept: the changes show staged");
        }
        s
    }
}

/// The backup ref of THREAD's last uncommit (the old HEAD).
pub fn backup_ref(thread: &str) -> String {
    let t: String = thread.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' }).collect();
    format!("refs/codehead/uncommit/{t}")
}

/// Uncommit PICKS (oids) in DIR: they leave the history, the later commits get replayed on top
/// of what stays. The worktree stays byte for byte; the picked changes show as uncommitted. A
/// later commit's change that conflicts with the change taken out: out of the history too
/// (cascade). HEAD must still be HEAD (else an agent committed meanwhile: 0 change).
pub fn uncommit(dir: &Path, picks: &[String], head: &str, thread: &str) -> Result<Uncommitted, String> {
    let now = git(dir, &["rev-parse", "HEAD"])?.trim().to_string();
    if now != head {
        return Err("HEAD moved meanwhile: read again (r), pick again".into());
    }

    if picks.is_empty() {
        return Err("0 commits picked".into());
    }

    if super::stopped(dir) {
        return Err("a rebase / merge stopped: finish it first".into());
    }

    if let Some(u) = upstream(dir) {
        let local: HashSet<String> = git(dir, &["rev-list", "HEAD", &format!("^{u}")])?.lines().map(str::to_string).collect();
        if let Some(p) = picks.iter().find(|p| !local.contains(*p)) {
            return Err(format!("{}: pushed (0 uncommit)", short(p)));
        }
    }

    // the first-parent chain from HEAD down to the oldest pick
    let picked: HashSet<&str> = picks.iter().map(String::as_str).collect();
    let chain = git(dir, &["rev-list", "--first-parent", "--parents", "-n2000", "HEAD"])?;
    let mut range: Vec<(String, Vec<String>)> = Vec::new();
    let mut found = 0;
    for l in chain.lines() {
        let mut it = l.split_whitespace();
        let Some(c) = it.next() else { continue };
        let parents: Vec<String> = it.map(str::to_string).collect();
        if parents.len() > 1 {
            return Err(format!("{}: a merge commit in the range (0 uncommit)", short(c)));
        }

        if picked.contains(c) {
            if parents.is_empty() {
                return Err(format!("{}: the root commit (0 uncommit)", short(c)));
            }
            found += 1;
        }

        range.push((c.to_string(), parents));
        if found == picked.len() {
            break;
        }
    }

    if found < picked.len() {
        return Err("a picked commit is not on the branch's first-parent line".into());
    }

    range.reverse();
    let mut cur = range[0].1[0].clone();
    let mut done = Uncommitted { count: picked.len(), ..Default::default() };
    for (c, parents) in &range {
        if picked.contains(c.as_str()) {
            continue;
        }

        let tree = replay(dir, &cur, c, &parents[0], &mut done)?;
        let cur_tree = git(dir, &["rev-parse", &format!("{cur}^{{tree}}")])?.trim().to_string();
        if tree == cur_tree {
            done.dropped.push(short(c).to_string());
            continue;
        }

        cur = recommit(dir, c, &tree, &cur)?;
    }

    move_head(dir, &now, &cur, thread, &mut done)?;
    Ok(done)
}

/// HEAD from OLD to NEW with the index along, under the index lock (0 agent `git commit` / `add`
/// between the two): the new index = a 2-way merge (`read-tree -m OLD NEW`) of a copy of the
/// index (staged entries, skip-worktree bits, stat data stay; a path both staged and changed by
/// the move: the index stays as it was, DONE says so). The old HEAD: THREAD's backup ref.
fn move_head(dir: &Path, old: &str, new: &str, thread: &str, done: &mut Uncommitted) -> Result<(), String> {
    let index = std::path::PathBuf::from(git(dir, &["rev-parse", "--path-format=absolute", "--git-path", "index"])?.trim());
    let lock = index.with_extension("lock");
    let tmp = index.with_extension("codehead-uncommit");
    std::fs::OpenOptions::new().write(true).create_new(true).open(&lock).map_err(|_| "the index is locked (an agent's git runs): try again".to_string())?;

    // the lock is ours until the rename puts it in the index's place (then another git may take a
    // new one: 0 remove after that)
    let mut renamed = false;
    let result = (|| {
        // a missing index (0 file yet): the merge starts from an empty one
        let _ = std::fs::remove_file(&tmp);
        if index.exists() {
            std::fs::copy(&index, &tmp).map_err(|e| format!("index copy: {e}"))?;
        }
        let env = [("GIT_INDEX_FILE", tmp.to_string_lossy().into_owned())];
        let merged = run(dir, &["read-tree", "-m", old, new], &env, None).is_ok();

        git(dir, &["update-ref", "--create-reflog", "-m", "codehead: uncommit backup", &backup_ref(thread), old])?;
        git(dir, &["update-ref", "-m", "codehead: uncommit", "HEAD", new, old]).map_err(|e| format!("HEAD moved meanwhile: {e}"))?;

        // HEAD moved: the new index in place through the lock file (as git writes it); a write
        // error: the old index stays (the note says so), 0 Err after HEAD moved
        renamed = merged && std::fs::copy(&tmp, &lock).is_ok() && std::fs::rename(&lock, &index).is_ok();
        done.index_kept = !renamed;
        Ok(())
    })();

    let _ = std::fs::remove_file(&tmp);
    if !renamed {
        let _ = std::fs::remove_file(&lock);
    }

    if result.is_ok() {
        let _ = git(dir, &["update-index", "-q", "--refresh"]);
    }
    result
}

/// Commit C (parent P) replayed on CUR: its tree. A conflicted path keeps CUR's version (C's
/// change of it: out of the history, noted in DONE).
fn replay(dir: &Path, cur: &str, c: &str, p: &str, done: &mut Uncommitted) -> Result<String, String> {
    let base = format!("--merge-base={p}");
    let (tree, conflicted) = match run(dir, &["merge-tree", "--write-tree", "--name-only", "--no-messages", &base, cur, c], &[], None) {
        Ok(o) => (o.lines().next().unwrap_or_default().to_string(), Vec::new()),
        Err((1, o, _)) => {
            let mut lines = o.lines();
            let t = lines.next().unwrap_or_default().to_string();
            let mut paths: Vec<String> = Vec::new();
            for l in lines.take_while(|l| !l.is_empty()) {
                if !paths.iter().any(|x| x == l) {
                    paths.push(l.to_string());
                }
            }
            (t, paths)
        }
        // git < 2.40 (0 `--merge-base`): the newest commits still uncommit (0 replay)
        Err((_, _, e)) => return Err(format!("merge-tree (git 2.40+; older git: pick the newest commits only): {e}")),
    };
    if conflicted.is_empty() {
        return Ok(tree);
    }

    // a temp index: the merged tree, each conflicted path as CUR has it
    let idx = std::env::temp_dir().join(format!("codehead-uncommit-{}-{}", std::process::id(), short(c)));
    let env = [("GIT_INDEX_FILE", idx.to_string_lossy().into_owned())];
    let result = (|| {
        run(dir, &["read-tree", &tree], &env, None).map_err(|e| e.2)?;
        for path in &conflicted {
            let ls = git(dir, &["ls-tree", cur, "--", path])?;
            match ls.split_whitespace().collect::<Vec<_>>().as_slice() {
                [mode, _, blob, ..] => {
                    run(dir, &["update-index", "--add", "--cacheinfo", &format!("{mode},{blob},{path}")], &env, None).map_err(|e| e.2)?;
                }
                _ => {
                    run(dir, &["update-index", "--force-remove", "--", path], &env, None).map_err(|e| e.2)?;
                }
            }
            done.cascaded.push((short(c).to_string(), path.clone()));
        }
        run(dir, &["write-tree"], &env, None).map(|t| t.trim().to_string()).map_err(|e| e.2)
    })();
    let _ = std::fs::remove_file(&idx);
    result
}

/// A copy of commit C with TREE on PARENT: C's author, committer, dates, message.
fn recommit(dir: &Path, c: &str, tree: &str, parent: &str) -> Result<String, String> {
    let meta = git(dir, &["log", "-1", "--format=%an%x00%ae%x00%ad%x00%cn%x00%ce%x00%cd", "--date=raw", c])?;
    let f: Vec<&str> = meta.trim_end_matches('\n').split('\0').collect();
    if f.len() < 6 {
        return Err(format!("{}: 0 metadata", short(c)));
    }
    // %B ends in a line break of its own: 1 line break kept (0 more per rewrite)
    let msg = format!("{}\n", git(dir, &["log", "-1", "--format=%B", c])?.trim_end_matches('\n'));
    let env = [
        ("GIT_AUTHOR_NAME", f[0].to_string()),
        ("GIT_AUTHOR_EMAIL", f[1].to_string()),
        ("GIT_AUTHOR_DATE", f[2].to_string()),
        ("GIT_COMMITTER_NAME", f[3].to_string()),
        ("GIT_COMMITTER_EMAIL", f[4].to_string()),
        ("GIT_COMMITTER_DATE", f[5].to_string()),
    ];
    run(dir, &["commit-tree", tree, "-p", parent, "-F", "-"], &env, Some(&msg)).map(|o| o.trim().to_string()).map_err(|e| format!("{}: {}", short(c), e.2))
}

/// Push DIR's commits up to OID (OID and the unpushed commits before it) to the branch of the
/// same name (as `push origin HEAD`): on the upstream's remote, else on origin. Never to the
/// branch of another name the local one may track.
pub fn push_to(dir: &Path, oid: &str) -> Result<String, String> {
    let Some(branch) = branch(dir) else { return Err("detached HEAD: 0 branch to push to".into()) };

    // an agent rebased while the question was open: the commit left the branch
    if git(dir, &["merge-base", "--is-ancestor", oid, "HEAD"]).is_err() {
        return Err(format!("{}: 0 more on the branch (read again: r)", short(oid)));
    }

    let tracked = tracked_remote(dir, &branch);
    let remote = tracked.clone().unwrap_or_else(|| "origin".into());
    let out = git(dir, &["push", &remote, &format!("{oid}:refs/heads/{branch}")])?;

    // 0 upstream of the same name yet: the pushed branch becomes it (as `push -u` does)
    if tracked.is_none() {
        let _ = git(dir, &["branch", &format!("--set-upstream-to={remote}/{branch}")]);
    }
    Ok(out)
}

/// Test repos: a work checkout with a bare `origin`.
#[cfg(test)]
pub(crate) mod fixture {
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};

    /// `sh -c C` in DIR (must pass).
    pub fn sh(dir: &Path, c: &str) {
        let ok = Command::new("sh").arg("-c").arg(c).current_dir(dir).stdout(Stdio::null()).stderr(Stdio::null()).status().unwrap().success();
        assert!(ok, "{c}");
    }

    /// `sh -c C` in DIR: its stdout, trimmed.
    pub fn out(dir: &Path, c: &str) -> String {
        let o = Command::new("sh").arg("-c").arg(c).current_dir(dir).output().unwrap();
        String::from_utf8_lossy(&o.stdout).trim().to_string()
    }

    /// A checkout on branch `feat`: `init` on origin/main; c1, c2 pushed (origin/feat); c3, c4
    /// local. c1-c4 each add `<name>.txt`.
    pub fn repo(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("codehead-commits-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("w")).unwrap();
        sh(&d, "git init -q --bare -b main origin.git");
        let w = d.join("w");
        sh(&w, "git init -q -b main && git config user.email t@t && git config user.name t && git config commit.gpgsign false && git config core.hooksPath /dev/null");
        sh(&w, "seq 1 5 > x.txt && echo y > y.txt && git add -A && git commit -qm init && git remote add origin ../origin.git && git push -q -u origin main");
        sh(&w, "git switch -q -c feat");
        for c in ["c1", "c2"] {
            sh(&w, &format!("echo {c} > {c}.txt && git add -A && git commit -qm '{c} subject' -m '{c} body'"));
        }
        sh(&w, "git push -q -u origin feat");
        for c in ["c3", "c4"] {
            sh(&w, &format!("echo {c} > {c}.txt && git add -A && git commit -qm '{c} subject' -m '{c} body'"));
        }
        w
    }

    /// The oid of REV in DIR.
    pub fn oid(dir: &Path, rev: &str) -> String {
        out(dir, &format!("git rev-parse {rev}"))
    }

    pub fn drop(dir: &Path) {
        if let Some(p) = dir.parent() {
            let _ = std::fs::remove_dir_all(p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fixture::*;
    use super::*;

    fn base(w: &Path) -> String {
        oid(w, "origin/main")
    }

    #[test]
    fn list_puts_unpushed_commits_first_then_pushed_ones() {
        let w = repo("list");

        let cs = list(&w, &base(&w));

        let subjects: Vec<(&str, bool)> = cs.list.iter().map(|c| (c.subject.as_str(), c.pushed)).collect();
        assert_eq!(subjects, vec![("c4 subject", false), ("c3 subject", false), ("c2 subject", true), ("c1 subject", true)]);
        assert_eq!(cs.upstream.as_deref(), Some("origin/feat"));
        assert_eq!(cs.head, oid(&w, "HEAD"));
        assert!(cs.list[0].body.contains("c4 body") && !cs.more && cs.remote_only == 0);

        // 0 upstream: every commit unpushed; HEAD as base: 0 commit
        sh(&w, "git branch --unset-upstream && git push -q origin --delete feat");
        let cs = list(&w, &base(&w));
        assert!(cs.upstream.is_none() && cs.list.iter().all(|c| !c.pushed) && cs.list.len() == 4);
        assert!(list(&w, "HEAD").list.len() == 5, "HEAD: the newest commits, 0 base");
        drop(&w);
    }

    #[test]
    fn uncommit_of_the_top_commit_keeps_the_files_and_the_staged_changes() {
        let w = repo("top");
        sh(&w, "echo staged > y.txt && git add y.txt && echo dirty >> x.txt");
        let (head, c4) = (oid(&w, "HEAD"), oid(&w, "HEAD"));
        let before = out(&w, "cat x.txt y.txt c3.txt c4.txt");

        let u = uncommit(&w, std::slice::from_ref(&c4), &head, "t1").unwrap();

        assert_eq!(u.count, 1);
        assert_eq!(oid(&w, "HEAD"), oid(&w, &format!("{c4}^")), "HEAD: c3");
        assert_eq!(out(&w, "cat x.txt y.txt c3.txt c4.txt"), before, "the worktree as before");
        assert_eq!(out(&w, "git diff --cached --name-only"), "y.txt", "the staged change stays staged; c4 unstaged");
        assert_eq!(out(&w, "git status --porcelain c4.txt"), "?? c4.txt");
        assert_eq!(oid(&w, &backup_ref("t1")), head, "the old HEAD in the backup ref");
        drop(&w);
    }

    #[test]
    fn uncommit_of_a_middle_commit_cascades_a_conflicting_later_change() {
        let w = repo("mid");
        // c5 changes x.txt line 1, c6 changes the same line again and adds z.txt
        sh(&w, "sed -i.bak '1s/.*/A/' x.txt && rm x.txt.bak && git commit -qam c5");
        sh(&w, "sed -i.bak '1s/.*/B/' x.txt && rm x.txt.bak && echo z > z.txt && git add -A && git commit -qm c6");
        let (head, c5) = (oid(&w, "HEAD"), oid(&w, "HEAD^"));
        let before = out(&w, "cat x.txt z.txt");

        let u = uncommit(&w, std::slice::from_ref(&c5), &head, "t2").unwrap();

        assert_eq!(out(&w, "cat x.txt z.txt"), before, "the worktree as before");
        assert_eq!(u.cascaded, vec![(head[..7].to_string(), "x.txt".to_string())]);
        assert_eq!(out(&w, "git log -1 --format=%s"), "c6", "c6 stays, with z.txt only");
        assert_eq!(out(&w, "git show --format= --name-only HEAD"), "z.txt");
        assert_eq!(out(&w, "git diff --name-only"), "x.txt", "x.txt: uncommitted");
        assert_eq!(out(&w, "git log --format=%s HEAD~1 -1"), "c4 subject");
        drop(&w);
    }

    #[test]
    fn uncommit_refuses_pushed_commits_a_moved_head_and_merges() {
        let w = repo("refuse");
        let head = oid(&w, "HEAD");
        let c2 = oid(&w, "origin/feat");

        assert!(uncommit(&w, &[c2], &head, "t3").unwrap_err().contains("pushed"));
        assert!(uncommit(&w, std::slice::from_ref(&head), "0000000", "t3").unwrap_err().contains("HEAD moved"));

        sh(&w, "git switch -q -c side HEAD~1 && echo s > s.txt && git add -A && git commit -qm side && git switch -q feat && git merge -q --no-edit side");
        let head = oid(&w, "HEAD");
        let c3 = oid(&w, "HEAD~1~1");
        assert!(uncommit(&w, &[c3], &head, "t3").unwrap_err().contains("merge commit"));
        assert_eq!(oid(&w, "HEAD"), head, "0 change");
        drop(&w);
    }

    #[test]
    fn push_to_pushes_the_commit_and_the_older_unpushed_ones_only() {
        let w = repo("push");
        let c3 = oid(&w, "HEAD^");

        push_to(&w, &c3).unwrap();

        assert_eq!(oid(&w, "origin/feat"), c3);
        let cs = list(&w, &base(&w));
        assert_eq!(cs.list.iter().filter(|c| !c.pushed).count(), 1, "c4 stays local");
        drop(&w);
    }

    #[test]
    fn a_branch_that_tracks_main_pushes_to_its_own_name_never_to_main() {
        let w = repo("tracks-main");
        sh(&w, "git branch -q -u origin/main");
        let (main, c3) = (oid(&w, "origin/main"), oid(&w, "HEAD^"));

        assert_eq!(upstream(&w).as_deref(), Some("origin/feat"), "the same name, not @{{u}}");
        let cs = list(&w, &base(&w));
        assert_eq!(cs.list.iter().filter(|c| c.pushed).count(), 2, "c1, c2 on origin/feat: pushed");

        push_to(&w, &c3).unwrap();

        assert_eq!(oid(&w, "origin/main"), main, "main untouched");
        assert_eq!(oid(&w, "origin/feat"), c3);
        assert_eq!(out(&w, "git rev-parse --abbrev-ref @{u}"), "origin/feat", "tracks its own name now");
        drop(&w);
    }

    #[test]
    fn push_to_without_an_upstream_makes_the_pushed_branch_the_upstream() {
        let w = repo("no-upstream");
        sh(&w, "git branch -q --unset-upstream && git push -q origin --delete feat && git update-ref -d refs/remotes/origin/feat");
        let c3 = oid(&w, "HEAD^");

        assert!(upstream(&w).is_none());
        push_to(&w, &c3).unwrap();

        assert_eq!(oid(&w, "origin/feat"), c3);
        assert_eq!(out(&w, "git rev-parse --abbrev-ref @{u}"), "origin/feat");
        drop(&w);
    }

    #[test]
    fn a_staged_change_on_top_of_a_picked_commit_keeps_the_index() {
        let w = repo("index-kept");
        // c4.txt: added by c4, a staged change on top of it
        sh(&w, "echo more >> c4.txt && git add c4.txt");
        let head = oid(&w, "HEAD");
        let before = out(&w, "cat c4.txt");

        let u = uncommit(&w, &[head.clone(), head.clone()], &head, "t4").unwrap();

        assert_eq!(u.count, 1, "1 commit, picked twice");
        assert!(u.index_kept && u.note().contains("index kept"), "{}", u.note());
        assert_eq!(oid(&w, "HEAD"), oid(&w, &format!("{head}^")));
        assert_eq!(out(&w, "cat c4.txt"), before, "the worktree as before");
        assert_eq!(out(&w, "git diff --cached --name-only"), "c4.txt", "the old index: c4.txt staged as a whole");
        drop(&w);
    }

    #[test]
    fn uncommit_in_a_sparse_checkout_keeps_the_files_outside_the_cone() {
        let w = repo("sparse");
        sh(&w, "mkdir -p a b && echo a > a/x && echo b > b/y && git add -A && git commit -qm ab && echo c5 > a/c5 && git add -A && git commit -qm c5");
        sh(&w, "git sparse-checkout set --no-cone a");
        let head = oid(&w, "HEAD");

        uncommit(&w, std::slice::from_ref(&head), &head, "t5").unwrap();

        let status = out(&w, "git status --porcelain");
        assert!(!status.contains(" D "), "0 deletions outside the cone: {status}");
        assert_eq!(status, "?? a/c5");
        drop(&w);
    }

    #[test]
    fn show_signature_in_the_config_breaks_neither_the_list_nor_a_rewrite() {
        let w = repo("signature");
        // signed commits: `No signature` on stdout before each record (unsigned ones: 0 line)
        sh(&w, "ssh-keygen -q -t ed25519 -N '' -f ../k && git config gpg.format ssh && git config user.signingkey \"$PWD/../k\"");
        sh(&w, "git commit -q --amend -S --no-edit && git config log.showSignature true && echo c5 > c5.txt && git add -A && git commit -q -S -m c5");
        assert!(out(&w, "git log -1 --format=%H").starts_with("No signature"), "the config bites");
        let (head, c4) = (oid(&w, "HEAD"), oid(&w, "HEAD^"));

        let cs = list(&w, &base(&w));
        assert_eq!(cs.list[0].oid, head);

        uncommit(&w, std::slice::from_ref(&c4), &head, "t6").unwrap();

        assert_eq!(out(&w, "git -c log.showSignature=false log -1 --format=%an%n%B"), "t\nc5");
        assert_eq!(out(&w, "git cat-file commit HEAD | tail -c 4 | od -c | head -1 | tr -s ' '"), out(&w, &format!("git cat-file commit {head} | tail -c 4 | od -c | head -1 | tr -s ' '")), "1 trailing line break, as before");
        drop(&w);
    }

    #[test]
    fn push_to_refuses_a_commit_that_left_the_branch_and_another_name_upstream() {
        let w = repo("stale");
        let c4 = oid(&w, "HEAD");
        sh(&w, "git commit -q --amend -m 'c4 amended'");
        let feat = oid(&w, "origin/feat");

        assert!(push_to(&w, &c4).unwrap_err().contains("0 more on the branch"));
        assert_eq!(oid(&w, "origin/feat"), feat, "0 push");

        // fix tracks origin/hans/fix: 0 upstream of the same name
        sh(&w, "git push -q origin HEAD:refs/heads/hans/fix && git fetch -q && git switch -q -c fix && git branch -q -u origin/hans/fix");
        assert!(upstream(&w).is_none(), "{:?}", upstream(&w));
        drop(&w);
    }

    #[test]
    fn uncommit_refuses_the_root_commit() {
        let w = repo("root");
        sh(&w, "git branch -q --unset-upstream");
        let (head, root) = (oid(&w, "HEAD"), out(&w, "git rev-list --max-parents=0 HEAD"));

        assert!(uncommit(&w, &[root], &head, "t7").unwrap_err().contains("pushed"), "on origin/feat: pushed first");
        sh(&w, "git push -q origin --delete feat && git update-ref -d refs/remotes/origin/feat");
        let root = out(&w, "git rev-list --max-parents=0 HEAD");
        assert!(uncommit(&w, &[root], &head, "t7").unwrap_err().contains("root commit"));
        drop(&w);
    }

    #[test]
    fn a_remote_branch_deleted_after_the_merge_leaves_the_commits_unpushed() {
        let w = repo("gone");
        sh(&w, "git push -q origin --delete feat && git fetch -q --prune");

        assert!(upstream(&w).is_none(), "[gone]: 0 upstream");
        let cs = list(&w, &base(&w));
        assert!(cs.list.iter().all(|c| !c.pushed) && cs.list.len() == 4);
        drop(&w);
    }
}
