use super::*;

#[test]
fn the_ship_menu_pushes_up_to_the_picked_commit_after_a_question() {
    use crate::features::changes::commits::fixture::*;
    let width = repo("shipto");
    let mut value = commit_view(&width, "commits-shipto-1");
    let c3 = oid(&width, "HEAD^");

    // 0 picks: the ship section as before
    assert!(!draw_ctx(&mut value, 140, 40).join("\n").contains("push up to"));

    on_commit(&mut value, "c3 subject");
    value.scm.focus = true;
    press(&mut value, KeyCode::Char(' '));
    let all = draw_ctx(&mut value, 140, 40).join("\n");

    assert!(all.contains("push up to") && all.contains("1 commit"), "{all}");

    // the row (under ship, push): the question lists the commits, 0 push yet
    let label = value
        .ship_rows()
        .iter()
        .find(|result| result.label.contains("push up to"))
        .map(|result_value| result_value.label.clone())
        .unwrap();

    ship(&mut value, &label);
    let all = draw(&mut value, 140, 40).join("\n");

    assert!(
        all.contains("push 1 commit to origin/feat?")
            && all.contains("1 newer stay local")
            && all.contains("c3 subject"),
        "{all}"
    );
    assert!(value.operation_state.op.is_none());

    press(&mut value, KeyCode::Char('y'));
    let note = wait_op(&mut value);

    assert!(note.contains("pushed up to"), "{note}");
    assert_eq!(oid(&width, "origin/feat"), c3);

    crate::domain::changes::commits::fixture::drop(&width);
}

#[test]
fn p_pushes_every_unpushed_commit_after_a_question() {
    use crate::features::changes::commits::fixture::*;
    let width = repo("pushall");
    let mut value = commit_view(&width, "commits-pushall-1");
    let head = oid(&width, "HEAD");

    // the UNPUSHED row: a push button
    let all = draw(&mut value, 140, 40).join("\n");

    assert!(all.contains("↑ push"), "{all}");

    value.scm.focus = true;
    press(&mut value, KeyCode::Char('P'));
    let all = draw(&mut value, 140, 40).join("\n");

    assert!(
        all.contains("push 2 commits to origin/feat?") && all.contains("c3 subject") && all.contains("c4 subject"),
        "{all}"
    );
    assert!(value.operation_state.op.is_none());

    press(&mut value, KeyCode::Char('y'));
    let note = wait_op(&mut value);

    assert!(note.contains("pushed up to"), "{note}");
    assert_eq!(oid(&width, "origin/feat"), head);

    // 0 unpushed: 0 question
    wait_read(&mut value);

    assert!(matches!(press(&mut value, KeyCode::Char('P')), Act::Toast(thread) if thread.contains("0 unpushed")));
    assert!(value.modal_state.modal.is_none());

    crate::domain::changes::commits::fixture::drop(&width);
}

#[test]
fn a_running_push_moves_its_commits_to_pushing_with_a_wave_0_keys_there() {
    use crate::features::changes::commits::fixture::*;
    let width = repo("pushing");
    let mut value = commit_view(&width, "commits-pushing-1");
    let c3 = oid(&width, "HEAD^");

    draw(&mut value, 140, 40);

    // the push up to c3 runs: c3 in PUSHING above UNPUSHED, c4 stays; the wave at the edge
    value.pushing = vec![c3.clone()];
    value.pushing_at = Some(std::time::Instant::now());
    let rows = draw(&mut value, 140, 40);

    let row = |thread: &str| {
        rows.iter()
            .position(|line| line.contains(thread))
            .unwrap_or_else(|| panic!("{thread}: {}", rows.join("\n")))
    };

    assert!(
        row("PUSHING") < row("c3 subject")
            && row("c3 subject") < row("▾ UNPUSHED")
            && row("▾ UNPUSHED") < row("c4 subject"),
        "{}",
        rows.join("\n")
    );

    let px = usize::from(value.scm.rect.right());
    let head: String = rows[row("PUSHING")].chars().take(px).collect();

    assert!(
        crate::ui::thread_card::WAVE
            .iter()
            .any(|file| head.trim_end().ends_with(file)),
        "{head}"
    );
    assert!(value.pushing());

    // j over the PUSHING rows: the cursor lands on UNPUSHED; a click on them: 0 change
    value.scm.focus = true;
    let all = value.scm.rows.iter().position(|result| *result == PRow::All).unwrap();

    value.scm.at = all;
    press(&mut value, KeyCode::Char('j'));

    assert_eq!(value.scm.rows[value.scm.at], PRow::Push(false, 1));

    press(&mut value, KeyCode::Char('k'));

    assert_eq!(value.scm.at, all);

    // the UNPUSHED group's items: c4 only
    assert_eq!(value.group_items(&PRow::Push(false, 1)).2.len(), 1);

    // the real push: c3 out of PUSHING into PUSHED
    value.pushing.clear();

    assert!(matches!(value.start_op(Op::PushTo(c3.clone())), Act::Toast(_)));
    assert_eq!(value.pushing, vec![c3.clone()]);

    wait_op(&mut value);

    assert!(value.pushing.is_empty() && value.pushing_at.is_none());
    assert!(
        value
            .changes
            .commits
            .find(&c3)
            .is_some_and(|character| character.pushed)
    );
    assert!(!draw(&mut value, 140, 40).join("\n").contains("PUSHING"));

    crate::domain::changes::commits::fixture::drop(&width);
}

#[test]
fn an_agents_push_shows_as_pushing_with_its_name_and_blocks_the_panels_pushes() {
    use crate::features::changes::commits::fixture::*;
    let width = repo("agentpush");
    let mut value = commit_view(&width, "commits-agentpush-1");

    wait_read(&mut value);
    let gd = value.changes.git_dir.clone().expect("git dir");

    // GIGAS's push runs (a live process holds the lock)
    let mut holder = std::process::Command::new("sleep").arg("30").spawn().unwrap();

    std::mem::forget(crate::infra::pushlock::acquire_in(&gd, "GIGAS", holder.id()).unwrap());
    let all = draw(&mut value, 140, 40).join("\n");

    assert!(
        all.contains("PUSHING  2 · GIGAS") && !all.contains("▾ UNPUSHED"),
        "{all}"
    );

    value.scm.focus = true;

    assert!(matches!(press(&mut value, KeyCode::Char('P')), Act::Toast(thread) if thread.contains("push")));
    assert!(value.modal_state.modal.is_none() && value.operation_state.op.is_none());

    // the ship section: push grayed, running it toasts
    assert!(
        value
            .ship_rows()
            .iter()
            .find(|result| result.label == "push")
            .is_some_and(|result_value| result_value.off && result_value.state == "a push runs")
    );
    assert!(matches!(ship(&mut value, "push"), Act::Toast(_)));

    // the panel's own git step: the lock says no
    assert!(
        crate::features::changes::run_op(&width, &Op::Push)
            .unwrap_err()
            .contains("GIGAS")
    );

    // the push ends: a read, then the commits leave PUSHING
    holder.kill().unwrap();
    holder.wait().unwrap();
    draw(&mut value, 140, 40);

    assert!(value.push_by.is_none() && value.push_hold);

    wait_read(&mut value);
    let all = draw(&mut value, 140, 40).join("\n");

    assert!(!all.contains("PUSHING") && all.contains("▾ UNPUSHED"), "{all}");

    crate::domain::changes::commits::fixture::drop(&width);
}
