use super::*;
use std::process::Stdio;

fn run_shell(dir: &Path, script: &str) {
    assert!(
        Command::new("sh")
            .arg("-c")
            .arg(script)
            .current_dir(dir)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@t")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success(),
        "{script}"
    );
}

fn temp_repo(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("codehead-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    std::fs::create_dir_all(&dir).unwrap();
    run_shell(
        &dir,
        "git init -q -b main && printf 'x\\n' > x.txt && printf 'y\\n' > y.txt && git add -A && git commit -qm init",
    );
    dir
}

/// (mtime, size, inode) of DIR's index.
fn index_stamp(dir: &Path) -> (std::time::SystemTime, u64, u64) {
    use std::os::unix::fs::MetadataExt;
    let metadata = std::fs::metadata(dir.join(".git/index")).unwrap();

    (metadata.modified().unwrap(), metadata.len(), metadata.ino())
}

/// The background read of VIEW: started, waited for, applied.
fn read_now(view: &mut View) {
    view.refresh(true);

    for _ in 0..300 {
        if view.poll() || view.loading.is_none() {
            return;
        }

        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    panic!("read: 0 end");
}

/// A subagent's worktree, a nested repo, temp files of atomic writes and editor swap files in
/// the checkout: 0 rows; the real new file: a row.
#[test]
fn nested_worktrees_and_temp_files_never_show() {
    let dir = temp_repo("read-strays");

    run_shell(
        &dir,
        "git worktree add -q -b sub .claude/worktrees/agent-1 && mkdir -p vendor/lib && git -C vendor/lib init -q \
         && printf 'a\\n' > x.txt.tmp.4821.0123456789ab && printf 'b\\n' > .new.md.swp && printf 'new\\n' > new.md",
    );
    let mut view = View::new("changes-read-strays", &dir);

    read_now(&mut view);
    let text = screen(&mut view, 140, 30).join("\n");
    let paths: Vec<&str> = view.changes.files.iter().map(|file| file.path.as_str()).collect();

    assert_eq!(paths, ["new.md"]);
    assert!(text.contains("new.md"), "{text}");

    for stray in [".claude", "vendor", ".tmp.", ".swp"] {
        assert!(!text.contains(stray), "{stray} drawn: {text}");
    }

    let _ = std::fs::remove_dir_all(dir);
}

/// A file with a new stat and its old text (a formatter's rewrite): the read writes 0 index
/// (`git diff` would: the watcher's topic, 1 more read), shows 0 row for it, and a staged file
/// with a new stat stays fully staged.
#[test]
fn a_read_writes_no_index() {
    let dir = temp_repo("read-index");

    run_shell(
        &dir,
        "printf 'y2\\n' > y.txt && git add y.txt && touch -t 202001010000 x.txt y.txt",
    );
    let before = index_stamp(&dir);
    let changes = load(&dir).unwrap();
    let after = index_stamp(&dir);
    let paths: Vec<&str> = changes.files.iter().map(|file| file.path.as_str()).collect();

    assert_eq!(before, after, "the read wrote the index");
    assert_eq!(paths, ["y.txt"]);
    assert_eq!(changes.staged, HashMap::from([("y.txt".to_string(), true)]));

    // the case is real: porcelain diff rewrites the index here
    std::thread::sleep(std::time::Duration::from_millis(20));
    run_shell(&dir, "git diff --quiet HEAD -- x.txt; git diff > /dev/null");

    assert_ne!(
        index_stamp(&dir),
        before,
        "git diff wrote 0 index: the case proves nothing"
    );

    let _ = std::fs::remove_dir_all(dir);
}

/// `diff-files --numstat -z`: the paths, binary files (`-`) too.
#[test]
fn numstat_paths_parse() {
    let paths = numstat_paths("1\t0\tsrc/a.rs\0-\t-\timg.png\0\0");

    assert_eq!(paths, HashSet::from(["src/a.rs".to_string(), "img.png".to_string()]));
}
