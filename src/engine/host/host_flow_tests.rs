use super::*;

#[test]
pub(crate) fn a_flow_goes_with_the_task_moves_on_reports_and_waits_at_gates() {
    let _env = crate::engine::ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let thread = store::Thread::new("flow test", "task", Path::new("/tmp"), vec!["ada".into()]);

    std::fs::create_dir_all(store::thread_dir(&thread.id)).unwrap();
    thread.save().unwrap();
    let (htx, _hrx) = mpsc::channel();
    let mut path = test_host(thread.id.clone(), htx);
    let flow = || store::load(&thread.id).unwrap().flow_of("ada").unwrap();

    // set before the 1st message: the step waits for the task
    path.cmd(&json!({"op": "flow_set", "flow": "chore"})).unwrap();

    assert!(path.later.is_empty() && !flow().sent);

    path.cmd(&json!({"op": "prompt", "text": "do it"})).unwrap();
    let (to, message) = path.later.pop().unwrap();

    assert_eq!(to, "ada");
    assert!(
        message.starts_with("[codehead flow: chore · step 1/3: implement]") && message.ends_with("do it"),
        "{message}"
    );
    assert!(flow().sent);

    // only the flow's agent reports it (bob has 0 flow); the reply holds the next step
    assert!(path.cmd(&json!({"op": "flow_step", "agent": "bob"})).is_err());

    let result = path.cmd(&json!({"op": "flow_step", "agent": "ada"})).unwrap();

    assert!(result["text"].as_str().unwrap().contains("step 2/3: verify"));

    // a switch after the flow: the queued flow starts when it ends
    path.cmd(&json!({"op": "flow_set", "flow": "new feature", "mode": "after"}))
        .unwrap();

    assert_eq!(flow().queued.unwrap().flow, "new-feature");

    path.cmd(&json!({"op": "flow_step", "agent": "ada"})).unwrap();
    let result_value = path.cmd(&json!({"op": "flow_step", "agent": "ada"})).unwrap();

    assert!(
        result_value["text"]
            .as_str()
            .unwrap()
            .contains("[codehead flow: new feature · step 1/9: scrutinize-task]"),
        "{result_value}"
    );
    assert_eq!(flow().flow, "new-feature");

    // a gate: the agent waits, the owner's outcome picks the edge
    path.cmd(&json!({"op": "flow_jump", "node": 3})).unwrap();
    let (_, message_value) = path.later.pop().unwrap();

    assert!(message_value.contains("plan approval") && message_value.contains("Gate:"));

    let result_2 = path.cmd(&json!({"op": "flow_step", "agent": "ada"})).unwrap();

    assert_eq!(result_2["wait"], "owner");
    assert!(
        path.cmd(&json!({"op": "flow_step", "agent": "ada", "pass_gate": true}))
            .is_err(),
        "ada is 0 autopilot agent of the roster"
    );

    verify_a_flow_goes_with_the_task_moves_on_reports_and_waits_at_gates(&thread, path);
}

fn verify_a_flow_goes_with_the_task_moves_on_reports_and_waits_at_gates(thread: &store::Thread, mut path: Host) {
    path.cmd(&json!({"op": "flow_answer", "choice": "changes", "note": "split it"}))
        .unwrap();
    let (_, message_2) = path.later.pop().unwrap();

    assert!(
        message_2.starts_with("[the owner] split it") && message_2.contains("step 3/9: plan"),
        "{message_2}"
    );

    // peer review: the flow's own agent asks another agent (roles are gone)
    path.cmd(&json!({"op": "flow_jump", "node": 6})).unwrap();
    let (to, message_3) = path.later.pop().unwrap();

    assert!(
        to == "ada" && message_3.contains("peer-review") && message_3.contains("list_agents"),
        "{message_3}"
    );

    let log: Vec<String> = store::events(&thread.id)
        .into_iter()
        .filter_map(|error_value| match error_value.kind {
            Kind::Flow { text, .. } => Some(text),
            _ => None,
        })
        .collect();

    assert!(
        log.iter()
            .any(|line| line
                .starts_with("⏸ new feature · gate plan approval: waits for the owner (approve · changes)")),
        "{log:?}"
    );
    assert!(
        log.iter()
            .any(|line_value| line_value == "▶ chore · step 1/3 implement · @ada"),
        "steps name their flow: {log:?}"
    );

    let _ = std::fs::remove_dir_all(store::thread_dir(&thread.id));
}
/// 2 agents, 2 flows: each step goes to its flow's agent (with the task at the start), each
/// `flow_step` moves only its own flow, owner ops name the agent once 2+ flows run.
#[test]
pub(crate) fn each_agent_runs_its_own_flow() {
    let _env = crate::engine::ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let thread = store::Thread::new("2 flows", "task", Path::new("/tmp"), vec!["ada".into(), "zed".into()]);

    std::fs::create_dir_all(store::thread_dir(&thread.id)).unwrap();
    thread.save().unwrap();
    let (htx, _hrx) = mpsc::channel();
    let mut path = test_host(thread.id.clone(), htx);
    let flow = |agent_value: &str| store::load(&thread.id).unwrap().flow_of(agent_value);

    path.cmd(&json!({"op": "flow_set", "flow": "chore", "agent": "ada"}))
        .unwrap();
    path.cmd(&json!({"op": "flow_set", "flow": "code review", "agent": "zed"}))
        .unwrap();

    assert!(
        path.cmd(&json!({"op": "flow_set", "flow": "chore", "agent": "bob"}))
            .is_err(),
        "bob is not on the thread"
    );

    // the task goes to the lead; each agent gets its step 1 with the task under it
    path.cmd(&json!({"op": "prompt", "text": "do it"})).unwrap();
    let ada = path
        .later
        .iter()
        .find(|(agent_2, _)| agent_2 == "ada")
        .map(|(_, message)| message.clone())
        .unwrap();
    let zed = path
        .later
        .iter()
        .find(|(agent_3, _)| agent_3 == "zed")
        .map(|(_, message_value)| message_value.clone())
        .unwrap();

    assert!(
        ada.starts_with("[codehead flow: chore · step 1/3: implement]") && ada.ends_with("do it"),
        "{ada}"
    );
    assert!(
        zed.starts_with("[codehead flow: code review · step 1/2: review-diff]") && zed.ends_with("do it"),
        "{zed}"
    );
    assert_eq!(path.later.len(), 2);

    // zed reports its step: only its flow moves
    let result = path.cmd(&json!({"op": "flow_step", "agent": "zed"})).unwrap();

    assert!(
        result["text"].as_str().unwrap().contains("step 2/2: report"),
        "{result}"
    );
    assert_eq!((flow("ada").unwrap().node, flow("zed").unwrap().node), (0, 1));

    // owner ops: 2 flows → the agent is named
    let error_value = path.cmd(&json!({"op": "flow_skip"})).unwrap_err();

    assert!(
        error_value.contains("@ada") && error_value.contains("@zed"),
        "{error_value}"
    );

    path.cmd(&json!({"op": "flow_skip", "agent": "ada"})).unwrap();

    assert_eq!(flow("ada").unwrap().node, 1);

    // the flow lines are the agent's events
    verify_each_agent_runs_its_own_flow(&thread, path, flow);
}

fn verify_each_agent_runs_its_own_flow(
    thread: &store::Thread,
    mut path: Host,
    flow: impl Fn(&str) -> Option<crate::engine::flows::run::FlowState>,
) {
    let flows: Vec<(Option<String>, String)> = store::events(&thread.id)
        .into_iter()
        .filter_map(|error_2| match error_2.kind {
            Kind::Flow { text, .. } => Some((error_2.agent, text)),
            _ => None,
        })
        .collect();

    assert!(
        flows
            .iter()
            .any(|(agent_4, line)| agent_4.as_deref() == Some("zed") && line.starts_with("▶ code review · step 1/2")),
        "{flows:?}"
    );

    // zed's flow goes, ada's stays
    path.cmd(&json!({"op": "flow_clear", "agent": "zed"})).unwrap();

    assert!(flow("zed").is_none() && flow("ada").is_some());

    // bob replaces ada: ada's flow goes to bob, its step again with bob's 1st message
    path.later.clear();
    path.cmd(&json!({"op": "leave", "agent": "ada", "to": "bob"})).unwrap();

    assert!(flow("ada").is_none() && flow("bob").is_some_and(|file| !file.sent && file.node == 1));

    path.cmd(&json!({"op": "prompt", "to": ["bob"], "text": "take over"}))
        .unwrap();
    let bob = path
        .later
        .iter()
        .find(|(agent_5, _)| agent_5 == "bob")
        .map(|(_, message_2)| message_2.clone())
        .unwrap();

    assert!(
        bob.starts_with("[codehead flow: chore · step 2/3") && bob.ends_with("take over"),
        "{bob}"
    );

    // zed leaves (0 replacement): 0 flow left
    path.cmd(&json!({"op": "leave", "agent": "zed"})).unwrap();

    assert!(flow("zed").is_none());

    let _ = std::fs::remove_dir_all(store::thread_dir(&thread.id));
}
/// An older version's thread: its 1 flow is the lead's; a save keeps `flow` = the lead's.
/// A rename keeps the flow with its agent: the lead of an older thread (its 1 flow) and a
/// non-lead; 0 copy under the old name.
#[test]
pub(crate) fn a_renamed_agent_keeps_its_flow() {
    let mut thread = store::Thread::new("rename", "task", Path::new("/tmp"), vec!["ada".into(), "zed".into()]);

    thread.flow = Some(crate::engine::flows::run::FlowState {
        flow: "chore".into(),
        ..Default::default()
    });
    thread.set_flow(
        "zed",
        Some(crate::engine::flows::run::FlowState {
            flow: "code-review".into(),
            ..Default::default()
        }),
    );
    thread.rename_agent("ada", "nova");

    assert_eq!(thread.agents, ["nova", "zed"]);
    assert_eq!(thread.flow_map().keys().cloned().collect::<Vec<_>>(), ["nova", "zed"]);
    assert_eq!(thread.flow_of("nova").map(|file| file.flow), Some("chore".into()));

    thread.rename_agent("zed", "rex");

    assert_eq!(
        thread.flow_of("rex").map(|file_value| file_value.flow),
        Some("code-review".into())
    );
    assert!(thread.flow_of("zed").is_none() && thread.flow_of("ada").is_none());
}
#[test]
pub(crate) fn an_old_thread_flow_belongs_to_the_lead() {
    let mut thread = store::Thread::new("old", "task", Path::new("/tmp"), vec!["ada".into(), "zed".into()]);

    thread.flow = Some(crate::engine::flows::run::FlowState {
        flow: "chore".into(),
        node: 2,
        ..Default::default()
    });

    assert_eq!(thread.flow_of("ada").map(|file| file.node), Some(2));
    assert!(thread.flow_of("zed").is_none());

    thread.set_flow(
        "zed",
        Some(crate::engine::flows::run::FlowState {
            flow: "code-review".into(),
            ..Default::default()
        }),
    );

    assert_eq!(
        (
            thread.flows.len(),
            thread.flow.as_ref().map(|file_value| file_value.flow.as_str())
        ),
        (2, Some("chore"))
    );

    thread.set_flow("ada", None);

    assert!(thread.flow.is_none() && thread.flow_of("ada").is_none() && thread.flow_of("zed").is_some());
}
