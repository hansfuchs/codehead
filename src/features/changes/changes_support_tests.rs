use super::*;

pub(super) const DIFF: &str = "diff --git a/src/a.rs b/src/a.rs
--- a/src/a.rs
+++ b/src/a.rs
@@ -10,4 +10,5 @@ fn run() {
 let a = 1;
-let b = 2;
+let b = 3;
+let c = 4;
 let d = 5;
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

/// A view of DIFF, 0 stored state.
pub(super) fn test_view() -> View {
    let mut files = parse(DIFF);

    order(&mut files);
    let id = format!("changes-scm-{}", std::process::id());
    let mut view = View::new(&id, Path::new("/tmp"));

    view.marks.clear();
    view.comment_state.comments.clear();
    view.stash_state.stash.clear();
    view.changes = Changes {
        base: "origin/main".into(),
        files,
        ..Default::default()
    };
    view.loaded = Some(Instant::now());

    view
}

/// The view drawn at WIDTH x HEIGHT: its rows of text.
pub(super) fn draw(view: &mut View, width: u16, height: u16) -> Vec<String> {
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();

    terminal.draw(|frame| view.draw(frame, frame.area())).unwrap();
    let buffer = terminal.backend().buffer().clone();

    (0..height)
        .map(|y| (0..width).map(|x| buffer[(x, y)].symbol().to_string()).collect())
        .collect()
}

/// The panel's context menu (ctrl+. in the thread view) over the view, where the thread view
/// puts it: the screen.
pub(super) fn draw_ctx(view: &mut View, width: u16, height: u16) -> Vec<String> {
    view.focus_panel(true);
    draw(view, width, height);
    let (title, items) = view.ctx_items();
    let mut menu = crate::ui::context_menu::Menu::new(title, items).expect("items");

    menu.head = view.ctx_head();
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();

    terminal
        .draw(|frame| {
            view.draw(frame, frame.area());
            let place = view.ctx_place();

            menu.render(frame, place, frame.area());
        })
        .unwrap();

    let buffer = terminal.backend().buffer().clone();

    (0..height)
        .map(|y| (0..width).map(|x| buffer[(x, y)].symbol().to_string()).collect())
        .collect()
}

/// Ship step LABEL of the panel's context menu runs (an outward one: the menu asked first).
pub(super) fn ship(view: &mut View, label: &str) -> Act {
    let i = view
        .ship_rows()
        .iter()
        .position(|row| row.label == label)
        .unwrap_or_else(|| panic!("{label}"));

    view.ctx_step(i)
}
