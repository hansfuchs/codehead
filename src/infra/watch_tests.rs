use super::*;
use std::process::Command;

fn watch_state() -> Watch {
    Watch {
        threads: "/d/threads".into(),
        gits: vec!["/r/.git".into(), "/r2/.git".into()],
        works: vec!["/wt".into(), "/r".into()],
        plain: HashSet::new(),
        ignores: HashMap::new(),
    }
}

/// Thread files by part, git metadata only, work tree paths to the ignore check.
#[test]
fn classify_maps_paths_to_topics() {
    let watch = watch_state();
    let thread = |id: &str, part| Class::Topic(Topic::Thread(id.into(), part));

    assert_eq!(
        watch.classify(Path::new("/d/threads/ab/thread.json")),
        thread("ab", Part::Meta)
    );
    assert_eq!(
        watch.classify(Path::new("/d/threads/ab/events.jsonl")),
        thread("ab", Part::Log)
    );
    assert_eq!(
        watch.classify(Path::new("/d/threads/ab/thread.tmp-12")),
        thread("ab", Part::Other)
    );

    let git = |part| Class::Topic(Topic::Git("/r/.git".into(), part));

    assert_eq!(
        watch.classify(Path::new("/r/.git/refs/heads/task/x")),
        git(Git::Branch("task/x".into()))
    );
    assert_eq!(
        watch.classify(Path::new("/r/.git/refs/remotes/origin/task/x")),
        git(Git::Remote("task/x".into()))
    );
    assert_eq!(
        watch.classify(Path::new("/r/.git/refs/remotes/origin/HEAD")),
        git(Git::All)
    );
    assert_eq!(
        watch.classify(Path::new("/r/.git/worktrees/wt/HEAD")),
        git(Git::Worktree("wt".into()))
    );
    assert_eq!(watch.classify(Path::new("/r/.git/worktrees/wt/ORIG_HEAD")), Class::Drop);
    assert_eq!(
        watch.classify(Path::new("/r/.git/index")),
        git(Git::Worktree(String::new()))
    );
    assert_eq!(watch.classify(Path::new("/r/.git/packed-refs")), git(Git::All));
    assert_eq!(watch.classify(Path::new("/r/.git/objects/ab/cdef")), Class::Drop);
    assert_eq!(watch.classify(Path::new("/r/.git/index.lock")), Class::Drop);
    assert_eq!(watch.classify(Path::new("/r/.git/logs/HEAD")), Class::Drop);

    assert_eq!(
        watch.classify(Path::new("/wt/src/a.rs")),
        Class::Work("/wt".into(), "src/a.rs".into())
    );
    assert_eq!(watch.classify(Path::new("/wt/.git")), Class::Drop);
    assert_eq!(watch.classify(Path::new("/elsewhere/x")), Class::Drop);
}

/// 1 check per new dir: an install storm in `node_modules` costs 1 check, then 0; a tracked
/// file edit publishes 1 `Work`.
#[test]
fn ignored_dirs_are_checked_once() {
    let mut watch = watch_state();
    let calls = std::cell::Cell::new(0);

    let mut check = |_: &Path, rels: &[PathBuf]| {
        calls.set(calls.get() + 1);
        rels.iter()
            .filter(|rel| rel.starts_with("node_modules"))
            .cloned()
            .collect::<HashSet<_>>()
    };

    let storm: Vec<PathBuf> = (0..1000)
        .map(|i| PathBuf::from(format!("/wt/node_modules/p{i}/index.js")))
        .collect();

    assert_eq!(watch.topics(&storm, &mut check), [] as [crate::infra::bus::Topic; 0]);
    assert_eq!(watch.topics(&storm, &mut check), [] as [crate::infra::bus::Topic; 0]);
    assert_eq!(
        watch.topics(&[PathBuf::from("/wt/src/a.rs")], &mut check),
        vec![Topic::Work("/wt".into())]
    );
    assert_eq!(
        watch.topics(&[PathBuf::from("/wt/src/a.rs")], &mut check),
        vec![Topic::Work("/wt".into())]
    );
    assert_eq!(calls.get(), 2, "node_modules once, src/a.rs once");

    // a .gitignore edit: verdicts again
    watch.topics(&[PathBuf::from("/wt/.gitignore")], &mut check);

    assert!(
        watch
            .ignores
            .get(Path::new("/wt"))
            .is_none_or(|verdicts| !verdicts.contains_key(Path::new("node_modules")))
    );
}

/// Git dirs of a main checkout and a linked worktree (common dir from `commondir`); the real
/// `git check-ignore` on a dir.
#[test]
fn git_dirs_and_check_ignore() {
    let base = std::env::temp_dir().join(format!("ch-watch-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);

    std::fs::create_dir_all(&base).unwrap();

    let sh = |script: &str| {
        assert!(
            Command::new("sh")
                .arg("-c")
                .arg(script)
                .current_dir(&base)
                .env("GIT_AUTHOR_NAME", "t")
                .env("GIT_AUTHOR_EMAIL", "t@t")
                .env("GIT_COMMITTER_NAME", "t")
                .env("GIT_COMMITTER_EMAIL", "t@t")
                .output()
                .unwrap()
                .status
                .success(),
            "{script}"
        );
    };

    sh(
        "git init -q -b main r && cd r && echo node_modules/ > .gitignore && mkdir -p node_modules src && git add .gitignore && git commit -q -m 1 && git worktree add -q -b feat ../w",
    );
    let repo = base.join("r");
    let (top, common, own) = git_dirs(&repo.join("src")).unwrap();

    assert_eq!(
        (top.as_path(), common.as_path(), own.as_path()),
        (repo.as_path(), repo.join(".git").as_path(), repo.join(".git").as_path())
    );

    let (top, common, own) = git_dirs(&base.join("w")).unwrap();

    assert_eq!(top, base.join("w"));
    assert_eq!(canon(&common), canon(&repo.join(".git")));
    assert_eq!(canon(&own), canon(&repo.join(".git/worktrees/w")));

    let ig = check_ignore(&repo, &["node_modules".into(), "src".into(), ".gitignore".into()]);

    assert_eq!(ig, HashSet::from([PathBuf::from("node_modules")]));

    std::fs::remove_dir_all(&base).unwrap();
}

/// A nested repo or worktree (a subagent's, inside the checkout) and temp files of atomic writes:
/// 0 `Work`; a repo that comes later (its `.git` event) replaces the dir's verdict.
#[test]
fn nested_repositories_and_temp_files_publish_nothing() {
    let base = std::env::temp_dir().join(format!("ch-watch-nested-{}", std::process::id()));
    let work = base.join("wt");
    let _ = std::fs::remove_dir_all(&base);

    std::fs::create_dir_all(work.join(".claude/worktrees/agent-1/src")).unwrap();
    std::fs::create_dir_all(work.join("vendor")).unwrap();
    std::fs::write(work.join(".claude/worktrees/agent-1/.git"), "gitdir: /elsewhere\n").unwrap();

    let mut watch = Watch {
        threads: "/d/threads".into(),
        gits: Vec::new(),
        works: vec![work.clone()],
        plain: HashSet::new(),
        ignores: HashMap::new(),
    };
    let mut topics = |rel: &str| watch.topics(&[work.join(rel)], &mut |_: &Path, _: &[PathBuf]| HashSet::new());
    let edit = vec![Topic::Work(work.clone())];

    assert_eq!(topics(".claude/worktrees/agent-1/src/x.rs"), []);
    assert_eq!(topics("src/a.rs.tmp.4821.0123456789ab"), []);
    assert_eq!(topics("src/.a.rs.swp"), []);
    assert_eq!(topics("src/a.rs"), edit);
    assert_eq!(topics("vendor/x.rs"), edit);

    std::fs::create_dir_all(work.join("vendor/.git")).unwrap();

    assert_eq!(topics("vendor/.git"), []);
    assert_eq!(topics("vendor/y.rs"), []);

    std::fs::remove_dir_all(&base).unwrap();
}
