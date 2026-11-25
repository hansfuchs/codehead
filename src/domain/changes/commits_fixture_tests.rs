use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// `sh -c C` in DIR (must pass).
pub(crate) fn sh(dir: &Path, script: &str) {
    let ok = Command::new("sh")
        .arg("-c")
        .arg(script)
        .current_dir(dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap()
        .success();

    assert!(ok, "{script}");
}

/// `sh -c C` in DIR: its stdout, trimmed.
pub(crate) fn out(dir: &Path, script: &str) -> String {
    let output = Command::new("sh")
        .arg("-c")
        .arg(script)
        .current_dir(dir)
        .output()
        .unwrap();

    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// A checkout on branch `feat`: `init` on origin/main; c1, c2 pushed (origin/feat); c3, c4
/// local. c1-c4 each add `<name>.txt`.
pub(crate) fn repo(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("codehead-commits-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);

    std::fs::create_dir_all(root.join("w")).unwrap();
    sh(&root, "git init -q --bare -b main origin.git");
    let work = root.join("w");

    sh(
        &work,
        "git init -q -b main && git config user.email t@t && git config user.name t && git config commit.gpgsign false && git config core.hooksPath /dev/null",
    );
    sh(
        &work,
        "seq 1 5 > x.txt && echo y > y.txt && git add -A && git commit -qm init && git remote add origin ../origin.git && git push -q -u origin main",
    );
    sh(&work, "git switch -q -c feat");

    for commit in ["c1", "c2"] {
        sh(
            &work,
            &format!(
                "echo {commit} > {commit}.txt && git add -A && git commit -qm '{commit} subject' -m '{commit} body'"
            ),
        );
    }

    sh(&work, "git push -q -u origin feat");

    for commit in ["c3", "c4"] {
        sh(
            &work,
            &format!(
                "echo {commit} > {commit}.txt && git add -A && git commit -qm '{commit} subject' -m '{commit} body'"
            ),
        );
    }

    work
}

/// The oid of REV in DIR.
pub(crate) fn oid(dir: &Path, rev: &str) -> String {
    out(dir, &format!("git rev-parse {rev}"))
}

pub(crate) fn drop(dir: &Path) {
    if let Some(root) = dir.parent() {
        let _ = std::fs::remove_dir_all(root);
    }
}
