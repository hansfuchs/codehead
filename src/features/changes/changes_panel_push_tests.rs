use super::*;

#[test]
fn the_ship_menu_pushes_up_to_the_picked_commit_after_a_question() {
    use crate::features::changes::commits::fixture::*;
    let dir = repo("shipto");
    let mut view = commit_view(&dir, "commits-shipto-1");
    let c3 = oid(&dir, "HEAD^");

    // 0 picks: the ship section as before
    assert!(!draw_ctx(&mut view, 140, 40).join("\n").contains("push up to"));

    on_commit(&mut view, "c3 subject");
    view.scm.focus = true;
    press(&mut view, KeyCode::Char(' '));
    let all = draw_ctx(&mut view, 140, 40).join("\n");

    assert!(all.contains("push up to") && all.contains("1 commit"), "{all}");

    // the row (under ship, push): the question lists the commits, 0 push yet
    let label = view
        .ship_rows()
        .iter()
        .find(|step| step.label.contains("push up to"))
        .map(|step| step.label.clone())
        .unwrap();

    ship(&mut view, &label);
    let all = draw(&mut view, 140, 40).join("\n");

    assert!(
        all.contains("push 1 commit to origin/feat?")
            && all.contains("1 newer stay local")
            && all.contains("c3 subject"),
        "{all}"
    );
    assert!(view.operation_state.op.is_none());

    press(&mut view, KeyCode::Char('y'));
    let note = wait_op(&mut view);

    assert!(note.contains("pushed up to"), "{note}");
    assert_eq!(oid(&dir, "origin/feat"), c3);

    crate::domain::changes::commits::fixture::drop(&dir);
}

#[test]
fn p_pushes_every_unpushed_commit_after_a_question() {
    use crate::features::changes::commits::fixture::*;
    let dir = repo("pushall");
    let mut view = commit_view(&dir, "commits-pushall-1");
    let head = oid(&dir, "HEAD");

    // the UNPUSHED row: a push button
    let all = draw(&mut view, 140, 40).join("\n");

    assert!(all.contains("↑ push"), "{all}");

    view.scm.focus = true;
    press(&mut view, KeyCode::Char('P'));
    let all = draw(&mut view, 140, 40).join("\n");

    assert!(
        all.contains("push 2 commits to origin/feat?") && all.contains("c3 subject") && all.contains("c4 subject"),
        "{all}"
    );
    assert!(view.operation_state.op.is_none());

    press(&mut view, KeyCode::Char('y'));
    let note = wait_op(&mut view);

    assert!(note.contains("pushed up to"), "{note}");
    assert_eq!(oid(&dir, "origin/feat"), head);

    // 0 unpushed: 0 question
    wait_read(&mut view);

    assert!(matches!(press(&mut view, KeyCode::Char('P')), Act::Toast(toast) if toast.contains("0 unpushed")));
    assert!(view.modal_state.modal.is_none());

    crate::domain::changes::commits::fixture::drop(&dir);
}

#[test]
fn a_running_push_moves_its_commits_to_pushing_with_a_wave_0_keys_there() {
    use crate::features::changes::commits::fixture::*;
    let dir = repo("pushing");
    let mut view = commit_view(&dir, "commits-pushing-1");
    let c3 = oid(&dir, "HEAD^");

    draw(&mut view, 140, 40);

    // the push up to c3 runs: c3 in PUSHING above UNPUSHED, c4 stays; the wave at the edge
    view.pushing = vec![c3.clone()];
    view.pushing_at = Some(std::time::Instant::now());
    let rows = draw(&mut view, 140, 40);

    let row = |pat: &str| {
        rows.iter()
            .position(|line| line.contains(pat))
            .unwrap_or_else(|| panic!("{pat}: {}", rows.join("\n")))
    };

    assert!(
        row("PUSHING") < row("c3 subject")
            && row("c3 subject") < row("▾ UNPUSHED")
            && row("▾ UNPUSHED") < row("c4 subject"),
        "{}",
        rows.join("\n")
    );

    let px = usize::from(view.scm.rect.right());
    let head: String = rows[row("PUSHING")].chars().take(px).collect();

    assert!(
        crate::ui::thread_card::WAVE
            .iter()
            .any(|frame| head.trim_end().ends_with(frame)),
        "{head}"
    );
    assert!(view.pushing());

    // j over the PUSHING rows: the cursor lands on UNPUSHED; a click on them: 0 change
    view.scm.focus = true;
    let all = view.scm.rows.iter().position(|row| *row == PRow::All).unwrap();

    view.scm.at = all;
    press(&mut view, KeyCode::Char('j'));

    assert_eq!(view.scm.rows[view.scm.at], PRow::Push(false, 1));

    press(&mut view, KeyCode::Char('k'));

    assert_eq!(view.scm.at, all);

    // the UNPUSHED group's items: c4 only
    assert_eq!(view.group_items(&PRow::Push(false, 1)).2.len(), 1);

    // the real push: c3 out of PUSHING into PUSHED
    view.pushing.clear();

    assert!(matches!(view.start_op(Op::PushTo(c3.clone())), Act::Toast(_)));
    assert_eq!(view.pushing, vec![c3.clone()]);

    wait_op(&mut view);

    assert!(view.pushing.is_empty() && view.pushing_at.is_none());
    assert!(view.changes.commits.find(&c3).is_some_and(|commit| commit.pushed));
    assert!(!draw(&mut view, 140, 40).join("\n").contains("PUSHING"));

    crate::domain::changes::commits::fixture::drop(&dir);
}

#[test]
fn an_agents_push_shows_as_pushing_with_its_name_and_blocks_the_panels_pushes() {
    use crate::features::changes::commits::fixture::*;
    let dir = repo("agentpush");
    let mut view = commit_view(&dir, "commits-agentpush-1");

    wait_read(&mut view);
    let gd = view.changes.git_dir.clone().expect("git dir");

    // GIGAS's push runs (a live process holds the lock)
    let mut holder = std::process::Command::new("sleep").arg("30").spawn().unwrap();

    std::mem::forget(crate::infra::pushlock::acquire_in(&gd, "GIGAS", holder.id()).unwrap());
    let all = draw(&mut view, 140, 40).join("\n");

    assert!(
        all.contains("PUSHING  2 · GIGAS") && !all.contains("▾ UNPUSHED"),
        "{all}"
    );

    view.scm.focus = true;

    assert!(matches!(press(&mut view, KeyCode::Char('P')), Act::Toast(toast) if toast.contains("push")));
    assert!(view.modal_state.modal.is_none() && view.operation_state.op.is_none());

    // the ship section: push grayed, running it toasts
    assert!(
        view.ship_rows()
            .iter()
            .find(|step| step.label == "push")
            .is_some_and(|step| step.off && step.state == "a push runs")
    );
    assert!(matches!(ship(&mut view, "push"), Act::Toast(_)));

    // the panel's own git step: the lock says no
    assert!(
        crate::features::changes::run_op(&dir, &Op::Push)
            .unwrap_err()
            .contains("GIGAS")
    );

    // the push ends: a read, then the commits leave PUSHING
    holder.kill().unwrap();
    holder.wait().unwrap();
    draw(&mut view, 140, 40);

    assert!(view.push_by.is_none() && view.push_hold);

    wait_read(&mut view);
    let all = draw(&mut view, 140, 40).join("\n");

    assert!(!all.contains("PUSHING") && all.contains("▾ UNPUSHED"), "{all}");

    crate::domain::changes::commits::fixture::drop(&dir);
}
