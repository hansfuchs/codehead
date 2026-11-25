use super::*;
use std::process::{Command, Stdio};

const DIFF: &str = "diff --git a/x b/x
--- a/x
+++ b/x
@@ -1,6 +1,6 @@
 a
-b
+B
 c
-d
+D
 e
 f
";

fn adds(lines: &[u32]) -> Pick {
    Pick {
        adds: lines.iter().copied().collect(),
        ..Default::default()
    }
}

#[test]
fn forward_keeps_the_picked_add_drops_the_rest() {
    // B alone: `-b` stays (context), `-d`, `+D`: d stays
    let selected = Pick {
        adds: [2].into(),
        dels: [(2, "b".into())].into(),
        all: false,
    };
    let out = pick(DIFF, &selected, Dir::Forward).unwrap();

    assert_eq!(
        out,
        "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1,6 +1,6 @@\n a\n-b\n+B\n c\n d\n e\n f\n"
    );

    // only the add of D: an insertion (d stays)
    let out = pick(DIFF, &adds(&[4]), Dir::Forward).unwrap();

    assert!(out.contains("@@ -1,6 +1,7 @@\n a\n b\n c\n d\n+D\n e\n f\n"), "{out}");
    assert!(pick(DIFF, &adds(&[9]), Dir::Forward).is_none());
}

#[test]
fn reverse_keeps_the_worktree_side() {
    // take B out (back to b): the rest (D) stays as context
    let selected = Pick {
        adds: [2].into(),
        dels: [(2, "b".into())].into(),
        all: false,
    };
    let out = pick(DIFF, &selected, Dir::Reverse).unwrap();

    assert!(out.ends_with("@@ -1,6 +1,6 @@\n a\n-b\n+B\n c\n D\n e\n f\n"), "{out}");
}

#[test]
fn a_new_file_part_and_whole() {
    let diff = "diff --git a/n b/n\nnew file mode 100755\n--- /dev/null\n+++ b/n\n@@ -0,0 +1,3 @@\n+1\n+2\n+3\n";
    let forward = pick(diff, &adds(&[2]), Dir::Forward).unwrap();

    assert_eq!(
        forward,
        "diff --git a/n b/n\nnew file mode 100755\n--- /dev/null\n+++ b/n\n@@ -0,0 +1,1 @@\n+2\n"
    );

    // reverse, line 2: the file stays, 1 and 3 are its context
    let reverse = pick(diff, &adds(&[2]), Dir::Reverse).unwrap();

    assert!(
        reverse.ends_with("--- a/n\n+++ b/n\n@@ -1,2 +1,3 @@\n 1\n+2\n 3\n"),
        "{reverse}"
    );

    let all = pick(
        diff,
        &Pick {
            all: true,
            ..Default::default()
        },
        Dir::Reverse,
    )
    .unwrap();

    assert!(
        all.contains("new file mode 100755\n--- /dev/null\n+++ b/n\n@@ -0,0 +1,3 @@"),
        "{all}"
    );
}

fn sh(directory: &std::path::Path, script: &str) -> String {
    let output = Command::new("sh")
        .arg("-c")
        .arg(script)
        .current_dir(directory)
        .stderr(Stdio::piped())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{script}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn apply(directory: &std::path::Path, patch: &str, args: &str) {
    let file = directory.join("p.diff");

    std::fs::write(&file, patch).unwrap();
    sh(directory, &format!("git apply {args} p.diff && rm p.diff"));
}

#[test]
fn real_git_takes_the_patches() {
    let directory = std::env::temp_dir().join(format!("codehead-patch-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);

    std::fs::create_dir_all(&directory).unwrap();
    sh(
        &directory,
        "git init -q && git config user.email t@t && git config user.name t && seq 1 30 > x && git add x && git commit -qm i",
    );

    // 2 hunks: 3 → three, 4 → four; 25 removed, 26 → twenty-six
    sh(
        &directory,
        "seq 1 30 | sed 's/^3$/three/;s/^4$/four/;/^25$/d;s/^26$/twenty-six/' > x",
    );
    let diff = sh(&directory, "git diff -- x");

    // stage `four` (its removed `4` too) and the removal of 25 (it sits before new line 25)
    let stage = Pick {
        adds: [4].into(),
        dels: [(3, "4".into()), (25, "25".into())].into(),
        all: false,
    };

    apply(&directory, &pick(&diff, &stage, Dir::Forward).unwrap(), "--cached");
    let cached = sh(&directory, "git diff --cached -U0 -- x");

    assert!(
        cached.contains("-4\n+four")
            && cached.contains("-25")
            && !cached.contains("three")
            && !cached.contains("twenty"),
        "{cached}"
    );

    // stash `three` (back to 3) in the worktree; the patch puts it back
    let diff = sh(&directory, "git diff HEAD -- x");
    let unstash = Pick {
        adds: [3].into(),
        dels: [(3, "3".into())].into(),
        all: false,
    };
    let stash = pick(&diff, &unstash, Dir::Reverse).unwrap();

    apply(&directory, &stash, "-R");
    let now = std::fs::read_to_string(directory.join("x")).unwrap();

    assert!(
        now.contains("\n3\nfour\n") && now.contains("twenty-six") && !now.contains("\n25\n"),
        "{now}"
    );

    apply(&directory, &stash, "");

    assert!(
        std::fs::read_to_string(directory.join("x"))
            .unwrap()
            .contains("\nthree\nfour\n")
    );

    // an untracked file: a part of it staged
    sh(&directory, "printf 'a\\nb\\nc\\n' > n");
    let output = Command::new("git")
        .args(["diff", "--no-index", "--", "/dev/null", "n"])
        .current_dir(&directory)
        .output()
        .unwrap();
    let diff = String::from_utf8_lossy(&output.stdout);

    apply(&directory, &pick(&diff, &adds(&[2]), Dir::Forward).unwrap(), "--cached");

    assert_eq!(sh(&directory, "git show :n"), "b\n");

    let _ = std::fs::remove_dir_all(directory);
}
