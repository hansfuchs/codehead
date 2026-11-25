use super::*;

/// A running sub-thread (and its held messages): 0 chat rows. Its end: a message of its agent
/// tagged `subthread`, with its result (a box of its own, 0 merge with the agent's messages).
#[test]
fn a_sub_thread_shows_in_the_chat_only_at_its_end_as_its_agents_message() {
    let mut thread = Thread::default();
    let sub = |agent: Option<&str>, state: &str, text: &str| Event {
        ts: 1_795_090_200_000,
        agent: agent.map(String::from),
        kind: Kind::SubThread {
            id: "k1".into(),
            title: "commit the changes".into(),
            state: state.into(),
            text: text.into(),
        },
    };

    thread.push(&sub(None, "running", "@ada · commit the changes"));
    thread.push(&sub(None, "held", "messages wait"));

    assert!(thread.items.is_empty(), "{:?}", thread.items);

    thread.push(&Event {
        ts: 1_795_090_200_000,
        agent: Some("ada".into()),
        kind: Kind::Text {
            text: "own message".into(),
        },
    });
    thread.push(&sub(Some("ada"), "done", "## Result\n3 commits made"));
    thread.push(&sub(None, "failed", "its host ended before its agents were done"));

    assert!(matches!(&thread.items[1], Item::Kid { agent, state, .. } if agent == "ada" && state == "done"));
    assert!(matches!(&thread.items[2], Item::Kid { agent, .. } if agent == KID));

    let none = HashSet::new();
    let fold = Fold {
        runs: &none,
        all_runs: false,
        thoughts: Thoughts::Hidden,
        open_thoughts: &none,
        all_thoughts: false,
    };

    a_sub_thread_shows_in_the_chat_only_at_its_end_as_its_agents_message_verify_color(&thread, fold);
}

fn a_sub_thread_shows_in_the_chat_only_at_its_end_as_its_agents_message_verify_color(thread: &Thread, fold: Fold<'_>) {
    for look in [Look::Bubbles, Look::Lines] {
        let color = text(&draw(thread, look, 70, &Colors::new(), &[], &fold, &Picker::default()));
        let rows: Vec<&str> = color.lines().collect();
        let head = rows
            .iter()
            .position(|result| result.contains("ada") && result.contains(" subthread") && !result.contains("failed"))
            .unwrap_or_else(|| panic!("{look:?}\n{color}"));

        // the name: bubbles in the box's bottom border (its text above), lines on top
        let body = if look == Look::Bubbles {
            &rows[..head]
        } else {
            &rows[head + 1..]
        };

        assert!(
            body.iter()
                .any(|result_value| result_value.contains("↳ “commit the changes”"))
                && body.iter().any(|result_2| result_2.contains("3 commits made")),
            "{look:?}\n{color}"
        );
        assert!(
            rows.iter().any(|result_3| result_3.contains("subthread")
                && result_3.contains("· failed")
                && !result_3.contains("subthread subthread"))
                && color.contains("host ended"),
            "{look:?}\n{color}"
        );
        assert!(
            !color.contains("started") && !color.contains("messages wait"),
            "{look:?}\n{color}"
        );

        if look == Look::Bubbles {
            assert_eq!(
                rows.iter().filter(|result_4| result_4.starts_with('┌')).count(),
                3,
                "own box per end: {color}"
            );
        }
    }
}
