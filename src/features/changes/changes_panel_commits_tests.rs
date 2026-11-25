use super::*;

#[test]
fn the_commits_section_lists_unpushed_then_pushed_space_picks_brackets_jump() {
    use crate::features::changes::commits::fixture::*;
    let width = repo("panel");

    sh(&width, "echo draft > x.txt");
    let mut value = commit_view(&width, "commits-panel-1");

    let all = draw(&mut value, 140, 40).join("\n");

    assert!(
        all.contains("▾ COMMITS  4") && all.contains("▾ UNPUSHED") && all.contains("▾ PUSHED"),
        "{all}"
    );

    let text = draw(&mut value, 140, 40);

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
    on_commit(&mut value, "c4 subject");
    press(&mut value, KeyCode::Char(' '));

    assert_eq!(value.scm.commit_picks.len(), 1);
    assert!(
        value.hints().contains("ctrl+.: uncommit"),
        "the cursor on c3: commit keys"
    );

    on_commit(&mut value, "c2 subject");

    assert!(matches!(press(&mut value, KeyCode::Char(' ')), Act::Toast(thread) if thread.contains("pushed")));
    assert_eq!(value.scm.commit_picks.len(), 1);

    // [ ]: section labels; esc: 0 picks
    press(&mut value, KeyCode::Char('['));

    assert_eq!(value.scm.rows[value.scm.at], PRow::Section(Sec::Commits, 4));

    press(&mut value, KeyCode::Char('['));

    assert_eq!(value.scm.rows[value.scm.at], PRow::Section(Sec::Stash, 0));

    press(&mut value, KeyCode::Char(']'));

    assert_eq!(value.scm.rows[value.scm.at], PRow::Section(Sec::Commits, 4));
    assert!(value.takes_esc());

    press(&mut value, KeyCode::Esc);

    assert!(value.scm.commit_picks.is_empty());

    crate::domain::changes::commits::fixture::drop(&width);
}

#[test]
fn enter_on_a_commit_shows_its_files_read_only_esc_brings_the_changes_back() {
    use crate::features::changes::commits::fixture::*;
    let width = repo("peek");

    sh(&width, "echo draft > x.txt");
    let mut value = commit_view(&width, "commits-peek-1");

    on_commit(&mut value, "c3 subject");

    press(&mut value, KeyCode::Enter);

    if let Some(path) = value.peek.as_mut() {
        wait_read(path);
    }

    let text = draw(&mut value, 140, 40);
    let all = text.join("\n");
    let review: String = text
        .iter()
        .map(|line| line.chars().skip(36).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(!value.scm.focus, "the keys to the review");
    assert!(
        review.contains("◆ ") && review.contains("c3 subject") && review.contains("read only"),
        "{all}"
    );
    assert!(
        review.contains("c3.txt") && !review.contains("x.txt"),
        "the commit's files only: {all}"
    );
    assert!(value.hints().contains("read only"));

    // c, v, s, space: refused; 0 comments / marks
    for character in ['c', 'v', 's', ' ', 'a'] {
        assert!(
            matches!(press(&mut value, KeyCode::Char(character)), Act::Toast(thread) if thread.contains("read only")),
            "{character}"
        );
    }

    assert!(
        value
            .peek
            .as_ref()
            .is_some_and(|path_value| path_value.comment_state.comments.is_empty()
                && path_value.marks.is_empty()
                && path_value.modal_state.modal.is_none())
    );

    // j k move in the commit's diff; esc: the changes back
    press(&mut value, KeyCode::Char('j'));

    assert!(value.takes_esc());

    press(&mut value, KeyCode::Esc);

    assert!(value.peek.is_none());

    let all = draw(&mut value, 140, 40).join("\n");

    assert!(all.contains("x.txt") && !all.contains("read only"), "{all}");

    // → on a commit: shown, the keys to the review; ← back to the panel
    on_commit(&mut value, "c2 subject");
    value.scm.focus = true;
    press(&mut value, KeyCode::Right);

    assert!(value.peek.is_some() && !value.scm.focus);

    press(&mut value, KeyCode::Left);

    assert!(value.peek.is_some() && value.scm.focus);
    assert_eq!(value.peeked(), Some(value.changes.commits.list[2].oid.as_str()));

    crate::domain::changes::commits::fixture::drop(&width);
}

#[test]
fn the_shown_commit_lists_its_files_under_it_enter_opens_one_hidden_again_when_unshown() {
    let (width, mut value, panel, text_value, row) = prepare_commit_files();

    assert!(
        text_value[row(&text_value, "b.txt")].contains(" d") && text_value[row(&text_value, "a.txt")].contains(" A "),
        "dir + letter: {text_value:#?}"
    );

    // j onto b.txt (the 2nd file), enter: the review on its box, the keys there
    press(&mut value, KeyCode::Char('j'));
    press(&mut value, KeyCode::Char('j'));

    assert!(value.hints().contains("the file in the review"));

    press(&mut value, KeyCode::Enter);

    assert!(!value.scm.focus);

    let path_2 = value.peek.as_ref().unwrap();

    assert_eq!(path_2.changes.files[path_2.layout.rows[path_2.at].file].path, "d/b.txt");

    // ← on a file: its commit; → on c3: c5's files hide, the cursor stays on c3
    value.scm.focus = true;
    press(&mut value, KeyCode::Left);

    assert!(
        matches!(value.scm.rows[value.scm.at], PRow::Commit(i) if value.changes.commits.list[i].subject == "c5 subject")
    );

    on_commit(&mut value, "c3 subject");
    press(&mut value, KeyCode::Right);
    value.scm.focus = true;

    assert!(
        matches!(value.scm.rows[value.scm.at], PRow::Commit(i) if value.changes.commits.list[i].subject == "c3 subject"),
        "{:?}",
        value.scm.rows
    );

    if let Some(path_3) = value.peek.as_mut() {
        wait_read(path_3);
    }

    let text_3 = panel(&mut value);

    assert!(
        !text_3.iter().any(|line_3| line_3.contains("a.txt"))
            && row(&text_3, "c3 subject") + 1 == row(&text_3, "c3.txt"),
        "{text_3:#?}"
    );

    // ← on the shown commit: the changes back, its files hide
    press(&mut value, KeyCode::Left);
    let text_4 = panel(&mut value);

    assert!(
        value.peek.is_none() && !text_4.iter().any(|line_4| line_4.contains("c3.txt")),
        "{text_4:#?}"
    );

    // a click on a commit's file: the review on it, the keys stay on the panel
    on_commit(&mut value, "c5 subject");
    press(&mut value, KeyCode::Right);
    value.scm.focus = true;

    if let Some(path_4) = value.peek.as_mut() {
        wait_read(path_4);
    }

    let text_5 = panel(&mut value);
    let y = row(&text_5, "b.txt") as u16;

    value.mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 10,
        row: y,
        modifiers: KeyModifiers::NONE,
    });
    let path_5 = value.peek.as_ref().unwrap();

    assert!(value.scm.focus && path_5.changes.files[path_5.layout.rows[path_5.at].file].path == "d/b.txt");

    crate::domain::changes::commits::fixture::drop(&width);
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
    let width = repo("cfiles");

    sh(
        &width,
        "echo a > a.txt && mkdir d && echo b > d/b.txt && git add -A && git commit -qm 'c5 subject'",
    );
    let mut value = commit_view(&width, "commits-files-1");

    let panel = |view: &mut View| {
        draw(view, 140, 40)
            .iter()
            .map(|line| line.chars().take(36).collect::<String>())
            .collect::<Vec<_>>()
    };

    let text = panel(&mut value);

    assert!(
        !text.iter().any(|line_value| line_value.contains("a.txt")),
        "0 commit shown: 0 files: {text:#?}"
    );

    // → on c5: shown, its files under it (before c4), the keys to the review; ← back
    on_commit(&mut value, "c5 subject");
    press(&mut value, KeyCode::Right);

    if let Some(path_value) = value.peek.as_mut() {
        wait_read(path_value);
    }

    let text_value = panel(&mut value);

    let row = |text_2: &[String], pat: &str| {
        text_2
            .iter()
            .position(|line_2| line_2.contains(pat))
            .unwrap_or_else(|| panic!("{pat}: {text_2:#?}"))
    };

    assert!(!value.scm.focus);

    press(&mut value, KeyCode::Left);

    assert!(value.scm.focus);
    assert!(
        row(&text_value, "c5 subject") < row(&text_value, "a.txt")
            && row(&text_value, "a.txt") < row(&text_value, "b.txt")
            && row(&text_value, "b.txt") < row(&text_value, "c4 subject"),
        "{text_value:#?}"
    );

    (width, value, panel, text_value, row)
}

#[test]
fn uncommit_asks_then_uncommits_the_picked_commit_its_message_goes_to_the_box() {
    use crate::features::changes::commits::fixture::*;
    let width = repo("uncommit");
    let mut value = commit_view(&width, "commits-uncommit-1");

    on_commit(&mut value, "c4 subject");
    let before = oid(&width, "HEAD");

    value.ctx_cmd("uncommit");
    let all = draw(&mut value, 140, 40).join("\n");

    assert!(
        all.contains("uncommit 1 commit? files stay") && all.contains("c4 subject"),
        "{all}"
    );
    assert!(value.operation_state.op.is_none(), "a question first");

    // n: 0 uncommit; u again, y: it runs
    press(&mut value, KeyCode::Char('n'));

    assert!(value.modal_state.modal.is_none() && value.operation_state.op.is_none());
    assert_eq!(oid(&width, "HEAD"), before);

    value.ctx_cmd("uncommit");
    press(&mut value, KeyCode::Char('y'));
    let note = wait_op(&mut value);

    assert!(note.contains("uncommitted 1 commit"), "{note}");
    assert_eq!(oid(&width, "HEAD"), oid(&width, &format!("{before}^")));
    assert!(value.scm.msg.text().contains("c4 subject") && value.scm.msg.text().contains("c4 body"));

    // a pushed commit: 0 question
    wait_read(&mut value);
    on_commit(&mut value, "c2 subject");

    assert!(matches!(value.ctx_cmd("uncommit"), Act::Toast(thread) if thread.contains("pushed")));
    assert!(value.modal_state.modal.is_none());

    crate::domain::changes::commits::fixture::drop(&width);
}
