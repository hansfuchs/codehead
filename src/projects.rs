//! Projects: 1 brief (`<slug>.md`) per project, per repo. A thread belongs to 0..n projects of
//! its repo (`store::Thread::projects`); the thread lists filter by them, the agents read the
//! briefs. The owner makes projects (projects tab); an agent only assigns existing ones.
//!
//! Where: the home repo, the repo's dir there (the `Notes:` dir of the repo's line in the home
//! repo's AGENTS.md, else `<home>/<repo name>/`), its `projects/`. 0 home repo:
//! `<data dir>/projects/<repo name>/`.
//!
//! A brief: an optional front matter (`title:`, `done: true`), then free text (goal, decisions,
//! open points). 0 title: the slug.
//!
//! Naming (the agents' `set_title`): title `area: outcome`, branch `<kind>/<area>-<slug>`. Kind
//! and area of a thread: from its branch / title (thread list filters `kind:fix`, `area:menu`).

use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Project {
    pub slug: String,
    pub title: String,
    pub done: bool,
    /// The brief's 1st line of text (after the front matter, 0 heading marks).
    pub goal: String,
    pub path: PathBuf,
}

/// The projects dir of REPO (a main checkout).
pub fn dir(repo: &Path) -> PathBuf {
    let home = if cfg!(test) { None } else { crate::config::home_repo() };
    dir_in(home.as_deref(), &crate::engine::data_dir(), repo)
}

/// The projects dir of REPO with home repo HOME, data dir DATA.
pub fn dir_in(home: Option<&Path>, data: &Path, repo: &Path) -> PathBuf {
    let name = repo_name(repo);
    match home {
        Some(h) => {
            let agents = std::fs::read_to_string(h.join("AGENTS.md")).unwrap_or_default();
            let sub = crate::home::notes_of(&agents, repo, &name).unwrap_or_else(|| name.clone());
            h.join(sub.trim_end_matches('/')).join("projects")
        }
        None => data.join("projects").join(name),
    }
}

fn repo_name(repo: &Path) -> String {
    repo.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "repo".into())
}

/// The projects of REPO: active first, then A→Z.
pub fn list(repo: &Path) -> Vec<Project> {
    list_in(&dir(repo))
}

pub fn list_in(dir: &Path) -> Vec<Project> {
    let mut v: Vec<Project> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "md"))
        .filter_map(|p| read(&p))
        .collect();
    v.sort_by(|a, b| a.done.cmp(&b.done).then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase())));
    v
}

/// The brief at PATH.
pub fn read(path: &Path) -> Option<Project> {
    let slug = path.file_stem()?.to_string_lossy().into_owned();
    let text = std::fs::read_to_string(path).ok()?;
    let (title, done, goal) = parse(&text);
    Some(Project { title: title.unwrap_or_else(|| slug.clone()), slug, done, goal, path: path.to_path_buf() })
}

/// Title, done, goal of a brief's TEXT.
pub fn parse(text: &str) -> (Option<String>, bool, String) {
    let (front, body) = split_front(text);
    let mut title = None;
    let mut done = false;
    for l in front.lines() {
        if let Some((k, v)) = l.split_once(':') {
            let v = v.trim().trim_matches('"').trim_matches('\'');
            match k.trim() {
                "title" if !v.is_empty() => title = Some(v.to_string()),
                "done" => done = v == "true" || v == "yes",
                _ => {}
            }
        }
    }
    let goal = body.lines().map(|l| l.trim().trim_start_matches('#').trim()).find(|l| !l.is_empty()).unwrap_or_default().to_string();
    (title, done, goal)
}

/// (front matter, body) of TEXT: 0 front matter → ("", TEXT).
fn split_front(text: &str) -> (&str, &str) {
    let Some(rest) = text.strip_prefix("---\n").or_else(|| text.strip_prefix("---\r\n")) else { return ("", text) };
    match rest.find("\n---") {
        Some(i) => {
            let after = &rest[i + 4..];
            (&rest[..i], after.strip_prefix('\n').or_else(|| after.strip_prefix("\r\n")).unwrap_or(after))
        }
        None => ("", text),
    }
}

/// TEXT with front matter key K = V (None: K out). 0 front matter: a new one.
pub fn set_front(text: &str, k: &str, v: Option<&str>) -> String {
    let (front, body) = split_front(text);
    let mut lines: Vec<String> = front.lines().filter(|l| l.split_once(':').is_none_or(|(x, _)| x.trim() != k)).map(String::from).collect();
    if let Some(v) = v {
        lines.push(format!("{k}: {v}"));
    }
    if lines.is_empty() {
        return body.to_string();
    }
    format!("---\n{}\n---\n{body}", lines.join("\n"))
}

/// The brief body of TEXT: after the front matter, 0 blank lines on top.
pub fn body_of(text: &str) -> String {
    split_front(text).1.trim_start_matches(['\n', '\r']).to_string()
}

/// TEXT with title TITLE, body BODY: the front matter's other keys (`done:`) stay.
pub fn merge(text: &str, title: &str, body: &str) -> String {
    let (front, _) = split_front(text);
    let mut body = body.trim_start_matches(['\n', '\r']).to_string();
    if !body.ends_with('\n') {
        body.push('\n');
    }
    let base = if front.is_empty() { format!("\n{body}") } else { format!("---\n{front}\n---\n\n{body}") };
    set_front(&base, "title", Some(title.trim()))
}

/// The default body of a new brief.
pub const TEMPLATE: &str = "Goal: \n\n## Decisions\n\n## Open\n";

/// A new project TITLE in DIR. Err: 0 title, the slug exists, a write error.
pub fn create_in(dir: &Path, title: &str) -> Result<Project, String> {
    let title = title.trim();
    let slug = crate::notes::slug(title);
    if slug.is_empty() {
        return Err("title: 1 letter or digit at least".into());
    }
    let path = dir.join(format!("{slug}.md"));
    if path.exists() {
        return Err(format!("{slug}: exists"));
    }
    let text = format!("---\ntitle: {title}\n---\n\n{TEMPLATE}");
    crate::engine::write_atomic(&path, text.as_bytes()).map_err(|e| format!("{}: {e}", path.display()))?;
    read(&path).ok_or_else(|| format!("{}: unreadable", path.display()))
}

/// Project P done / active again.
pub fn set_done(p: &Project, done: bool) -> Result<(), String> {
    let text = std::fs::read_to_string(&p.path).map_err(|e| format!("{}: {e}", p.path.display()))?;
    let text = set_front(&text, "done", done.then_some("true"));
    crate::engine::write_atomic(&p.path, text.as_bytes()).map_err(|e| format!("{}: {e}", p.path.display()))
}

/// The slugs of SLUGS that are projects of REPO (any state), in the order given, 0 dupes. Err:
/// the unknown ones, with the known active ones.
pub fn resolve(repo: &Path, slugs: &[String]) -> Result<Vec<String>, String> {
    resolve_in(&list(repo), slugs)
}

pub fn resolve_in(all: &[Project], slugs: &[String]) -> Result<Vec<String>, String> {
    let mut out: Vec<String> = Vec::new();
    let mut bad = Vec::new();
    for s in slugs {
        let s = s.trim();
        match all.iter().find(|p| p.slug == s || p.title.eq_ignore_ascii_case(s)) {
            Some(p) if !out.contains(&p.slug) => out.push(p.slug.clone()),
            Some(_) => {}
            None => bad.push(s.to_string()),
        }
    }
    if bad.is_empty() {
        return Ok(out);
    }
    let known: Vec<&str> = all.iter().filter(|p| !p.done).map(|p| p.slug.as_str()).collect();
    Err(format!(
        "unknown project(s): {}. projects of this repo: {}. only the owner makes projects.",
        bad.join(", "),
        if known.is_empty() { "0".to_string() } else { known.join(", ") }
    ))
}

// ---------- naming ----------

/// Kinds of work: the branch prefix of an agent-named branch.
pub const KINDS: &[&str] = &["feat", "fix", "refactor", "perf", "docs", "test", "chore", "style", "ci", "build"];

/// Words that name no outcome: 0 title of only these.
const VAGUE: &[&str] = &["misc", "misc.", "improvements", "improvement", "changes", "updates", "update", "fixes", "stuff", "wip", "work", "various", "tweaks", "cleanup", "more"];

/// The kind of BRANCH: its prefix (`fix/…` → fix) when a kind.
pub fn kind_of(branch: &str) -> Option<&'static str> {
    let (k, _) = branch.split_once('/')?;
    KINDS.iter().copied().find(|x| *x == k)
}

/// The area of TITLE: its `area:` prefix (1 word: letters, digits, `-`, `_`, `/`, `.`).
pub fn area_of(title: &str) -> Option<String> {
    let (a, rest) = title.lines().next()?.split_once(':')?;
    let a = a.trim();
    let word = !a.is_empty() && a.len() <= 24 && a.chars().all(|c| c.is_ascii_alphanumeric() || "-_/.".contains(c));
    (word && !rest.trim().is_empty()).then(|| a.to_lowercase())
}

/// TITLE as the naming rules want it: `area: outcome`, max. 72 chars, an outcome, 0 `#n`.
pub fn check_title(title: &str) -> Result<(), String> {
    let t = title.trim();
    let format = "format: `area: outcome`, e.g. `menu: live quick commands`";
    if t.is_empty() || t.contains('\n') {
        return Err(format!("title: 1 line. {format}"));
    }
    if t.chars().count() > 72 {
        return Err(format!("title: max. 72 chars. {format}"));
    }
    let Some(area) = area_of(t) else { return Err(format!("title: 0 area prefix. {format}")) };
    let outcome = t.split_once(':').map(|x| x.1.trim()).unwrap_or_default();
    let words: Vec<String> = outcome.split_whitespace().map(|w| w.to_lowercase().trim_matches(|c: char| !c.is_alphanumeric()).to_string()).filter(|w| !w.is_empty()).collect();
    if outcome.contains('#') {
        return Err(format!("title: 0 `#n` numbering. {format}"));
    }
    if words.is_empty() || words.iter().all(|w| VAGUE.contains(&w.as_str()) || w.chars().all(|c| c.is_ascii_digit())) {
        return Err(format!("title: `{outcome}` names 0 outcome (area `{area}`). say what changed. {format}"));
    }
    Ok(())
}

/// BRANCH as the naming rules want it: `<kind>/<slug>`, slug `a-z 0-9 -`, max. 60 chars.
pub fn check_branch(branch: &str) -> Result<(), String> {
    let format = format!("format: `<kind>/<area>-<slug>`, kind: {}", KINDS.join(" "));
    let Some((k, slug)) = branch.split_once('/') else { return Err(format!("branch: 0 kind prefix. {format}")) };
    if !KINDS.contains(&k) {
        return Err(format!("branch: kind `{k}` unknown. {format}"));
    }
    if slug.is_empty() || branch.len() > 60 || !slug.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') || slug.starts_with('-') || slug.ends_with('-') || slug.contains("--") {
        return Err(format!("branch: slug `{slug}`: a-z, 0-9, single `-`, max. 60 chars in all. {format}"));
    }
    Ok(())
}

/// Branches an agent never renames.
fn protected(b: &str, default: Option<&str>) -> bool {
    matches!(b, "main" | "master" | "develop" | "trunk") || Some(b) == default
}

/// Rename branch OLD of checkout DIR to NEW: local (`git branch -m`), the worktree's pin, the
/// remote branch when OLD has an upstream (GitHub: its rename API, open PRs follow; else NEW
/// pushed, OLD kept there). Ok: what changed.
pub fn rename_branch(dir: &Path, old: &str, new: &str) -> Result<String, String> {
    use crate::threads::git;
    if old == new {
        return Ok("branch unchanged".into());
    }
    let def = git(dir, &["symbolic-ref", "--quiet", "--short", "refs/remotes/origin/HEAD"]).1.trim().trim_start_matches("origin/").to_string();
    if protected(old, Some(def.as_str()).filter(|d| !d.is_empty())) {
        return Err(format!("branch {old}: the default branch, 0 rename"));
    }
    if git(dir, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{new}")]).0 {
        return Err(format!("branch {new}: exists"));
    }
    let up = git(dir, &["rev-parse", "--abbrev-ref", "--symbolic-full-name", &format!("{old}@{{upstream}}")]);
    let upstream = up.0.then(|| up.1.trim().to_string()).filter(|u| !u.is_empty());
    let (ok, _, e) = git(dir, &["branch", "-m", old, new]);
    if !ok {
        return Err(format!("git branch -m {old} {new}: {}", e.trim()));
    }
    let mut done = vec![format!("branch {old} → {new}")];
    // the worktree's pin: the guard reads the branch name from the git dir
    let gitdir = PathBuf::from(git(dir, &["rev-parse", "--absolute-git-dir"]).1.trim());
    if gitdir.join("codehead-branch").is_file()
        && let Err(e) = crate::branchpin::pin(dir, new)
    {
        done.push(format!("pin: {e}"));
    }
    let Some(upstream) = upstream else { return Ok(done.join(" · ")) };
    let remote = upstream.split_once('/').map(|x| x.0.to_string()).unwrap_or_else(|| "origin".into());
    let remote_old = upstream.split_once('/').map(|x| x.1.to_string()).unwrap_or_else(|| old.to_string());
    let gh = std::process::Command::new("gh")
        .args(["api", "-X", "POST", &format!("repos/{{owner}}/{{repo}}/branches/{remote_old}/rename"), "-f", &format!("new_name={new}")])
        .current_dir(dir)
        .stdin(std::process::Stdio::null())
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if gh {
        let _ = git(dir, &["fetch", "--quiet", "--prune", &remote]);
        let (ok, _, e) = git(dir, &["branch", "--set-upstream-to", &format!("{remote}/{new}"), new]);
        done.push(if ok { format!("{remote}: renamed (GitHub), PRs follow") } else { format!("{remote}: renamed (GitHub); upstream: {}", e.trim()) });
    } else {
        let (ok, _, e) = git(dir, &["push", "--quiet", "-u", &remote, new]);
        if !ok {
            return Err(format!("{} · git push -u {remote} {new}: {}", done.join(" · "), e.trim()));
        }
        done.push(format!("{remote}: {new} pushed, {remote_old} kept there (a PR on it: still open)"));
    }
    Ok(done.join(" · "))
}

// ---------- the agents' tools ----------

/// The repo of thread T (its main checkout).
fn repo_of(t: &crate::engine::store::Thread) -> Option<PathBuf> {
    crate::threads::repo_of_thread_store(t)
}

/// `set_title`: thread ID's title TITLE, its branch BRANCH (a codehead worktree's own branch
/// only). An owner-written title stays. Ok: what changed.
pub fn set_title(id: &str, title: &str, branch: Option<&str>) -> Result<String, String> {
    use crate::engine::store;
    let t = store::load(id).ok_or_else(|| format!("no thread {id}"))?;
    let title = title.trim();
    check_title(title)?;
    let branch = branch.map(str::trim).filter(|b| !b.is_empty());
    if let Some(b) = branch {
        check_branch(b)?;
    }
    let mut done = Vec::new();
    let new_title = if t.title_set {
        done.push("title kept: the owner wrote it".to_string());
        None
    } else if t.title.lines().next() == Some(title) {
        None
    } else {
        done.push(format!("title: {title}"));
        Some(title.to_string())
    };
    let mut new_branch = None;
    match (branch, &t.worktree, &t.branch) {
        (Some(b), Some(_), Some(old)) if b != old => {
            done.push(rename_branch(&t.work_dir(), old, b)?);
            new_branch = Some((old.clone(), b.to_string()));
        }
        (Some(_), None, _) => done.push("branch kept: the thread works in an existing checkout (0 codehead worktree)".into()),
        _ => {}
    }
    store::update(id, |s| {
        if let Some(n) = &new_title {
            s.title = n.clone();
        }
        if let Some((old, new)) = &new_branch {
            s.branch = Some(new.clone());
            for c in s.checkouts.iter_mut().filter(|c| c.branch.as_deref() == Some(old.as_str())) {
                c.branch = Some(new.clone());
            }
        }
    })?;
    Ok(if done.is_empty() { "0 changes".into() } else { done.join(" · ") })
}

/// `set_projects`: thread ID's projects SLUGS (existing projects of its repo only). Owner-picked
/// projects stay. Ok: what changed.
pub fn set_projects(id: &str, slugs: &[String]) -> Result<String, String> {
    use crate::engine::store;
    let t = store::load(id).ok_or_else(|| format!("no thread {id}"))?;
    if t.projects_set {
        return Ok(format!("projects kept: the owner picked them ({})", if t.projects.is_empty() { "0".into() } else { t.projects.join(", ") }));
    }
    let repo = repo_of(&t).ok_or("the thread has 0 git repo: 0 projects")?;
    let v = resolve(&repo, slugs)?;
    store::update(id, |s| s.projects = v.clone())?;
    Ok(format!("projects: {}", if v.is_empty() { "0".into() } else { v.join(", ") }))
}

/// The system prompt block on naming and projects for thread T.
pub fn prompt(t: &crate::engine::store::Thread) -> String {
    let mut s = String::from(
        "\n\n# Naming\n\
Name the thread after what it does, with the `set_title` tool of the `codehead` MCP server: after your 1st turn's work, \
again when the work changes direction (not every turn). Title: `area: outcome`, imperative, e.g. `menu: live quick commands`; \
never vague (`misc. improvements #8`).",
    );
    if t.worktree.is_some() {
        s.push_str(&format!(
            " Branch (`branch` of `set_title`, this thread's worktree only): `<kind>/<area>-<slug>`, kind: {}, e.g. `feat/menu-live-commands`; \
codehead renames it, also on the remote.",
            KINDS.join(" ")
        ));
    }
    if t.title_set {
        s.push_str(" The owner wrote the title: it stays; `set_title` still names the branch.");
    }
    let Some(repo) = repo_of(t) else { return s };
    let all = list(&repo);
    let active: Vec<&Project> = all.iter().filter(|p| !p.done).collect();
    if active.is_empty() && t.projects.is_empty() {
        return s;
    }
    s.push_str(&format!("\n\n# Projects\nProjects of this repo (briefs in `{}`):", dir(&repo).display()));
    for p in &active {
        s.push_str(&format!("\n- `{}`: {}{}", p.slug, p.title, if p.goal.is_empty() { String::new() } else { format!(" ({})", p.goal) }));
    }
    if t.projects.is_empty() {
        if !t.projects_set {
            s.push_str("\nThis thread: 0 projects yet. Once its work is clear, assign the fitting ones (0..n) with `set_projects`. Never make a project: only the owner does.");
        }
    } else {
        s.push_str(&format!("\nThis thread: {}. Read their briefs before work.", t.projects.join(", ")));
        if !t.projects_set {
            s.push_str(" The work moved to other projects: `set_projects`.");
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn briefs_parse_title_done_goal() {
        let (t, d, g) = parse("---\ntitle: UX improvements\ndone: true\n---\n\n# Goal\n\nless noise\n");
        assert_eq!((t.as_deref(), d, g.as_str()), (Some("UX improvements"), true, "Goal"));
        let (t, d, g) = parse("plain text\nmore");
        assert_eq!((t, d, g.as_str()), (None, false, "plain text"));
        let text = set_front("---\ntitle: X\n---\nbody\n", "done", Some("true"));
        assert_eq!(text, "---\ntitle: X\ndone: true\n---\nbody\n");
        assert_eq!(set_front(&text, "done", None), "---\ntitle: X\n---\nbody\n");
        assert_eq!(set_front("body\n", "done", Some("true")), "---\ndone: true\n---\nbody\n");
    }

    #[test]
    fn title_and_body_merge_into_the_brief() {
        let old = "---\ntitle: Old\ndone: true\n---\n\nGoal: x\n";
        assert_eq!(body_of(old), "Goal: x\n");
        assert_eq!(merge(old, " New ", "Goal: y"), "---\ndone: true\ntitle: New\n---\n\nGoal: y\n");
        assert_eq!(merge("", "T", "b\n"), "---\ntitle: T\n---\n\nb\n");
    }

    #[test]
    fn projects_live_in_the_home_repo_under_the_repos_dir() {
        let base = std::env::temp_dir().join(format!("ch-proj-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let home = base.join("home");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(home.join("AGENTS.md"), "## Other repos\n\n- platform: core. `/abs/platform`. Notes: `notes/platform/`.\n").unwrap();
        assert_eq!(dir_in(Some(&home), &base, Path::new("/abs/platform")), home.join("notes/platform/projects"));
        assert_eq!(dir_in(Some(&home), &base, Path::new("/x/codehead")), home.join("codehead/projects"));
        assert_eq!(dir_in(None, &base.join("data"), Path::new("/x/codehead")), base.join("data/projects/codehead"));

        let d = home.join("codehead/projects");
        let p = create_in(&d, "Website refinement").unwrap();
        assert_eq!((p.slug.as_str(), p.title.as_str(), p.done), ("website-refinement", "Website refinement", false));
        assert!(create_in(&d, "website refinement").is_err(), "same slug");
        create_in(&d, "Bug fixes").unwrap();
        set_done(&p, true).unwrap();
        let all = list_in(&d);
        assert_eq!(all.iter().map(|p| (p.slug.as_str(), p.done)).collect::<Vec<_>>(), [("bug-fixes", false), ("website-refinement", true)]);
        assert_eq!(resolve_in(&all, &["Bug fixes".into(), "website-refinement".into(), "bug-fixes".into()]).unwrap(), ["bug-fixes", "website-refinement"]);
        let e = resolve_in(&all, &["ux".into()]).unwrap_err();
        assert!(e.contains("ux") && e.contains("bug-fixes") && !e.contains("website-refinement"), "{e}");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn naming_rules() {
        assert!(check_title("menu: live quick commands").is_ok());
        assert!(check_title("live quick commands").is_err(), "0 area");
        assert!(check_title("misc: improvements #8").is_err());
        assert!(check_title("ui: misc. improvements").is_err());
        assert!(check_title("ui: tweaks 2").is_err());
        assert!(check_title(&format!("ui: {}", "x".repeat(80))).is_err());
        assert!(check_branch("feat/menu-live-commands").is_ok());
        assert!(check_branch("task/menu-live").is_err());
        assert!(check_branch("feat/Menu_live").is_err());
        assert!(check_branch("feat/menu--live").is_err());
        assert_eq!(kind_of("fix/nav-crash"), Some("fix"));
        assert_eq!(kind_of("task/x"), None);
        assert_eq!(area_of("menu: live quick commands").as_deref(), Some("menu"));
        assert_eq!(area_of("fix the menu"), None);
        assert_eq!(area_of("a b: c"), None);
    }

    #[test]
    fn rename_branch_moves_the_branch_and_its_upstream() {
        use std::process::{Command, Stdio};
        let base = std::env::temp_dir().join(format!("ch-rename-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let sh = |c: &str| assert!(Command::new("sh").arg("-c").arg(c).current_dir(&base).stdout(Stdio::null()).stderr(Stdio::null()).status().unwrap().success(), "{c}");
        sh("git init -q --bare -b main remote.git && git clone -q remote.git r 2>/dev/null; cd r && git -c user.email=t@t -c user.name=t commit -q --allow-empty -m 1 && git push -q origin main && git worktree add -q -b task/x ../w && cd ../w && git push -q -u origin task/x");
        let wt = base.join("w");
        assert!(rename_branch(&wt, "main", "feat/y").is_err(), "default branch");
        let out = rename_branch(&wt, "task/x", "feat/menu-y").unwrap();
        assert!(out.contains("task/x → feat/menu-y"), "{out}");
        assert_eq!(crate::threads::current_branch(&wt), "feat/menu-y");
        // 0 GitHub: the new name pushed, its upstream set
        let up = crate::threads::git(&wt, &["rev-parse", "--abbrev-ref", "feat/menu-y@{upstream}"]).1;
        assert_eq!(up.trim(), "origin/feat/menu-y");
        assert!(rename_branch(&wt, "feat/menu-y", "feat/menu-y").unwrap().contains("unchanged"));
        let _ = std::fs::remove_dir_all(&base);
    }
}
