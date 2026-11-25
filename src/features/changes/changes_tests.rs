use super::*;
use std::fmt::Write as _;

fn view(mut files: Vec<FileDiff>) -> View {
    order(&mut files);
    let mut view_value = View::new("changes-test-0000", Path::new("/tmp"));

    view_value.marks.clear();
    view_value.scm.shown = false;
    view_value.changes = Changes {
        base: "origin/main".into(),
        files,
        ..Default::default()
    };
    view_value.loaded = Some(Instant::now());

    view_value
}

fn text(view: &View) -> Vec<String> {
    (0..view.layout.rows.len()).map(|i| text_of(view, i)).collect()
}

fn summary(view: &View) -> String {
    view.summary().spans.iter().map(|text| text.content.as_ref()).collect()
}

fn text_of(view: &View, i: usize) -> String {
    view.line(i).spans.iter().map(|text| text.content.as_ref()).collect()
}

/// The changes view drawn at W x H: its rows of text.
fn screen(view: &mut View, width: u16, height: u16) -> Vec<String> {
    let mut thread = ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();

    thread.draw(|file| view.draw(file, file.area())).unwrap();
    let bytes = thread.backend().buffer().clone();

    (0..height)
        .map(|y| (0..width).map(|x| bytes[(x, y)].symbol().to_string()).collect())
        .collect()
}

fn press(view: &mut View, character: KeyCode) -> Act {
    view.key(KeyEvent::new(character, KeyModifiers::NONE))
}

/// The screen row that holds S.
fn row_of(text: &[String], pat: &str) -> usize {
    text.iter()
        .position(|line| line.contains(pat))
        .unwrap_or_else(|| panic!("{pat}: {text:#?}"))
}

/// A view of a real repo D (its changes read now), the cursor's thread ID.
fn real_view(path: &Path, id: &str) -> View {
    let mut view = View::new(id, path);

    view.scm.shown = false;
    view.changes = load(path).unwrap();
    view.loaded = Some(Instant::now());
    view.stash_state.stash.clear();
    view.build(120);

    view
}

/// Select the rows whose text has PAT.
fn select(view: &mut View, pat: &str) {
    let rows: Vec<usize> = (0..view.layout.rows.len())
        .filter(|&i| text_of(view, i).contains(pat))
        .collect();

    assert!(!rows.is_empty(), "{pat}");

    for i in rows {
        view.select_row(i, true);
    }
}

/// The git step on its way ends (its toast note back).
fn wait_op(view: &mut View) -> String {
    for _ in 0..200 {
        if view.poll_op() {
            return view.take_note().unwrap_or_default();
        }

        std::thread::sleep(Duration::from_millis(20));
    }

    panic!("git step: 0 end");
}

/// A diff of N files, each with HUNKS hunks of 10 changed lines (Rust code).
fn big_diff(count: usize, hunks: usize) -> String {
    let mut text = String::new();

    for i in 0..count {
        let _ = write!(
            text,
            "diff --git a/src/m{i}.rs b/src/m{i}.rs\n--- a/src/m{i}.rs\n+++ b/src/m{i}.rs\n"
        );

        diff_hunks(hunks, &mut text, i);
    }

    text
}

fn diff_hunks(hunks: usize, text: &mut String, i: usize) {
    for hunk in 0..hunks {
        let at = hunk * 40 + 1;

        let _ = writeln!(text, "@@ -{at},8 +{at},10 @@ fn f{hunk}() {{");

        text.push_str(" let a = vec![1, 2, 3];\n");

        for key in 0..4 {
            let _ = writeln!(
                text,
                "-    let x{key}_{i} = compute(a.iter().map(|v| v * {key}).sum::<i32>(), \"old {hunk}\");"
            );
        }

        for key_value in 0..6 {
            let _ = writeln!(
                text,
                "+    let x{key_value}_{i} = compute(a.iter().map(|v| v * {key_value} + 1).sum::<i32>(), \"new {hunk}\");"
            );
        }

        text.push_str(" // done\n let b = 2;\n");
    }
}

/// A view on DIFF for comments thread ID (its comments file gone first).
fn thread_view(id: &str) -> View {
    let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir(id));
    let mut value = view(parse(DIFF));

    value.id = id.into();
    value.comment_state.comments.clear();
    value.comment_state.comments_base.clear();
    value.build(100);

    value
}

fn spot11() -> Vec<Spot> {
    vec![Spot {
        path: "src/a.rs".into(),
        line: 11,
        lines: String::new(),
    }]
}

/// The view reads the comments file again (an agent wrote it).
fn resync(view: &mut View) {
    view.comment_state.comments_at = None;
    view.sync_comments();
    view.rebuild_here();
}

#[path = "changes_comments_tests.rs"]
mod comments;

#[path = "changes_parse_tests.rs"]
mod parse;

#[path = "changes_render_tests.rs"]
mod render;

#[path = "changes_navigation_tests.rs"]
mod navigation;

#[path = "changes_git_tests.rs"]
mod git;

#[path = "changes_comment_threads_tests.rs"]
mod comment_threads;

#[path = "changes_performance_tests.rs"]
mod performance;

#[path = "changes_comment_storage_tests.rs"]
mod comment_storage;

const DIFF: &str = "diff --git a/src/a.rs b/src/a.rs
index 1..2 100644
--- a/src/a.rs
+++ b/src/a.rs
@@ -10,4 +10,5 @@ fn run() {
 let a = 1;
-let b = 2;
+let b = 3;
+let c = 4;
 let d = 5;
\\ No newline at end of file
diff --git a/tests/a_test.rs b/tests/a_test.rs
new file mode 100644
--- /dev/null
+++ b/tests/a_test.rs
@@ -0,0 +1,2 @@
+#[test]
+fn t() {}
diff --git a/old.rs b/new.rs
similarity index 100%
rename from old.rs
rename to new.rs
diff --git a/img.png b/img.png
Binary files a/img.png and b/img.png differ
";
