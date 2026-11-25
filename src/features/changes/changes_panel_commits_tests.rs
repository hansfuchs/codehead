use super::*;

#[test]
fn the_commits_section_lists_unpushed_then_pushed_space_picks_brackets_jump() {
    use crate::features::changes::commits::fixture::*;
    let dir = repo("panel");

    sh(&dir, "echo draft > x.txt");
    let mut view = commit_view(&dir, "commits-panel-1");

    let all = draw(&mut view, 140, 40).join("\n");

    assert!(
        all.contains("▾ COMMITS  4") && all.contains("▾ UNPUSHED") && all.contains("▾ PUSHED"),
        "{all}"
    );

    let text = draw(&mut view, 140, 40);

    let row = |pat: &str| {
        text.iter()
            .position(|line| line.contains(pat))
            .unwrap_or_else(|| panic!("{pat}"))
    };

    assert!(
        row("UNPUSHED") < row("c4 subject")
            && row("c4 subject") < row("c3 subject")
            && row("c3 subject") < row("▾ PUSHED")
            && row("▾ PUSHED") < row("c2 subject")
    );
    assert!(
        text[row("c4 subject")].contains("● ") && text[row("c2 subject")].contains("○ "),
        "{all}"
    );

    // space: an unpushed commit picked, a pushed one refused
    on_commit(&mut view, "c4 subject");
    press(&mut view, KeyCode::Char(' '));

    assert_eq!(view.scm.commit_picks.len(), 1);
    assert!(
        view.hints().contains("ctrl+.: uncommit"),
        "the cursor on c3: commit keys"
    );

    on_commit(&mut view, "c2 subject");

    assert!(matches!(press(&mut view, KeyCode::Char(' ')), Act::Toast(toast) if toast.contains("pushed")));
    assert_eq!(view.scm.commit_picks.len(), 1);

    // [ ]: section labels; esc: 0 picks
    press(&mut view, KeyCode::Char('['));

    assert_eq!(view.scm.rows[view.scm.at], PRow::Section(Sec::Commits, 4));

    press(&mut view, KeyCode::Char('['));

    assert_eq!(view.scm.rows[view.scm.at], PRow::Section(Sec::Stash, 0));

    press(&mut view, KeyCode::Char(']'));

    assert_eq!(view.scm.rows[view.scm.at], PRow::Section(Sec::Commits, 4));
    assert!(view.takes_esc());

    press(&mut view, KeyCode::Esc);

    assert!(view.scm.commit_picks.is_empty());

    crate::domain::changes::commits::fixture::drop(&dir);
}

#[test]
fn enter_on_a_commit_shows_its_files_read_only_esc_brings_the_changes_back() {
    use crate::features::changes::commits::fixture::*;
    let dir = repo("peek");

    sh(&dir, "echo draft > x.txt");
    let mut view = commit_view(&dir, "commits-peek-1");

    on_commit(&mut view, "c3 subject");

    press(&mut view, KeyCode::Enter);

    if let Some(peek) = view.peek.as_mut() {
        wait_read(peek);
    }

    let text = draw(&mut view, 140, 40);
    let all = text.join("\n");
    let review: String = text
        .iter()
        .map(|line| line.chars().skip(36).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(!view.scm.focus, "the keys to the review");
    assert!(
        review.contains("◆ ") && review.contains("c3 subject") && review.contains("read only"),
        "{all}"
    );
    assert!(
        review.contains("c3.txt") && !review.contains("x.txt"),
        "the commit's files only: {all}"
    );
    assert!(view.hints().contains("read only"));

    // c, v, s, space: refused; 0 comments / marks
    for key in ['c', 'v', 's', ' ', 'a'] {
        assert!(
            matches!(press(&mut view, KeyCode::Char(key)), Act::Toast(toast) if toast.contains("read only")),
            "{key}"
        );
    }

    assert!(
        view.peek
            .as_ref()
            .is_some_and(|peek| peek.comment_state.comments.is_empty()
                && peek.marks.is_empty()
                && peek.modal_state.modal.is_none())
    );

    // j k move in the commit's diff; esc: the changes back
    press(&mut view, KeyCode::Char('j'));

    assert!(view.takes_esc());

    press(&mut view, KeyCode::Esc);

    assert!(view.peek.is_none());

    let all = draw(&mut view, 140, 40).join("\n");

    assert!(all.contains("x.txt") && !all.contains("read only"), "{all}");

    // → on a commit: shown, the keys to the review; ← back to the panel
    on_commit(&mut view, "c2 subject");
    view.scm.focus = true;
    press(&mut view, KeyCode::Right);

    assert!(view.peek.is_some() && !view.scm.focus);

    press(&mut view, KeyCode::Left);

    assert!(view.peek.is_some() && view.scm.focus);
    assert_eq!(view.peeked(), Some(view.changes.commits.list[2].oid.as_str()));

    crate::domain::changes::commits::fixture::drop(&dir);
}

#[test]
fn the_shown_commit_lists_its_files_under_it_enter_opens_one_hidden_again_when_unshown() {
    let (dir, mut view, panel, c5_shown, row) = prepare_commit_files();

    assert!(
        c5_shown[row(&c5_shown, "b.txt")].contains(" d") && c5_shown[row(&c5_shown, "a.txt")].contains(" A "),
        "dir + letter: {c5_shown:#?}"
    );

    // j onto b.txt (the 2nd file), enter: the review on its box, the keys there
    press(&mut view, KeyCode::Char('j'));
    press(&mut view, KeyCode::Char('j'));

    assert!(view.hints().contains("the file in the review"));

    press(&mut view, KeyCode::Enter);

    assert!(!view.scm.focus);

    let peek = view.peek.as_ref().unwrap();

    assert_eq!(peek.changes.files[peek.layout.rows[peek.at].file].path, "d/b.txt");

    // ← on a file: its commit; → on c3: c5's files hide, the cursor stays on c3
    view.scm.focus = true;
    press(&mut view, KeyCode::Left);

    assert!(
        matches!(view.scm.rows[view.scm.at], PRow::Commit(i) if view.changes.commits.list[i].subject == "c5 subject")
    );

    on_commit(&mut view, "c3 subject");
    press(&mut view, KeyCode::Right);
    view.scm.focus = true;

    assert!(
        matches!(view.scm.rows[view.scm.at], PRow::Commit(i) if view.changes.commits.list[i].subject == "c3 subject"),
        "{:?}",
        view.scm.rows
    );

    if let Some(peek) = view.peek.as_mut() {
        wait_read(peek);
    }

    let c3_shown = panel(&mut view);

    assert!(
        !c3_shown.iter().any(|line| line.contains("a.txt"))
            && row(&c3_shown, "c3 subject") + 1 == row(&c3_shown, "c3.txt"),
        "{c3_shown:#?}"
    );

    // ← on the shown commit: the changes back, its files hide
    press(&mut view, KeyCode::Left);
    let unshown = panel(&mut view);

    assert!(
        view.peek.is_none() && !unshown.iter().any(|line| line.contains("c3.txt")),
        "{unshown:#?}"
    );

    // a click on a commit's file: the review on it, the keys stay on the panel
    on_commit(&mut view, "c5 subject");
    press(&mut view, KeyCode::Right);
    view.scm.focus = true;

    if let Some(peek) = view.peek.as_mut() {
        wait_read(peek);
    }

    let c5_shown_again = panel(&mut view);
    let y = row(&c5_shown_again, "b.txt") as u16;

    view.mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 10,
        row: y,
        modifiers: KeyModifiers::NONE,
    });
    let peek = view.peek.as_ref().unwrap();

    assert!(view.scm.focus && peek.changes.files[peek.layout.rows[peek.at].file].path == "d/b.txt");

    crate::domain::changes::commits::fixture::drop(&dir);
}

type CommitFilesFixture = (
    PathBuf,
    View,
    fn(&mut View) -> Vec<String>,
    Vec<String>,
    fn(&[String], &str) -> usize,
);

fn prepare_commit_files() -> CommitFilesFixture {
    use crate::features::changes::commits::fixture::*;
    let dir = repo("cfiles");

    sh(
        &dir,
        "echo a > a.txt && mkdir d && echo b > d/b.txt && git add -A && git commit -qm 'c5 subject'",
    );
    let mut view = commit_view(&dir, "commits-files-1");

    let panel = |view: &mut View| {
        draw(view, 140, 40)
            .iter()
            .map(|line| line.chars().take(36).collect::<String>())
            .collect::<Vec<_>>()
    };

    let text = panel(&mut view);

    assert!(
        !text.iter().any(|line| line.contains("a.txt")),
        "0 commit shown: 0 files: {text:#?}"
    );

    // → on c5: shown, its files under it (before c4), the keys to the review; ← back
    on_commit(&mut view, "c5 subject");
    press(&mut view, KeyCode::Right);

    if let Some(peek) = view.peek.as_mut() {
        wait_read(peek);
    }

    let c5_shown = panel(&mut view);

    let row = |lines: &[String], pat: &str| {
        lines
            .iter()
            .position(|line| line.contains(pat))
            .unwrap_or_else(|| panic!("{pat}: {lines:#?}"))
    };

    assert!(!view.scm.focus);

    press(&mut view, KeyCode::Left);

    assert!(view.scm.focus);
    assert!(
        row(&c5_shown, "c5 subject") < row(&c5_shown, "a.txt")
            && row(&c5_shown, "a.txt") < row(&c5_shown, "b.txt")
            && row(&c5_shown, "b.txt") < row(&c5_shown, "c4 subject"),
        "{c5_shown:#?}"
    );

    (dir, view, panel, c5_shown, row)
}

#[test]
fn uncommit_asks_then_uncommits_the_picked_commit_its_message_goes_to_the_box() {
    use crate::features::changes::commits::fixture::*;
    let dir = repo("uncommit");
    let mut view = commit_view(&dir, "commits-uncommit-1");

    on_commit(&mut view, "c4 subject");
    let before = oid(&dir, "HEAD");

    view.ctx_cmd("uncommit");
    let all = draw(&mut view, 140, 40).join("\n");

    assert!(
        all.contains("uncommit 1 commit? files stay") && all.contains("c4 subject"),
        "{all}"
    );
    assert!(view.operation_state.op.is_none(), "a question first");

    // n: 0 uncommit; u again, y: it runs
    press(&mut view, KeyCode::Char('n'));

    assert!(view.modal_state.modal.is_none() && view.operation_state.op.is_none());
    assert_eq!(oid(&dir, "HEAD"), before);

    view.ctx_cmd("uncommit");
    press(&mut view, KeyCode::Char('y'));
    let note = wait_op(&mut view);

    assert!(note.contains("uncommitted 1 commit"), "{note}");
    assert_eq!(oid(&dir, "HEAD"), oid(&dir, &format!("{before}^")));
    assert!(view.scm.msg.text().contains("c4 subject") && view.scm.msg.text().contains("c4 body"));

    // a pushed commit: 0 question
    wait_read(&mut view);
    on_commit(&mut view, "c2 subject");

    assert!(matches!(view.ctx_cmd("uncommit"), Act::Toast(toast) if toast.contains("pushed")));
    assert!(view.modal_state.modal.is_none());

    crate::domain::changes::commits::fixture::drop(&dir);
}
