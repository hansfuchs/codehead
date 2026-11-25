use super::*;

#[test]
fn the_panels_menu_has_the_ship_steps_a_stopped_rebase_only_its_own() {
    let mut value = test_view();

    // S: 0 menu of its own any more
    value.key(KeyEvent::new(KeyCode::Char('S'), KeyModifiers::SHIFT));

    assert!(value.modal_state.modal.is_none());

    let all = draw_ctx(&mut value, 120, 30).join("\n");

    assert!(
        all.contains(" ship ") && all.contains("push") && all.contains("open the pull request"),
        "{all}"
    );

    let (_, items) = value.ctx_items();

    assert!(
        items
            .iter()
            .any(|i| i.label == "push" && i.asks && i.section == "ship/git"),
        "push: outward, asks first"
    );

    // a stopped rebase: its steps only; the agents resolve at once, abort asks
    value.changes.stopped = Some(vec!["src/a.rs".into()]);
    let all = draw_ctx(&mut value, 120, 30).join("\n");

    assert!(
        all.contains("resolve the conflicts") && !all.contains("open the pull request"),
        "{all}"
    );
    assert!(matches!(ship(&mut value, "resolve the conflicts"), Act::Send(thread) if thread.contains("src/a.rs")));

    let (_, items) = value.ctx_items();

    assert!(
        items.iter().any(|i| i.label.contains("abort") && i.asks),
        "abort: asks first"
    );
}

#[test]
fn the_ship_section_hands_pr_steps_to_a_sub_thread_and_shows_the_open_pr() {
    let mut value = prepare_ship_steps();

    match ship(&mut value, "create PR draft") {
        Act::Sub(thread) => assert!(
            thread.starts_with("Open a draft pull request (`--draft`) for the branch `task/x`")
                && thread.contains("git push -u origin task/x")
                && thread.contains("--head task/x --base main --draft")
                && !thread.contains("HEAD`,")
                && thread.contains("uncommitted changes stay"),
            "{thread}"
        ),
        agent_value => panic!("{agent_value:?}"),
    }

    // a PR open: its label on top, update description, 0 create rows
    value.pull_request.pr = Some(crate::features::changes::Pr {
        number: 1234,
        url: "https://github.com/o/codehead/pull/1234".into(),
        ..Default::default()
    });
    let all = draw_ctx(&mut value, 140, 40).join("\n");

    assert!(
        all.contains("codehead#1234") && all.contains("update PR description") && !all.contains("create PR"),
        "{all}"
    );

    // update PR on the newest main: uncommitted files → 0 sub-thread; a clean tree → the task
    let up = value
        .ship_rows()
        .iter()
        .position(|result| result.label == "update PR on the newest main")
        .unwrap();

    assert!(value.ship_rows()[up].agent && value.ship_rows()[up].group == crate::features::changes::ShipGroup::Pr);
    assert!(
        matches!(ship(&mut value, "update PR on the newest main"), Act::Toast(thread_2) if thread_2.contains("uncommitted"))
    );

    let files = std::mem::take(&mut value.changes.files);

    match ship(&mut value, "update PR on the newest main") {
        Act::Sub(thread_value) => assert!(
            thread_value.contains("git rebase origin/main")
                && thread_value.contains("--force-with-lease origin task/x")
                && thread_value.contains("`task/x`"),
            "{thread_value}"
        ),
        agent_2 => panic!("{agent_2:?}"),
    }

    value.changes.files = files;

    assert!(
        matches!(ship(&mut value, "update PR description"), Act::Sub(thread_3) if thread_3.contains("gh pr edit 1234") && thread_3.contains("`headRefName` is not `task/x`"))
    );

    // ship: row 0, the state along, never merge
    let first = value.ship_rows()[0].label.clone();

    assert!(
        matches!(ship(&mut value, &first), Act::Sub(thread_4) if thread_4.contains("codehead#1234") && thread_4.contains("Never merge") && thread_4.contains("Never push to any branch but `task/x`") && !thread_4.contains("origin HEAD"))
    );
}

fn prepare_ship_steps() -> View {
    let mut value = test_view();

    value.focus_panel(true);

    assert!(value.scm.focus);
    assert_eq!(value.hints(), "tab: next · ctrl+.: actions, ship ");

    // 0 PR: ship, create PR, create PR draft; right of the panel, under a "ship" subheading
    let rows = draw_ctx(&mut value, 140, 40);
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
            .filter(|line_value| line_value.contains('├') && line_value.contains('┤'))
            .count(),
        3,
        "{all}"
    );

    let at = rows.iter().find(|line_2| line_2.contains("create PR draft")).unwrap();
    let px = usize::from(value.scm.rect.right());

    assert!(
        at.chars().take(px).all(|character| character != '›')
            && at.chars().nth(px).is_some_and(|character_value| character_value == '│'),
        "{at}"
    );

    // create PR draft: 0 commits ahead → 0 sub-thread
    assert!(matches!(ship(&mut value, "create PR draft"), Act::Toast(thread) if thread.contains("commit first")));

    // 2 commits ahead, 0 branch (detached / 0 git): 0 sub-thread
    value.changes.ahead = 2;

    assert!(matches!(ship(&mut value, "create PR draft"), Act::Toast(_)));

    // on a branch: the sub-thread's task names it, a draft, the commits only
    value.branch_override = Some("task/x".into());

    value
}
