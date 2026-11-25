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
        .map(|(role, name)| Member {
            name: name.into(),
            role: role.to_string(),
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

fn panel_text(coach: &Coach) -> Vec<String> {
    panel_lines(coach, 60)
        .iter()
        .map(|line| line.spans.iter().map(|span| span.content.to_string()).collect())
        .collect()
}

#[test]
fn coach_follows_a_pair_through_every_quest() {
    let mut coach = Coach::new(&crew(&["builder", "reviewer"]));

    assert_eq!(coach.current(), Some(Task::Send));
    assert!(coach.see(&[ev(None, prompt("owner", &["ADA"], TASK))]));
    assert!(
        coach.see_view(&peek(Pane::Files, false)),
        "the attachment's click: the files view"
    );
    assert!(coach.text(coach.current().unwrap()).0.contains("@NOVA #review"));

    // a message to the builder again: 0 step
    assert!(!coach.see(&[ev(None, prompt("owner", &["ADA"], "more"))]));
    assert!(coach.see(&[ev(None, prompt("owner", &["NOVA"], "@NOVA review the change"))]));

    // the reviewer's 1st turn end: not yet (the builder has not fixed)
    assert!(!coach.see(&[
        ev(Some("NOVA"), prompt("NOVA", &["ADA"], "nit")),
        ev(Some("NOVA"), turn_end())
    ]));
    assert!(coach.see(&[
        ev(Some("ADA"), prompt("ADA", &["NOVA"], "fixed")),
        ev(Some("NOVA"), turn_end())
    ]));

    // quest 3: the changes view, the IDE (mocked), then git's state (stage, commit)
    assert!(
        !coach.see_view(&peek(Pane::Chat, true)),
        "the menu before the changes view: 0 step"
    );
    assert!(coach.see_view(&peek(Pane::Changes, false)));
    assert!(coach.see_view(&crate::screens::thread_view::Peek {
        pane: Pane::Changes,
        menu: false,
        ide: true
    }));
    assert!(!coach.finished());
    assert!(coach.reach(Task::Commit));
    assert!(
        coach.finished() && coach.at_stop(),
        "the basics done: the owner may stop"
    );
    assert!(panel_text(&coach).iter().any(|line| line.contains("basics done")));

    // quest 4: the sub-thread's start and end
    assert!(coach.see(&[ev(
        None,
        Kind::SubThread {
            id: "k".into(),
            title: "t".into(),
            state: "running".into(),
            text: String::new()
        }
    )]));

    assert_remaining_pair_quests(coach);
}

fn assert_remaining_pair_quests(mut coach: Coach) {
    assert!(!coach.at_stop(), "an advanced task done: 0 stop offer");
    assert!(coach.see(&[ev(
        None,
        Kind::SubThread {
            id: "k".into(),
            title: "t".into(),
            state: "done".into(),
            text: String::new()
        }
    )]));

    // quest 5: the PR link, the merge view, the merge
    assert!(coach.see(&[ev(
        Some("ADA"),
        Kind::Text {
            text: format!("PR opened: {PR_URL}")
        }
    )]));
    assert!(coach.see_view(&peek(Pane::Merge, false)));
    assert_eq!(coach.current(), Some(Task::Merged));
    assert!(coach.reach(Task::Merged));

    // quest 6: ctrl+p, the switch; quest 7: the menu, n, a flow, the start, the flow's end
    assert!(coach.see_view(&peek(Pane::Chat, true)));
    assert!(coach.reach(Task::Switch));
    assert!(coach.see_view(&peek(Pane::Chat, true)));
    assert!(coach.reach(Task::New));
    assert!(coach.tick(Task::Flow));
    assert!(coach.reach(Task::Start));
    assert!(coach.see(&[ev(
        None,
        Kind::Flow {
            op: "finished".into(),
            text: String::new()
        }
    )]));
    assert_eq!(coach.current(), Some(Task::Leave));
}

#[test]
fn reach_marks_skipped_tasks_and_solo_has_0_watch() {
    let mut coach = Coach::new(&crew(&["builder"]));

    assert!(!coach.order().contains(&Task::Watch));

    coach.see(&[ev(None, prompt("owner", &["ADA"], TASK))]);

    assert!(coach.text(Task::Mention).0.contains("@ADA #polish"));

    // the files view skipped: the 2nd message counts all the same
    assert!(coach.see(&[ev(None, prompt("owner", &["ADA"], "@ADA add a 2nd line"))]));
    assert!(coach.is_done(Task::Attach) && coach.is_done(Task::Mention));

    // n in the menu early: the tasks before count as done
    assert!(coach.reach(Task::New));
    assert!(
        coach.is_done(Task::Changes)
            && coach.is_done(Task::Commit)
            && coach.is_done(Task::Merged)
            && coach.is_done(Task::Menu2)
    );
    assert_eq!(coach.current(), Some(Task::Flow));
    assert!(!coach.reach(Task::New), "again: 0 change");
}

#[test]
fn an_advanced_event_before_the_commit_counts_nothing() {
    let mut coach = Coach::new(&crew(&["builder"]));

    assert!(!coach.see(&[ev(
        Some("ADA"),
        Kind::Text {
            text: format!("PR opened: {PR_URL}")
        }
    )]));
    assert!(!coach.see(&[ev(
        None,
        Kind::SubThread {
            id: "k".into(),
            title: "t".into(),
            state: "running".into(),
            text: String::new()
        }
    )]));
    assert_eq!(coach.current(), Some(Task::Send));
}

#[test]
fn panel_shows_the_current_quest_page_only() {
    let mut coach = Coach::new(&crew(&["builder", "reviewer"]));

    coach.reach(Task::Attach);
    let text = panel_text(&coach);

    assert!(
        text.iter().any(|line| line.starts_with("2/8 work with the crew")),
        "{text:?}"
    );
    assert!(
        text.iter().any(|line| line.contains("#name: a shortcut")),
        "the concept"
    );
    assert!(
        text.iter()
            .any(|line| line.starts_with("  › back to the chat. call NOVA in"))
    );
    assert!(text.iter().any(|line| line.contains(" F1 · @ · # · ctrl+enter ")));
    assert!(
        text.iter()
            .any(|line| line.contains("codehead.sh/docs/concepts/messaging/"))
    );
    assert!(
        !text
            .iter()
            .any(|line| line.contains("start a task") || line.contains("send the task")),
        "0 other quest: {text:?}"
    );
    assert!(!text.iter().any(|line| line.contains("basics done")));

    // the dots: 1 done, this one, a gap before the advanced
    assert_eq!(text[0], " ✓ ◆ ○ │ ○ ○ ○ ○ ○");
}

/// The screen of a W x H terminal at C's state, as text rows.
fn screen(coach: &Coach, width: u16, height: u16) -> Vec<String> {
    let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
    let (view, side) = layout(width, height);

    terminal.draw(|frame| chrome(frame, coach, false, view, side)).unwrap();
    let buffer = terminal.backend().buffer();

    (0..height)
        .map(|y| (0..width).map(|x| buffer[(x, y)].symbol().to_string()).collect())
        .collect()
}

#[test]
fn the_longest_page_fits_the_smallest_window() {
    let mut coach = Coach::new(&crew(&["planner", "builder", "reviewer"]));

    // quest 7: 5 concept lines, 5 tasks
    coach.reach(Task::Start);
    let text = screen(&coach, MIN_W, MIN_H);

    assert!(
        text.iter()
            .any(|line| line.contains("more: codehead.sh/docs/concepts/flows/")),
        "{}",
        text.join("\n")
    );

    // the stop offer + quest 4
    let mut pair = Coach::new(&crew(&["builder", "reviewer"]));

    pair.reach(Task::Commit);
    let pair_text = screen(&pair, MIN_W, MIN_H);

    assert!(
        pair_text.iter().any(|line| line.contains("basics done"))
            && pair_text
                .iter()
                .any(|line| line.contains("more: codehead.sh/docs/concepts/threads/")),
        "{}",
        pair_text.join("\n")
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
    let dir = std::env::temp_dir().join(format!("codehead-tmock-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("hello.md"), "hello\n").unwrap();
    git(&dir, &["init", "-q", "-b", "main"]).unwrap();
    git(&dir, &["add", "."]).unwrap();
    git(&dir, &["commit", "-q", "-m", "hello"]).unwrap();
    std::fs::write(dir.join("hello.md"), "hi crew.\nwelcome aboard.\n").unwrap();
    git(&dir, &["commit", "-qam", "hello.md says hi"]).unwrap();
    let pr_ref = crate::infra::gh::PrRef::new("crew/hello", 7);
    let pull_request = mock_pr(&pr_ref, &dir);

    assert_eq!(
        (
            pull_request.title.as_str(),
            pull_request.commits.len(),
            pull_request.additions,
            pull_request.deletions,
            pull_request.files
        ),
        ("hello.md says hi", 1, 2, 1, 1)
    );
    assert_eq!(pull_request.ready(), crate::infra::gh::Ready::Mergeable);
    assert!(mock_diff(&dir, None).contains("+welcome aboard."));

    mock_run(&dir, &crate::infra::gh::Op::Merge("SQUASH".into(), false)).unwrap();

    assert_eq!(mock_pr(&pr_ref, &dir).state, crate::infra::gh::State::Merged);

    let _ = std::fs::remove_dir_all(&dir);
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
    let (view, side) = layout(300, 60);
    let side = side.unwrap();

    assert!(view.width <= VIEW_MAX && side.x > view.right() && side.right() <= 300 && side.width == PANEL_MAX);

    // a 3rd of the width for the quests
    let (_, side) = layout(150, 40);

    assert_eq!(side.unwrap().width, 50);

    let (view, side) = layout(90, 30);

    assert!(side.is_none() && view.width == 84);
}
