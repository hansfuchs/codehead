use super::*;

fn ids(models: &[Model]) -> Vec<&str> {
    models.iter().map(|model| model.id.as_str()).collect()
}

#[test]
fn session_models_of_both_shapes() {
    let new = json!({"configOptions": [{"id": "mode", "options": []}, {"id": "model", "currentValue": "b", "options": [
        {"value": "a", "name": "A"}, {"group": "more", "name": "More", "options": [{"value": "b", "name": "B"}]}]}]});

    assert_eq!(
        session_models(&new),
        [
            Model {
                id: "a".into(),
                name: "A".into()
            },
            Model {
                id: "b".into(),
                name: "B".into()
            }
        ]
    );

    let old = json!({"models": {"currentModelId": "x", "availableModels": [{"modelId": "x", "name": "X"}, {"modelId": "y"}]}});

    assert_eq!(ids(&session_models(&old)), ["x", "y"]);
    assert_eq!(session_models(&json!({})), [] as [crate::infra::models::Model; 0]);
}

#[test]
fn codex_cache_lists_picker_models_by_priority() {
    let json = r#"{"models": [{"slug": "b", "display_name": "B", "visibility": "list", "priority": 2},
        {"slug": "hidden", "visibility": "hide", "priority": 0},
        {"slug": "a", "display_name": "A", "visibility": "list", "priority": 1}]}"#;

    assert_eq!(ids(&codex_cache(json)), ["a", "b"]);
    assert_eq!(codex_cache("not json"), [] as [crate::infra::models::Model; 0]);
}

#[test]
fn claude_aliases_with_exact_ids_then_ids() {
    let models = claude(&["opus".into(), "fable".into()]);

    assert_eq!(
        ids(&models),
        [
            "default",
            "opus",
            "fable",
            "claude-fable-5-2",
            "claude-opus-5-5",
            "claude-sonnet-5-5",
            "claude-haiku-5-5"
        ]
    );
    assert_eq!(models[1].name, "claude-opus-5-5");
    assert_eq!(ids(&lines("a/b\n\n  c/d \nnot a model\n")), ["a/b", "c/d"]);
}

#[test]
fn probe_of_a_fake_acp_agent_and_its_end() {
    // a shell ACP agent: answers initialize and session/new, then hangs; the probe ends it
    let script = r#"read a; echo '{"jsonrpc":"2.0","method":"note","params":{}}'; echo '{"jsonrpc":"2.0","id":1,"result":{}}'; read b; echo '{"jsonrpc":"2.0","id":2,"result":{"sessionId":"s","models":{"availableModels":[{"modelId":"m1","name":"M 1"}]}}}'; sleep 60"#;
    let at = Instant::now();
    let models = acp(&["sh".into(), "-c".into(), script.into()], Duration::from_secs(5)).unwrap();

    assert_eq!(
        models,
        [Model {
            id: "m1".into(),
            name: "M 1".into()
        }]
    );
    assert!(at.elapsed() < Duration::from_secs(3));

    // 0 answer: the timeout, the process still goes
    let at = Instant::now();
    let error = acp(
        &["sh".into(), "-c".into(), "sleep 60".into()],
        Duration::from_millis(300),
    )
    .unwrap_err();

    assert!(error.starts_with("initialize: 0 answer"), "{error}");
    assert!(at.elapsed() < Duration::from_secs(3));
    assert!(acp(&["/nonexistent/acp".into()], Duration::from_secs(1)).is_err());
    assert_eq!(
        run(
            &["sh".into(), "-c".into(), "echo a; echo b".into()],
            Duration::from_secs(5)
        )
        .unwrap(),
        "a\nb\n"
    );
}

#[test]
#[ignore = "spawns the installed harnesses"]
fn live_read() {
    for harness in harnesses::installed() {
        let at = Instant::now();

        let result = read(harness.name).map(|models| models.into_iter().map(|model| model.id).collect::<Vec<_>>());

        println!("{} ({} ms): {result:?}", harness.name, at.elapsed().as_millis());
    }
}

#[test]
fn cache_round_trip_and_age() {
    let entry = store(
        "probe-test",
        &[Model {
            id: "x".into(),
            name: String::new(),
        }],
    );

    assert!(fresh(&entry));
    assert_eq!(ids(&load()["probe-test"].models), ["x"]);
    assert!(!fresh(&Entry {
        at: now() - MAX_AGE.as_secs() - 1,
        models: Vec::new()
    }));
}
