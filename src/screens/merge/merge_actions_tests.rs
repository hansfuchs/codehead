use super::test_support::*;
use super::*;

/// `CODEHEAD_GH_LIVE=1 cargo test live_merge -- --ignored --nocapture`: a real PR drawn
/// (`CODEHEAD_GH_PR=owner/repo#n`: that PR).
#[test]
#[ignore = "requires live GitHub pull request"]
fn live_merge_draws_a_real_pr() {
    let pr = std::env::var("CODEHEAD_GH_PR")
        .ok()
        .and_then(|spec| {
            spec.split_once('#')
                .and_then(|(repo, number)| Some(PrRef::new(repo, number.parse().ok()?)))
        })
        .unwrap_or_else(|| PrRef::new("cli/cli", 15100));

    let mut merge = Merge::one(pr.clone());

    merge.read(pr);
    merge.poll(false);
    let tab = merge.tab().unwrap();

    assert!(tab.pr.is_some(), "{:?}", tab.error);

    tab.checks_open = true;
    let width = std::env::var("CODEHEAD_GH_W")
        .ok()
        .and_then(|width| width.parse().ok())
        .unwrap_or(120);

    println!("{}", draw(&mut merge, width, 90));
    merge.set_sub(Sub::Changes);
    let backend = ratatui::backend::TestBackend::new(140, 40);
    let mut term = ratatui::Terminal::new(backend).unwrap();

    // the diff: read inline (tests), applied by the next draws
    for _ in 0..100 {
        term.draw(|frame| merge.draw(frame, frame.area(), true)).unwrap();

        if merge.changes_view().is_some_and(|view| !view.changes.files.is_empty()) {
            break;
        }

        std::thread::sleep(Duration::from_millis(100));
    }

    // a box with a GitHub thread in view
    if let Some(view) = merge.changes_view() {
        let i = view
            .changes
            .files
            .iter()
            .position(|file| file.path == "internal/authflow/revoke.go")
            .unwrap_or(0);

        view.jump_to_file(i);
    }

    term.draw(|frame| merge.draw(frame, frame.area(), true)).unwrap();
    let buf = term.backend().buffer().clone();

    println!(
        "{}",
        (0..40)
            .map(|y| (0..140).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn merge_when_ready_shows_its_state_at_each_step() {
    let mut merge = with(pr());

    merge.tab().unwrap().focus = Some(Stop::Merge);

    // the 1st enter arms: the button and the banner say so
    key(&mut merge, KeyCode::Enter);
    let text = draw(&mut merge, 100, 40);

    assert!(
        text.contains("⏎ again: merge when ready · squash") && text.contains("enter again: merge when ready (squash)"),
        "{text}"
    );

    // the 2nd: the gh call on its way
    key(&mut merge, KeyCode::Enter);

    assert_eq!(
        merge.tab_ref().unwrap().input.busy,
        Some(Op::Merge("SQUASH".into(), true))
    );

    let text = draw(&mut merge, 100, 40);

    assert!(
        text.contains("auto-merge on…") && text.contains("gh: merge when ready (squash)…"),
        "{text}"
    );

    // done: auto-merge on next to the state at once, the button turns it off
    done(&mut merge, Op::Merge("SQUASH".into(), true));

    assert_eq!(merge.note.as_deref(), Some("auto-merge on: merges when ready"));

    let text = draw(&mut merge, 100, 40);

    assert!(
        text.contains("checks failed · auto-merge on (squash)") && text.contains("cancel auto-merge"),
        "{text}"
    );

    let act = key(&mut merge, KeyCode::Enter);

    assert!(
        matches!(act, Act::Toast(ref toast) if toast == "enter again: auto-merge off"),
        "{act:?}"
    );

    key(&mut merge, KeyCode::Enter);

    assert_eq!(merge.tab_ref().unwrap().input.busy, Some(Op::DisableAuto));

    done(&mut merge, Op::DisableAuto);

    assert!(draw(&mut merge, 100, 40).contains("merge when ready · squash"));
}

#[test]
fn a_merge_queue_shows_its_entry_and_lets_the_pr_leave_it() {
    let mut pr = pr();

    pr.queue = true;
    pr.id = "PR_1".into();
    pr.checks.clear();
    pr.merge_state = "CLEAN".into();
    let mut merge = with(pr);

    merge.tab().unwrap().focus = Some(Stop::Merge);

    assert!(draw(&mut merge, 100, 40).contains(" add to merge queue "));

    let act = key(&mut merge, KeyCode::Enter);

    assert!(
        matches!(act, Act::Toast(ref toast) if toast == "enter again: add to the merge queue"),
        "{act:?}"
    );

    key(&mut merge, KeyCode::Enter);
    done(&mut merge, Op::Merge("SQUASH".into(), false));

    assert_eq!(merge.note.as_deref(), Some("added to the merge queue"));

    let text = draw(&mut merge, 100, 40);

    assert!(
        text.contains("◷ in the merge queue") && text.contains("leave merge queue") && !text.contains("merged"),
        "{text}"
    );

    // the read: its position, state, time
    merge.tab().unwrap().pr.as_mut().unwrap().queued = Some(gh::Queued {
        position: 2,
        state: "AWAITING_CHECKS".into(),
        eta: 508,
    });
    let text = draw(&mut merge, 100, 40);

    assert!(
        text.contains("position 2 · awaiting checks · merges in ~8m 28s"),
        "{text}"
    );

    key(&mut merge, KeyCode::Enter);
    key(&mut merge, KeyCode::Enter);

    assert_eq!(merge.tab_ref().unwrap().input.busy, Some(Op::Dequeue("PR_1".into())));
}

#[test]
fn merge_when_ready_says_why_when_the_repo_has_auto_merge_off() {
    let mut pr = pr();

    pr.auto_allowed = false;
    let mut merge = with(pr);

    merge.tab().unwrap().focus = Some(Stop::Merge);

    let act = key(&mut merge, KeyCode::Enter);

    assert!(
        matches!(act, Act::Toast(ref toast) if toast.contains("auto-merge is off in the repo settings")),
        "{act:?}"
    );

    // 0 arm, 0 call
    key(&mut merge, KeyCode::Enter);

    assert!(merge.tab_ref().unwrap().input.busy.is_none());
    assert!(draw(&mut merge, 100, 40).contains("auto-merge is off in the repo settings"));
}

#[test]
fn labels_and_buttons_are_dim_without_the_focus_full_with_it() {
    let mut merge = with(pr());

    merge.tab().unwrap().focus = Some(Stop::Title);
    let unfocused = buffer(&mut merge, 100, 40);
    let (bug, merge_button, status) = (
        bg_of(&unfocused, " bug × "),
        bg_of(&unfocused, " merge when ready"),
        bg_of(&unfocused, " ● open"),
    );

    assert!(
        bug != hex("d73a4a") && merge_button != theme::pal().success() && status != theme::pal().success(),
        "{bug:?} {merge_button:?} {status:?}"
    );

    for (stop, text, full) in [
        (Stop::Label(0), " bug × ", hex("d73a4a")),
        (Stop::Merge, " merge when ready", theme::pal().success()),
        (Stop::Status, " ● open", theme::pal().success()),
    ] {
        merge.tab().unwrap().focus = Some(stop);
        let focused = buffer(&mut merge, 100, 40);

        assert_eq!(bg_of(&focused, text), full, "{stop:?}");
    }
}

#[test]
fn the_merge_method_merge_is_not_named_twice_failed_optional_checks_show() {
    let mut pr = pr();

    pr.method = "MERGE".into();
    pr.merge_state = "UNSTABLE".into();
    pr.checks[0].required = false;
    let mut merge = with(pr);
    let text = draw(&mut merge, 100, 40);

    assert!(
        text.contains("✓ ready to merge") && text.contains(" merge ") && !text.contains("merge · merge"),
        "{text}"
    );
    assert!(
        text.contains("all requirements are met · 1 check failed (not required)"),
        "{text}"
    );

    merge.tab().unwrap().focus = Some(Stop::Merge);
    let act = key(&mut merge, KeyCode::Enter);

    assert_eq!(act, Act::Toast("enter again: merge".into()));
}

#[test]
fn a_label_without_rights_has_no_remove_mark() {
    let mut pr = pr();

    pr.can_edit = false;
    let mut merge = with(pr);
    let text = draw(&mut merge, 100, 40);

    assert!(text.contains(" bug ") && !text.contains("bug ×"), "{text}");

    merge.tab().unwrap().focus = Some(Stop::Label(0));
    let act = key(&mut merge, KeyCode::Enter);

    assert!(
        matches!(act, Act::Toast(ref toast) if toast.contains("0 rights")),
        "{act:?}"
    );
}

#[test]
fn the_context_names_the_pr_and_its_failed_checks() {
    let merge = with(pr());
    let context = merge.context().unwrap();

    assert!(
        context.contains("https://github.com/acme/platform/pull/12")
            && context.contains("CI / lint (u1)")
            && context.contains("src/x.rs:4 rev: why?"),
        "{context}"
    );
}
