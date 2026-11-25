use super::*;

#[test]
fn steps_round_trip_through_front_matter() {
    let text_value = Step {
        name: "x".into(),
        description: "d".into(),
        gate: true,
        done: "it is done".into(),
        needs: vec!["skill:code-review".into(), "mcp:gh".into()],
        text: "line 1\n\nline 2".into(),
        ..Default::default()
    };

    assert_eq!(parse_step("x", &render_step(&text_value)), text_value);

    // 0 front matter: all body
    assert_eq!(parse_step("y", "just text").text, "just text");
}

/// Roles are gone: an old step / flow file with roles parses, its role gives a warning (the
/// step runs on the flow's agent), a save drops it.
#[test]
fn old_roles_parse_warn_and_go_on_save() {
    let text_value = parse_step("rev", "---\nrole: reviewer\ngate: true\n---\nreview it");

    assert_eq!(text_value.old_role.as_deref(), Some("reviewer"));
    assert!(
        !render_step(&text_value).contains("role"),
        "{}",
        render_step(&text_value)
    );

    let file = parse_flow("t", "name = \"t\"\nroles = [\"lead\", \"tester\"]\n[[step]]\ntext = \"a\"\nrole = \"tester\"\n[[step]]\ntext = \"b\"\nrole = \"lead\"\n").unwrap();
    let result = resolve(&file, &Library::default());

    assert_eq!(
        result[0].errors,
        vec!["role tester: roles are gone, the step runs on the flow's agent".to_string()]
    );
    assert!(result[1].errors.is_empty(), "role lead: 0 warning");

    let text = render_flow(&file).unwrap();

    assert!(!text.contains("role"), "{text}");
}

#[test]
fn every_builtin_parses_and_resolves_without_errors() {
    let lib = Library::load(None);

    assert_eq!(lib.flows.len(), BUILTIN_FLOWS.len(), "{:?}", lib.errors);
    assert!(lib.errors.is_empty(), "{:?}", lib.errors);

    for file in &lib.flows {
        for result in resolve(file, &lib) {
            assert!(
                result.errors.is_empty(),
                "{}: {}: {:?}",
                file.slug,
                result.id,
                result.errors
            );
            assert!(!result.text.is_empty(), "{}: {} has 0 text", file.slug, result.id);
        }
    }

    let nf = lib.flow("new feature").unwrap();
    let result_value = resolve(nf, &lib);
    let gate = result_value
        .iter()
        .position(|count| count.title == "plan approval")
        .unwrap();

    assert!(result_value[gate].gate);
    assert_eq!(
        result_value[gate].edges,
        vec![(
            "changes".to_string(),
            result_value
                .iter()
                .position(|count_value| count_value.id == "plan")
                .unwrap()
        )]
    );

    let ar = result_value
        .iter()
        .find(|count_2| count_2.id == "address-review")
        .unwrap();

    assert_eq!(
        ar.loops.as_ref().unwrap().0,
        result_value.iter().position(|count_3| count_3.id == "review").unwrap()
    );
}

#[test]
fn loops_must_go_up_and_targets_must_exist() {
    let file = parse_flow(
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
    let result = resolve(&file, &Library::default());

    assert!(result[0].loops.is_none() && !result[0].errors.is_empty());
    assert!(result[1].gate && result[1].edges.is_empty() && !result[1].errors.is_empty());
    assert_eq!(result[0].id, "step-1");
    assert_eq!(result[0].title, "a");
}

#[test]
fn user_layer_wins_and_rename_and_delete_keep_flows_whole() {
    let _g = crate::engine::ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _ = std::fs::remove_dir_all(user_dir("steps").parent().unwrap());
    let lib = Library::load(None);

    // edit of a built-in: a user file over it
    let mut text_value = lib.step("report").unwrap().clone();

    text_value.text = "my report".into();
    lib.save_step(&text_value).unwrap();
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
    let file = Flow {
        slug: "my-flow".into(),
        name: "my flow".into(),
        steps: vec![Node {
            uses: Some("mine".into()),
            ..Default::default()
        }],
        layer: Layer::User,
        ..Default::default()
    };

    lib.save_flow(&file).unwrap();
    let lib = Library::load(None);

    assert_eq!(lib.users_of("mine").len(), 1);

    lib.rename_step("mine", "ours").unwrap();
    let lib = Library::load(None);

    assert_eq!(lib.flow("my-flow").unwrap().steps[0].uses.as_deref(), Some("ours"));
    assert_eq!(lib.delete_step("ours").unwrap(), 1);

    let lib = Library::load(None);
    let count = &lib.flow("my-flow").unwrap().steps[0];

    assert_eq!((count.uses.as_deref(), count.text.as_deref()), (None, Some("do mine")));

    // promote it back
    let (text_2, n2) = promote(count, "again");

    assert_eq!((text_2.text.as_str(), n2.uses.as_deref()), ("do mine", Some("again")));

    let _ = std::fs::remove_dir_all(user_dir("steps").parent().unwrap());
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
