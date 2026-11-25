use super::*;

#[test]
fn the_panels_menu_has_the_ship_steps_a_stopped_rebase_only_its_own() {
    let mut view = test_view();

    // S: 0 menu of its own any more
    view.key(KeyEvent::new(KeyCode::Char('S'), KeyModifiers::SHIFT));

    assert!(view.modal_state.modal.is_none());

    let all = draw_ctx(&mut view, 120, 30).join("\n");

    assert!(
        all.contains(" ship ") && all.contains("push") && all.contains("open the pull request"),
        "{all}"
    );

    let (_, items) = view.ctx_items();

    assert!(
        items
            .iter()
            .any(|item| item.label == "push" && item.asks && item.section == "ship/git"),
        "push: outward, asks first"
    );

    // a stopped rebase: its steps only; the agents resolve at once, abort asks
    view.changes.stopped = Some(vec!["src/a.rs".into()]);
    let all = draw_ctx(&mut view, 120, 30).join("\n");

    assert!(
        all.contains("resolve the conflicts") && !all.contains("open the pull request"),
        "{all}"
    );
    assert!(matches!(ship(&mut view, "resolve the conflicts"), Act::Send(prompt) if prompt.contains("src/a.rs")));

    let (_, items) = view.ctx_items();

    assert!(
        items.iter().any(|item| item.label.contains("abort") && item.asks),
        "abort: asks first"
    );
}

#[test]
fn the_ship_section_hands_pr_steps_to_a_sub_thread_and_shows_the_open_pr() {
    let mut view = prepare_ship_steps();

    match ship(&mut view, "create PR draft") {
        Act::Sub(task) => assert!(
            task.starts_with("Open a draft pull request (`--draft`) for the branch `task/x`")
                && task.contains("git push -u origin task/x")
                && task.contains("--head task/x --base main --draft")
                && !task.contains("HEAD`,")
                && task.contains("uncommitted changes stay"),
            "{task}"
        ),
        other => panic!("{other:?}"),
    }

    // a PR open: its label on top, update description, 0 create rows
    view.pull_request.pr = Some(crate::features::changes::Pr {
        number: 1234,
        url: "https://github.com/o/codehead/pull/1234".into(),
        ..Default::default()
    });
    let all = draw_ctx(&mut view, 140, 40).join("\n");

    assert!(
        all.contains("codehead#1234") && all.contains("update PR description") && !all.contains("create PR"),
        "{all}"
    );

    // update PR on the newest main: uncommitted files → 0 sub-thread; a clean tree → the task
    let up = view
        .ship_rows()
        .iter()
        .position(|step| step.label == "update PR on the newest main")
        .unwrap();

    assert!(view.ship_rows()[up].agent && view.ship_rows()[up].group == crate::features::changes::ShipGroup::Pr);
    assert!(
        matches!(ship(&mut view, "update PR on the newest main"), Act::Toast(toast) if toast.contains("uncommitted"))
    );

    let files = std::mem::take(&mut view.changes.files);

    match ship(&mut view, "update PR on the newest main") {
        Act::Sub(task) => assert!(
            task.contains("git rebase origin/main")
                && task.contains("--force-with-lease origin task/x")
                && task.contains("`task/x`"),
            "{task}"
        ),
        other => panic!("{other:?}"),
    }

    view.changes.files = files;

    assert!(
        matches!(ship(&mut view, "update PR description"), Act::Sub(task) if task.contains("gh pr edit 1234") && task.contains("`headRefName` is not `task/x`"))
    );

    // ship: row 0, the state along, never merge
    let first = view.ship_rows()[0].label.clone();

    assert!(
        matches!(ship(&mut view, &first), Act::Sub(task) if task.contains("codehead#1234") && task.contains("Never merge") && task.contains("Never push to any branch but `task/x`") && !task.contains("origin HEAD"))
    );
}

fn prepare_ship_steps() -> View {
    let mut view = test_view();

    view.focus_panel(true);

    assert!(view.scm.focus);
    assert_eq!(view.hints(), "tab: next · ctrl+.: actions, ship ");

    // 0 PR: ship, create PR, create PR draft; right of the panel, under a "ship" subheading
    let rows = draw_ctx(&mut view, 140, 40);
    let all = rows.join("\n");

    assert!(
        all.contains("ship: commit, push, PR, review, CI green")
            && all.contains("create PR draft")
            && !all.contains("update PR description"),
        "{all}"
    );

    // ✦ on the agent steps only; 1 subheading, a rule between ship / git / PR
    assert!(
        all.contains("✦ ship: commit")
            && all.contains("✦ create PR draft")
            && all.contains("   push ")
            && !all.contains("✦ push"),
        "{all}"
    );
    assert_eq!(
        rows.iter().filter(|line| line.contains("├─ ship ─")).count(),
        1,
        "{all}"
    );
    assert_eq!(
        rows.iter()
            .filter(|line| line.contains('├') && line.contains('┤'))
            .count(),
        3,
        "{all}"
    );

    let draft_row = rows.iter().find(|line| line.contains("create PR draft")).unwrap();
    let px = usize::from(view.scm.rect.right());

    assert!(
        draft_row.chars().take(px).all(|symbol| symbol != '›')
            && draft_row.chars().nth(px).is_some_and(|symbol| symbol == '│'),
        "{draft_row}"
    );

    // create PR draft: 0 commits ahead → 0 sub-thread
    assert!(matches!(ship(&mut view, "create PR draft"), Act::Toast(toast) if toast.contains("commit first")));

    // 2 commits ahead, 0 branch (detached / 0 git): 0 sub-thread
    view.changes.ahead = 2;

    assert!(matches!(ship(&mut view, "create PR draft"), Act::Toast(_)));

    // on a branch: the sub-thread's task names it, a draft, the commits only
    view.branch_override = Some("task/x".into());

    view
}
