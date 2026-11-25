use super::*;

#[test]
fn a_message_without_a_name_goes_to_the_agents_of_the_last_one() {
    let (tx, _rx) = crate::infra::bus::channel();
    let mut view = View::new("defaulttest-0000", Path::new("/tmp"), "t", tx);

    view.agents.members = vec!["ada".into(), "bob".into()];

    // 0 owner message yet: the lead
    assert_eq!(view.default_to(), ["ada"]);

    view.thread.push(&store::Event {
        ts: 1,
        agent: None,
        kind: store::Kind::Prompt {
            from: "owner".into(),
            to: vec!["bob".into()],
            text: "@bob look".into(),
        },
    });

    // an agent's message in between changes 0
    view.thread.push(&store::Event {
        ts: 2,
        agent: Some("ada".into()),
        kind: store::Kind::Prompt {
            from: "ada".into(),
            to: vec!["bob".into()],
            text: "hi".into(),
        },
    });

    assert_eq!(view.default_to(), ["bob"]);

    view.prompt.input.set_text("and now?");

    assert_eq!(view.recipients(), ["bob"]);

    view.send_prompt("and now?");
    let ops = view.ops.borrow().clone();

    assert!(
        ops.iter()
            .any(|recorded| recorded["op"] == "prompt" && recorded["to"] == serde_json::json!(["bob"])),
        "{ops:?}"
    );

    // bob left: the lead again
    view.agents.members = vec!["ada".into()];

    assert_eq!(view.default_to(), ["ada"]);

    // @name of a known agent: the host routes (0 `to`)
    view.agents.names = vec![("cleo".into(), Color::Reset)];
    view.send_prompt("@cleo join");
    let last = view.ops.borrow().last().cloned().unwrap();

    assert!(last["to"].is_null(), "{last:?}");
}
