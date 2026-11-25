use super::*;

#[test]
pub(crate) fn a_flow_goes_with_the_task_moves_on_reports_and_waits_at_gates() {
    let env_lock = crate::engine::ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let thread = store::Thread::new("flow test", "task", Path::new("/tmp"), vec!["ada".into()]);

    std::fs::create_dir_all(store::thread_dir(&thread.id)).unwrap();
    thread.save().unwrap();
    let (htx, harness_rx) = mpsc::channel();
    let mut host = test_host(thread.id.clone(), htx);
    let flow = || store::load(&thread.id).unwrap().flow_of("ada").unwrap();

    // set before the 1st message: the step waits for the task
    host.cmd(&json!({"op": "flow_set", "flow": "chore"})).unwrap();

    assert!(host.later.is_empty() && !flow().sent);

    host.cmd(&json!({"op": "prompt", "text": "do it"})).unwrap();
    let (to, message) = host.later.pop().unwrap();

    assert_eq!(to, "ada");
    assert!(
        message.starts_with("[codehead flow: chore · step 1/3: implement]") && message.ends_with("do it"),
        "{message}"
    );
    assert!(flow().sent);

    // only the flow's agent reports it (bob has 0 flow); the reply holds the next step
    assert!(host.cmd(&json!({"op": "flow_step", "agent": "bob"})).is_err());

    let step_reply = host.cmd(&json!({"op": "flow_step", "agent": "ada"})).unwrap();

    assert!(step_reply["text"].as_str().unwrap().contains("step 2/3: verify"));

    // a switch after the flow: the queued flow starts when it ends
    host.cmd(&json!({"op": "flow_set", "flow": "new feature", "mode": "after"}))
        .unwrap();

    assert_eq!(flow().queued.unwrap().flow, "new-feature");

    host.cmd(&json!({"op": "flow_step", "agent": "ada"})).unwrap();
    let next_flow = host.cmd(&json!({"op": "flow_step", "agent": "ada"})).unwrap();

    assert!(
        next_flow["text"]
            .as_str()
            .unwrap()
            .contains("[codehead flow: new feature · step 1/9: scrutinize-task]"),
        "{next_flow}"
    );
    assert_eq!(flow().flow, "new-feature");

    // a gate: the agent waits, the owner's outcome picks the edge
    host.cmd(&json!({"op": "flow_jump", "node": 3})).unwrap();
    let (_, gate_step) = host.later.pop().unwrap();

    assert!(gate_step.contains("plan approval") && gate_step.contains("Gate:"));

    let gate_reply = host.cmd(&json!({"op": "flow_step", "agent": "ada"})).unwrap();

    assert_eq!(gate_reply["wait"], "owner");
    assert!(
        host.cmd(&json!({"op": "flow_step", "agent": "ada", "pass_gate": true}))
            .is_err(),
        "ada is 0 autopilot agent of the roster"
    );

    verify_a_flow_goes_with_the_task_moves_on_reports_and_waits_at_gates(&thread, host);

    drop(harness_rx);
    drop(env_lock);
}

fn verify_a_flow_goes_with_the_task_moves_on_reports_and_waits_at_gates(thread: &store::Thread, mut host: Host) {
    host.cmd(&json!({"op": "flow_answer", "choice": "changes", "note": "split it"}))
        .unwrap();
    let (_, revised) = host.later.pop().unwrap();

    assert!(
        revised.starts_with("[the owner] split it") && revised.contains("step 3/9: plan"),
        "{revised}"
    );

    // peer review: the flow's own agent asks another agent (roles are gone)
    host.cmd(&json!({"op": "flow_jump", "node": 6})).unwrap();
    let (to, review_step) = host.later.pop().unwrap();

    assert!(
        to == "ada" && review_step.contains("peer-review") && review_step.contains("list_agents"),
        "{review_step}"
    );

    let log: Vec<String> = store::events(&thread.id)
        .into_iter()
        .filter_map(|event| match event.kind {
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
        log.iter().any(|line| line == "▶ chore · step 1/3 implement · @ada"),
        "steps name their flow: {log:?}"
    );

    let _ = std::fs::remove_dir_all(store::thread_dir(&thread.id));
}

/// 2 agents, 2 flows: each step goes to its flow's agent (with the task at the start), each
/// `flow_step` moves only its own flow, owner ops name the agent once 2+ flows run.
#[test]
pub(crate) fn each_agent_runs_its_own_flow() {
    let env_lock = crate::engine::ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let thread = store::Thread::new("2 flows", "task", Path::new("/tmp"), vec!["ada".into(), "zed".into()]);

    std::fs::create_dir_all(store::thread_dir(&thread.id)).unwrap();
    thread.save().unwrap();
    let (htx, harness_rx) = mpsc::channel();
    let mut host = test_host(thread.id.clone(), htx);
    let flow = |agent: &str| store::load(&thread.id).unwrap().flow_of(agent);

    host.cmd(&json!({"op": "flow_set", "flow": "chore", "agent": "ada"}))
        .unwrap();
    host.cmd(&json!({"op": "flow_set", "flow": "code review", "agent": "zed"}))
        .unwrap();

    assert!(
        host.cmd(&json!({"op": "flow_set", "flow": "chore", "agent": "bob"}))
            .is_err(),
        "bob is not on the thread"
    );

    // the task goes to the lead; each agent gets its step 1 with the task under it
    host.cmd(&json!({"op": "prompt", "text": "do it"})).unwrap();
    let ada = host
        .later
        .iter()
        .find(|(agent, _)| agent == "ada")
        .map(|(_, message)| message.clone())
        .unwrap();
    let zed = host
        .later
        .iter()
        .find(|(agent, _)| agent == "zed")
        .map(|(_, message)| message.clone())
        .unwrap();

    assert!(
        ada.starts_with("[codehead flow: chore · step 1/3: implement]") && ada.ends_with("do it"),
        "{ada}"
    );
    assert!(
        zed.starts_with("[codehead flow: code review · step 1/2: review-diff]") && zed.ends_with("do it"),
        "{zed}"
    );
    assert_eq!(host.later.len(), 2);

    // zed reports its step: only its flow moves
    let result = host.cmd(&json!({"op": "flow_step", "agent": "zed"})).unwrap();

    assert!(
        result["text"].as_str().unwrap().contains("step 2/2: report"),
        "{result}"
    );
    assert_eq!((flow("ada").unwrap().node, flow("zed").unwrap().node), (0, 1));

    // owner ops: 2 flows → the agent is named
    let error = host.cmd(&json!({"op": "flow_skip"})).unwrap_err();

    assert!(error.contains("@ada") && error.contains("@zed"), "{error}");

    host.cmd(&json!({"op": "flow_skip", "agent": "ada"})).unwrap();

    assert_eq!(flow("ada").unwrap().node, 1);

    // the flow lines are the agent's events
    verify_each_agent_runs_its_own_flow(&thread, host, flow);

    drop(harness_rx);
    drop(env_lock);
}

fn verify_each_agent_runs_its_own_flow(
    thread: &store::Thread,
    mut host: Host,
    flow: impl Fn(&str) -> Option<crate::engine::flows::run::FlowState>,
) {
    let flows: Vec<(Option<String>, String)> = store::events(&thread.id)
        .into_iter()
        .filter_map(|event| match event.kind {
            Kind::Flow { text, .. } => Some((event.agent, text)),
            _ => None,
        })
        .collect();

    assert!(
        flows
            .iter()
            .any(|(agent, line)| agent.as_deref() == Some("zed") && line.starts_with("▶ code review · step 1/2")),
        "{flows:?}"
    );

    // zed's flow goes, ada's stays
    host.cmd(&json!({"op": "flow_clear", "agent": "zed"})).unwrap();

    assert!(flow("zed").is_none() && flow("ada").is_some());

    // bob replaces ada: ada's flow goes to bob, its step again with bob's 1st message
    host.later.clear();
    host.cmd(&json!({"op": "leave", "agent": "ada", "to": "bob"})).unwrap();

    assert!(flow("ada").is_none() && flow("bob").is_some_and(|state| !state.sent && state.node == 1));

    host.cmd(&json!({"op": "prompt", "to": ["bob"], "text": "take over"}))
        .unwrap();
    let bob = host
        .later
        .iter()
        .find(|(agent, _)| agent == "bob")
        .map(|(_, message)| message.clone())
        .unwrap();

    assert!(
        bob.starts_with("[codehead flow: chore · step 2/3") && bob.ends_with("take over"),
        "{bob}"
    );

    // zed leaves (0 replacement): 0 flow left
    host.cmd(&json!({"op": "leave", "agent": "zed"})).unwrap();

    assert!(flow("zed").is_none());

    let _ = std::fs::remove_dir_all(store::thread_dir(&thread.id));
}

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
    assert_eq!(thread.flow_of("nova").map(|state| state.flow), Some("chore".into()));

    thread.rename_agent("zed", "rex");

    assert_eq!(
        thread.flow_of("rex").map(|state| state.flow),
        Some("code-review".into())
    );
    assert!(thread.flow_of("zed").is_none() && thread.flow_of("ada").is_none());
}

/// An older version's thread: its 1 flow is the lead's; a save keeps `flow` = the lead's.
#[test]
pub(crate) fn an_old_thread_flow_belongs_to_the_lead() {
    let mut thread = store::Thread::new("old", "task", Path::new("/tmp"), vec!["ada".into(), "zed".into()]);

    thread.flow = Some(crate::engine::flows::run::FlowState {
        flow: "chore".into(),
        node: 2,
        ..Default::default()
    });

    assert_eq!(thread.flow_of("ada").map(|state| state.node), Some(2));
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
            thread.flow.as_ref().map(|state| state.flow.as_str())
        ),
        (2, Some("chore"))
    );

    thread.set_flow("ada", None);

    assert!(thread.flow.is_none() && thread.flow_of("ada").is_none() && thread.flow_of("zed").is_some());
}
