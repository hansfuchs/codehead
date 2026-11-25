use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// `sh -c C` in DIR (must pass).
pub(crate) fn sh(dir: &Path, character: &str) {
    let ok = Command::new("sh")
        .arg("-c")
        .arg(character)
        .current_dir(dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap()
        .success();

    assert!(ok, "{character}");
}

/// `sh -c C` in DIR: its stdout, trimmed.
pub(crate) fn out(dir: &Path, character: &str) -> String {
    let output_value = Command::new("sh")
        .arg("-c")
        .arg(character)
        .current_dir(dir)
        .output()
        .unwrap();

    String::from_utf8_lossy(&output_value.stdout).trim().to_string()
}

/// A checkout on branch `feat`: `init` on origin/main; c1, c2 pushed (origin/feat); c3, c4
/// local. c1-c4 each add `<name>.txt`.
pub(crate) fn repo(name: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!("codehead-commits-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);

    std::fs::create_dir_all(directory.join("w")).unwrap();
    sh(&directory, "git init -q --bare -b main origin.git");
    let width = directory.join("w");

    sh(
        &width,
        "git init -q -b main && git config user.email t@t && git config user.name t && git config commit.gpgsign false && git config core.hooksPath /dev/null",
    );
    sh(
        &width,
        "seq 1 5 > x.txt && echo y > y.txt && git add -A && git commit -qm init && git remote add origin ../origin.git && git push -q -u origin main",
    );
    sh(&width, "git switch -q -c feat");

    for character in ["c1", "c2"] {
        sh(
            &width,
            &format!(
                "echo {character} > {character}.txt && git add -A && git commit -qm '{character} subject' -m '{character} body'"
            ),
        );
    }

    sh(&width, "git push -q -u origin feat");

    for character_value in ["c3", "c4"] {
        sh(
            &width,
            &format!(
                "echo {character_value} > {character_value}.txt && git add -A && git commit -qm '{character_value} subject' -m '{character_value} body'"
            ),
        );
    }

    width
}

/// The oid of REV in DIR.
pub(crate) fn oid(dir: &Path, rev: &str) -> String {
    out(dir, &format!("git rev-parse {rev}"))
}

pub(crate) fn drop(dir: &Path) {
    if let Some(path) = dir.parent() {
        let _ = std::fs::remove_dir_all(path);
    }
}
