use super::*;

#[test]
fn steps_round_trip_through_front_matter() {
    let step = Step {
        name: "x".into(),
        description: "d".into(),
        gate: true,
        done: "it is done".into(),
        needs: vec!["skill:code-review".into(), "mcp:gh".into()],
        text: "line 1\n\nline 2".into(),
        ..Default::default()
    };

    assert_eq!(parse_step("x", &render_step(&step)), step);

    // 0 front matter: all body
    assert_eq!(parse_step("y", "just text").text, "just text");
}

/// Roles are gone: an old step / flow file with roles parses, its role gives a warning (the
/// step runs on the flow's agent), a save drops it.
#[test]
fn old_roles_parse_warn_and_go_on_save() {
    let step = parse_step("rev", "---\nrole: reviewer\ngate: true\n---\nreview it");

    assert_eq!(step.old_role.as_deref(), Some("reviewer"));
    assert!(!render_step(&step).contains("role"), "{}", render_step(&step));

    let flow = parse_flow("t", "name = \"t\"\nroles = [\"lead\", \"tester\"]\n[[step]]\ntext = \"a\"\nrole = \"tester\"\n[[step]]\ntext = \"b\"\nrole = \"lead\"\n").unwrap();
    let nodes = resolve(&flow, &Library::default());

    assert_eq!(
        nodes[0].errors,
        vec!["role tester: roles are gone, the step runs on the flow's agent".to_string()]
    );
    assert!(nodes[1].errors.is_empty(), "role lead: 0 warning");

    let text = render_flow(&flow).unwrap();

    assert!(!text.contains("role"), "{text}");
}

#[test]
fn every_builtin_parses_and_resolves_without_errors() {
    let lib = Library::load(None);

    assert_eq!(lib.flows.len(), BUILTIN_FLOWS.len(), "{:?}", lib.errors);
    assert!(lib.errors.is_empty(), "{:?}", lib.errors);

    for flow in &lib.flows {
        for node in resolve(flow, &lib) {
            assert!(node.errors.is_empty(), "{}: {}: {:?}", flow.slug, node.id, node.errors);
            assert!(!node.text.is_empty(), "{}: {} has 0 text", flow.slug, node.id);
        }
    }

    let new_feature = lib.flow("new feature").unwrap();
    let nodes = resolve(new_feature, &lib);
    let gate = nodes.iter().position(|node| node.title == "plan approval").unwrap();

    assert!(nodes[gate].gate);
    assert_eq!(
        nodes[gate].edges,
        vec![(
            "changes".to_string(),
            nodes.iter().position(|node| node.id == "plan").unwrap()
        )]
    );

    let address_review = nodes.iter().find(|node| node.id == "address-review").unwrap();

    assert_eq!(
        address_review.loops.as_ref().unwrap().0,
        nodes.iter().position(|node| node.id == "review").unwrap()
    );
}

#[test]
fn loops_must_go_up_and_targets_must_exist() {
    let flow = parse_flow(
        "t",
        r#"name = "t"
[[step]]
text = "a"
loop = { to = "b" }
[[step]]
id = "b"
text = "b"
gate = { back = "nowhere" }
"#,
    )
    .unwrap();
    let nodes = resolve(&flow, &Library::default());

    assert!(nodes[0].loops.is_none() && !nodes[0].errors.is_empty());
    assert!(nodes[1].gate && nodes[1].edges.is_empty() && !nodes[1].errors.is_empty());
    assert_eq!(nodes[0].id, "step-1");
    assert_eq!(nodes[0].title, "a");
}

#[test]
fn user_layer_wins_and_rename_and_delete_keep_flows_whole() {
    let env_lock = crate::engine::ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _ = std::fs::remove_dir_all(user_dir("steps").parent().unwrap());
    let lib = Library::load(None);

    // edit of a built-in: a user file over it
    let mut step = lib.step("report").unwrap().clone();

    step.text = "my report".into();
    lib.save_step(&step).unwrap();
    let lib = Library::load(None);

    assert_eq!(lib.step("report").unwrap().text, "my report");
    assert!(lib.step("report").unwrap().edited);

    lib.remove_step_file("report", Layer::User).unwrap();

    // a user step in a user flow: rename, then delete → one-off node with its text
    let lib = Library::load(None);

    lib.save_step(&Step {
        name: "mine".into(),
        text: "do mine".into(),
        layer: Layer::User,
        ..Default::default()
    })
    .unwrap();
    let flow = Flow {
        slug: "my-flow".into(),
        name: "my flow".into(),
        steps: vec![Node {
            uses: Some("mine".into()),
            ..Default::default()
        }],
        layer: Layer::User,
        ..Default::default()
    };

    lib.save_flow(&flow).unwrap();
    let lib = Library::load(None);

    assert_eq!(lib.users_of("mine").len(), 1);

    lib.rename_step("mine", "ours").unwrap();
    let lib = Library::load(None);

    assert_eq!(lib.flow("my-flow").unwrap().steps[0].uses.as_deref(), Some("ours"));
    assert_eq!(lib.delete_step("ours").unwrap(), 1);

    let lib = Library::load(None);
    let node = &lib.flow("my-flow").unwrap().steps[0];

    assert_eq!((node.uses.as_deref(), node.text.as_deref()), (None, Some("do mine")));

    // promote it back
    let (promoted_step, promoted_node) = promote(node, "again");

    assert_eq!(
        (promoted_step.text.as_str(), promoted_node.uses.as_deref()),
        ("do mine", Some("again"))
    );

    let _ = std::fs::remove_dir_all(user_dir("steps").parent().unwrap());

    drop(env_lock);
}

#[test]
fn slugs_and_titles() {
    assert_eq!(slug("Migration / Upgrade"), "migration-upgrade");
    assert_eq!(
        node_title(&Node {
            text: Some("\n  update CHANGELOG.md\nmore".into()),
            ..Default::default()
        }),
        "update CHANGELOG.md"
    );
}
