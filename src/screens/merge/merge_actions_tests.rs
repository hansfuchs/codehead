use super::test_support::*;
use super::*;

/// `CODEHEAD_GH_LIVE=1 cargo test live_merge -- --ignored --nocapture`: a real PR drawn
/// (`CODEHEAD_GH_PR=owner/repo#n`: that PR).
#[test]
#[ignore = "requires live GitHub pull request"]
fn live_merge_draws_a_real_pr() {
    let pr = std::env::var("CODEHEAD_GH_PR")
        .ok()
        .and_then(|text| {
            text.split_once('#')
                .and_then(|(result, count)| Some(PrRef::new(result, count.parse().ok()?)))
        })
        .unwrap_or_else(|| PrRef::new("cli/cli", 15100));

    let mut message = Merge::one(pr.clone());

    message.read(pr);
    message.poll(false);
    let thread_value = message.tab().unwrap();

    assert!(thread_value.pr.is_some(), "{:?}", thread_value.error);

    thread_value.checks_open = true;
    let width_value = std::env::var("CODEHEAD_GH_W")
        .ok()
        .and_then(|width| width.parse().ok())
        .unwrap_or(120);

    println!("{}", draw(&mut message, width_value, 90));
    message.set_sub(Sub::Changes);
    let backend = ratatui::backend::TestBackend::new(140, 40);
    let mut term = ratatui::Terminal::new(backend).unwrap();

    // the diff: read inline (tests), applied by the next draws
    for _ in 0..100 {
        term.draw(|file| message.draw(file, file.area(), true)).unwrap();

        if message
            .changes_view()
            .is_some_and(|character| !character.changes.files.is_empty())
        {
            break;
        }

        std::thread::sleep(Duration::from_millis(100));
    }

    // a box with a GitHub thread in view
    if let Some(character_value) = message.changes_view() {
        let i = character_value
            .changes
            .files
            .iter()
            .position(|file_value| file_value.path == "internal/authflow/revoke.go")
            .unwrap_or(0);

        character_value.jump_to_file(i);
    }

    term.draw(|file_2| message.draw(file_2, file_2.area(), true)).unwrap();
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
    let mut message = with(pr());

    message.tab().unwrap().focus = Some(Stop::Merge);

    // the 1st enter arms: the button and the banner say so
    key(&mut message, KeyCode::Enter);
    let text = draw(&mut message, 100, 40);

    assert!(
        text.contains("⏎ again: merge when ready · squash") && text.contains("enter again: merge when ready (squash)"),
        "{text}"
    );

    // the 2nd: the gh call on its way
    key(&mut message, KeyCode::Enter);

    assert_eq!(
        message.tab_ref().unwrap().input.busy,
        Some(Op::Merge("SQUASH".into(), true))
    );

    let text_value = draw(&mut message, 100, 40);

    assert!(
        text_value.contains("auto-merge on…") && text_value.contains("gh: merge when ready (squash)…"),
        "{text_value}"
    );

    // done: auto-merge on next to the state at once, the button turns it off
    done(&mut message, Op::Merge("SQUASH".into(), true));

    assert_eq!(message.note.as_deref(), Some("auto-merge on: merges when ready"));

    let text_2 = draw(&mut message, 100, 40);

    assert!(
        text_2.contains("checks failed · auto-merge on (squash)") && text_2.contains("cancel auto-merge"),
        "{text_2}"
    );

    let agent = key(&mut message, KeyCode::Enter);

    assert!(
        matches!(agent, Act::Toast(ref thread) if thread == "enter again: auto-merge off"),
        "{agent:?}"
    );

    key(&mut message, KeyCode::Enter);

    assert_eq!(message.tab_ref().unwrap().input.busy, Some(Op::DisableAuto));

    done(&mut message, Op::DisableAuto);

    assert!(draw(&mut message, 100, 40).contains("merge when ready · squash"));
}

#[test]
fn a_merge_queue_shows_its_entry_and_lets_the_pr_leave_it() {
    let mut path = pr();

    path.queue = true;
    path.id = "PR_1".into();
    path.checks.clear();
    path.merge_state = "CLEAN".into();
    let mut message = with(path);

    message.tab().unwrap().focus = Some(Stop::Merge);

    assert!(draw(&mut message, 100, 40).contains(" add to merge queue "));

    let agent = key(&mut message, KeyCode::Enter);

    assert!(
        matches!(agent, Act::Toast(ref thread) if thread == "enter again: add to the merge queue"),
        "{agent:?}"
    );

    key(&mut message, KeyCode::Enter);
    done(&mut message, Op::Merge("SQUASH".into(), false));

    assert_eq!(message.note.as_deref(), Some("added to the merge queue"));

    let text = draw(&mut message, 100, 40);

    assert!(
        text.contains("◷ in the merge queue") && text.contains("leave merge queue") && !text.contains("merged"),
        "{text}"
    );

    // the read: its position, state, time
    message.tab().unwrap().pr.as_mut().unwrap().queued = Some(gh::Queued {
        position: 2,
        state: "AWAITING_CHECKS".into(),
        eta: 508,
    });
    let text_value = draw(&mut message, 100, 40);

    assert!(
        text_value.contains("position 2 · awaiting checks · merges in ~8m 28s"),
        "{text_value}"
    );

    key(&mut message, KeyCode::Enter);
    key(&mut message, KeyCode::Enter);

    assert_eq!(message.tab_ref().unwrap().input.busy, Some(Op::Dequeue("PR_1".into())));
}

#[test]
fn merge_when_ready_says_why_when_the_repo_has_auto_merge_off() {
    let mut path = pr();

    path.auto_allowed = false;
    let mut message = with(path);

    message.tab().unwrap().focus = Some(Stop::Merge);

    let agent = key(&mut message, KeyCode::Enter);

    assert!(
        matches!(agent, Act::Toast(ref thread) if thread.contains("auto-merge is off in the repo settings")),
        "{agent:?}"
    );

    // 0 arm, 0 call
    key(&mut message, KeyCode::Enter);

    assert!(message.tab_ref().unwrap().input.busy.is_none());
    assert!(draw(&mut message, 100, 40).contains("auto-merge is off in the repo settings"));
}

#[test]
fn labels_and_buttons_are_dim_without_the_focus_full_with_it() {
    let mut message = with(pr());

    message.tab().unwrap().focus = Some(Stop::Title);
    let bytes = buffer(&mut message, 100, 40);
    let (bug, merge, status) = (
        bg_of(&bytes, " bug × "),
        bg_of(&bytes, " merge when ready"),
        bg_of(&bytes, " ● open"),
    );

    assert!(
        bug != hex("d73a4a") && merge != theme::pal().success() && status != theme::pal().success(),
        "{bug:?} {merge:?} {status:?}"
    );

    for (text_value, text, full) in [
        (Stop::Label(0), " bug × ", hex("d73a4a")),
        (Stop::Merge, " merge when ready", theme::pal().success()),
        (Stop::Status, " ● open", theme::pal().success()),
    ] {
        message.tab().unwrap().focus = Some(text_value);
        let bytes_value = buffer(&mut message, 100, 40);

        assert_eq!(bg_of(&bytes_value, text), full, "{text_value:?}");
    }
}

#[test]
fn the_merge_method_merge_is_not_named_twice_failed_optional_checks_show() {
    let mut path = pr();

    path.method = "MERGE".into();
    path.merge_state = "UNSTABLE".into();
    path.checks[0].required = false;
    let mut message = with(path);
    let text = draw(&mut message, 100, 40);

    assert!(
        text.contains("✓ ready to merge") && text.contains(" merge ") && !text.contains("merge · merge"),
        "{text}"
    );
    assert!(
        text.contains("all requirements are met · 1 check failed (not required)"),
        "{text}"
    );

    message.tab().unwrap().focus = Some(Stop::Merge);
    let agent = key(&mut message, KeyCode::Enter);

    assert_eq!(agent, Act::Toast("enter again: merge".into()));
}

#[test]
fn a_label_without_rights_has_no_remove_mark() {
    let mut path = pr();

    path.can_edit = false;
    let mut message = with(path);
    let text = draw(&mut message, 100, 40);

    assert!(text.contains(" bug ") && !text.contains("bug ×"), "{text}");

    message.tab().unwrap().focus = Some(Stop::Label(0));
    let agent = key(&mut message, KeyCode::Enter);

    assert!(
        matches!(agent, Act::Toast(ref thread) if thread.contains("0 rights")),
        "{agent:?}"
    );
}

#[test]
fn the_context_names_the_pr_and_its_failed_checks() {
    let message = with(pr());
    let character = message.context().unwrap();

    assert!(
        character.contains("https://github.com/acme/platform/pull/12")
            && character.contains("CI / lint (u1)")
            && character.contains("src/x.rs:4 rev: why?"),
        "{character}"
    );
}
