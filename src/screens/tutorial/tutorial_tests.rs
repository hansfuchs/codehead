use super::*;
use super::{
    panel::panel_lines,
    sandbox::{git, turn_end},
};
use crate::screens::thread_view::Pane;

fn crew(roles: &[&str]) -> Vec<Member> {
    roles
        .iter()
        .zip(["ADA", "NOVA", "ZED"])
        .map(|(result, count)| Member {
            name: count.into(),
            role: result.to_string(),
            harness: "claude".into(),
            avatar: "regi:1".into(),
            color: "blue".into(),
        })
        .collect()
}

fn ev(agent: Option<&str>, kind: Kind) -> Event {
    Event {
        ts: 0,
        agent: agent.map(String::from),
        kind,
    }
}

fn prompt(from: &str, to: &[&str], text: &str) -> Kind {
    Kind::Prompt {
        from: from.into(),
        to: to.iter().map(std::string::ToString::to_string).collect(),
        text: text.into(),
    }
}

fn peek(pane: crate::screens::thread_view::Pane, menu: bool) -> crate::screens::thread_view::Peek {
    crate::screens::thread_view::Peek { pane, menu, ide: false }
}

fn panel_text(character: &Coach) -> Vec<String> {
    panel_lines(character, 60)
        .iter()
        .map(|line| line.spans.iter().map(|text| text.content.to_string()).collect())
        .collect()
}

#[test]
fn coach_follows_a_pair_through_every_quest() {
    let mut character = Coach::new(&crew(&["builder", "reviewer"]));

    assert_eq!(character.current(), Some(Task::Send));
    assert!(character.see(&[ev(None, prompt("owner", &["ADA"], TASK))]));
    assert!(
        character.see_view(&peek(Pane::Files, false)),
        "the attachment's click: the files view"
    );
    assert!(character.text(character.current().unwrap()).0.contains("@NOVA #review"));

    // a message to the builder again: 0 step
    assert!(!character.see(&[ev(None, prompt("owner", &["ADA"], "more"))]));
    assert!(character.see(&[ev(None, prompt("owner", &["NOVA"], "@NOVA review the change"))]));

    // the reviewer's 1st turn end: not yet (the builder has not fixed)
    assert!(!character.see(&[
        ev(Some("NOVA"), prompt("NOVA", &["ADA"], "nit")),
        ev(Some("NOVA"), turn_end())
    ]));
    assert!(character.see(&[
        ev(Some("ADA"), prompt("ADA", &["NOVA"], "fixed")),
        ev(Some("NOVA"), turn_end())
    ]));

    // quest 3: the changes view, the IDE (mocked), then git's state (stage, commit)
    assert!(
        !character.see_view(&peek(Pane::Chat, true)),
        "the menu before the changes view: 0 step"
    );
    assert!(character.see_view(&peek(Pane::Changes, false)));
    assert!(character.see_view(&crate::screens::thread_view::Peek {
        pane: Pane::Changes,
        menu: false,
        ide: true
    }));
    assert!(!character.finished());
    assert!(character.reach(Task::Commit));
    assert!(
        character.finished() && character.at_stop(),
        "the basics done: the owner may stop"
    );
    assert!(panel_text(&character).iter().any(|line| line.contains("basics done")));

    // quest 4: the sub-thread's start and end
    assert!(character.see(&[ev(
        None,
        Kind::SubThread {
            id: "k".into(),
            title: "t".into(),
            state: "running".into(),
            text: String::new()
        }
    )]));

    assert_remaining_pair_quests(character);
}

fn assert_remaining_pair_quests(mut character: Coach) {
    assert!(!character.at_stop(), "an advanced task done: 0 stop offer");
    assert!(character.see(&[ev(
        None,
        Kind::SubThread {
            id: "k".into(),
            title: "t".into(),
            state: "done".into(),
            text: String::new()
        }
    )]));

    // quest 5: the PR link, the merge view, the merge
    assert!(character.see(&[ev(
        Some("ADA"),
        Kind::Text {
            text: format!("PR opened: {PR_URL}")
        }
    )]));
    assert!(character.see_view(&peek(Pane::Merge, false)));
    assert_eq!(character.current(), Some(Task::Merged));
    assert!(character.reach(Task::Merged));

    // quest 6: ctrl+p, the switch; quest 7: the menu, n, a flow, the start, the flow's end
    assert!(character.see_view(&peek(Pane::Chat, true)));
    assert!(character.reach(Task::Switch));
    assert!(character.see_view(&peek(Pane::Chat, true)));
    assert!(character.reach(Task::New));
    assert!(character.tick(Task::Flow));
    assert!(character.reach(Task::Start));
    assert!(character.see(&[ev(
        None,
        Kind::Flow {
            op: "finished".into(),
            text: String::new()
        }
    )]));
    assert_eq!(character.current(), Some(Task::Leave));
}

#[test]
fn reach_marks_skipped_tasks_and_solo_has_0_watch() {
    let mut character = Coach::new(&crew(&["builder"]));

    assert!(!character.order().contains(&Task::Watch));

    character.see(&[ev(None, prompt("owner", &["ADA"], TASK))]);

    assert!(character.text(Task::Mention).0.contains("@ADA #polish"));

    // the files view skipped: the 2nd message counts all the same
    assert!(character.see(&[ev(None, prompt("owner", &["ADA"], "@ADA add a 2nd line"))]));
    assert!(character.is_done(Task::Attach) && character.is_done(Task::Mention));

    // n in the menu early: the tasks before count as done
    assert!(character.reach(Task::New));
    assert!(
        character.is_done(Task::Changes)
            && character.is_done(Task::Commit)
            && character.is_done(Task::Merged)
            && character.is_done(Task::Menu2)
    );
    assert_eq!(character.current(), Some(Task::Flow));
    assert!(!character.reach(Task::New), "again: 0 change");
}

#[test]
fn an_advanced_event_before_the_commit_counts_0() {
    let mut character = Coach::new(&crew(&["builder"]));

    assert!(!character.see(&[ev(
        Some("ADA"),
        Kind::Text {
            text: format!("PR opened: {PR_URL}")
        }
    )]));
    assert!(!character.see(&[ev(
        None,
        Kind::SubThread {
            id: "k".into(),
            title: "t".into(),
            state: "running".into(),
            text: String::new()
        }
    )]));
    assert_eq!(character.current(), Some(Task::Send));
}

#[test]
fn panel_shows_the_current_quest_page_only() {
    let mut character = Coach::new(&crew(&["builder", "reviewer"]));

    character.reach(Task::Attach);
    let text = panel_text(&character);

    assert!(
        text.iter().any(|line| line.starts_with("2/8 work with the crew")),
        "{text:?}"
    );
    assert!(
        text.iter().any(|line_value| line_value.contains("#name: a shortcut")),
        "the concept"
    );
    assert!(
        text.iter()
            .any(|line_2| line_2.starts_with("  › back to the chat. call NOVA in"))
    );
    assert!(text.iter().any(|line_3| line_3.contains(" F1 · @ · # · ctrl+enter ")));
    assert!(
        text.iter()
            .any(|line_4| line_4.contains("codehead.sh/docs/concepts/messaging/"))
    );
    assert!(
        !text
            .iter()
            .any(|line_5| line_5.contains("start a task") || line_5.contains("send the task")),
        "0 other quest: {text:?}"
    );
    assert!(!text.iter().any(|line_6| line_6.contains("basics done")));

    // the dots: 1 done, this one, a gap before the advanced
    assert_eq!(text[0], " ✓ ◆ ○ │ ○ ○ ○ ○ ○");
}

/// The screen of a W x H terminal at C's state, as text rows.
fn screen(character: &Coach, width: u16, height: u16) -> Vec<String> {
    let mut thread = Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
    let (view, side) = layout(width, height);

    thread.draw(|file| chrome(file, character, false, view, side)).unwrap();
    let bytes = thread.backend().buffer();

    (0..height)
        .map(|y| (0..width).map(|x| bytes[(x, y)].symbol().to_string()).collect())
        .collect()
}

#[test]
fn the_longest_page_fits_the_smallest_window() {
    let mut character = Coach::new(&crew(&["planner", "builder", "reviewer"]));

    // quest 7: 5 concept lines, 5 tasks
    character.reach(Task::Start);
    let text = screen(&character, MIN_W, MIN_H);

    assert!(
        text.iter()
            .any(|line| line.contains("more: codehead.sh/docs/concepts/flows/")),
        "{}",
        text.join("\n")
    );

    // the stop offer + quest 4
    let mut character_value = Coach::new(&crew(&["builder", "reviewer"]));

    character_value.reach(Task::Commit);
    let text_value = screen(&character_value, MIN_W, MIN_H);

    assert!(
        text_value.iter().any(|line_value| line_value.contains("basics done"))
            && text_value
                .iter()
                .any(|line_2| line_2.contains("more: codehead.sh/docs/concepts/threads/")),
        "{}",
        text_value.join("\n")
    );
}

#[test]
fn asks_pr_needs_the_word() {
    assert!(asks_pr("@ADA open a PR"));
    assert!(asks_pr("a pull request, please"));
    assert!(!asks_pr("print the prompt"));
}

#[test]
fn mock_pr_shows_the_owners_commits_and_merges() {
    let directory = std::env::temp_dir().join(format!("codehead-tmock-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);

    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("hello.md"), "hello\n").unwrap();
    git(&directory, &["init", "-q", "-b", "main"]).unwrap();
    git(&directory, &["add", "."]).unwrap();
    git(&directory, &["commit", "-q", "-m", "hello"]).unwrap();
    std::fs::write(directory.join("hello.md"), "hi crew.\nwelcome aboard.\n").unwrap();
    git(&directory, &["commit", "-qam", "hello.md says hi"]).unwrap();
    let result = crate::infra::gh::PrRef::new("crew/hello", 7);
    let path = mock_pr(&result, &directory);

    assert_eq!(
        (
            path.title.as_str(),
            path.commits.len(),
            path.additions,
            path.deletions,
            path.files
        ),
        ("hello.md says hi", 1, 2, 1, 1)
    );
    assert_eq!(path.ready(), crate::infra::gh::Ready::Mergeable);
    assert!(mock_diff(&directory, None).contains("+welcome aboard."));

    mock_run(&directory, &crate::infra::gh::Op::Merge("SQUASH".into(), false)).unwrap();

    assert_eq!(mock_pr(&result, &directory).state, crate::infra::gh::State::Merged);

    let _ = std::fs::remove_dir_all(&directory);
}

#[test]
fn steps_of_flow_messages() {
    assert_eq!(
        step_of("[the owner] go\n\n[codehead flow: chore · step 2/3: verify]\n\nverify it").as_deref(),
        Some("verify")
    );
    assert_eq!(step_of("[from @NOVA] fixed"), None);
    assert_eq!(
        step_of("- a message that starts with `[codehead flow: ...]` is a step"),
        None,
        "the system prompt"
    );
}

#[test]
fn layout_puts_the_panel_right_and_keeps_the_view_under_the_split() {
    let (value, path) = layout(300, 60);
    let path_value = path.unwrap();

    assert!(
        value.width <= VIEW_MAX
            && path_value.x > value.right()
            && path_value.right() <= 300
            && path_value.width == PANEL_MAX
    );

    // a 3rd of the width for the quests
    let (_, path_2) = layout(150, 40);

    assert_eq!(path_2.unwrap().width, 50);

    let (value_value, path_3) = layout(90, 30);

    assert!(path_3.is_none() && value_value.width == 84);
}
