use super::*;

#[test]
fn defaults_name_the_harness_and_its_command() {
    assert_eq!(default_of("harness.codex.command").as_deref(), Some("codex app-server"));
    assert_eq!(default_of("harness.opencode.command").as_deref(), Some("opencode acp"));
    assert_eq!(default_of("harness.claude.model").as_deref(), Some("Claude Code's own"));
    assert_eq!(default_of("harness.claude.args").as_deref(), Some("none"));
    assert_eq!(default_of("notes"), None);
    assert_eq!(default_of("harness.nope.model"), None);
}

#[test]
fn harness_fields_round_trip_through_the_config_file() {
    let directory = std::env::temp_dir().join(format!("codehead-harnesstab-{}", std::process::id()));

    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("codehead.toml");

    std::fs::write(&path, "notes = '~/n'\n[aliases]\nd = '~/d/'\n").unwrap();
    let (base, mut file) = crate::infra::config::read_file(&path).unwrap();

    file.set("harness.codex.model", Some("gpt-5".into()));
    file.set("harness.claude.effort", Some("high".into()));
    let text = crate::infra::config::save_file(&path, &base, &file).unwrap();

    assert!(
        text.contains("[harness.codex]") && text.contains("model = \"gpt-5\""),
        "{text}"
    );

    // the other keys stay
    assert!(text.contains("notes = '~/n'") && text.contains("[aliases]"), "{text}");

    let (base, mut back) = crate::infra::config::read_file(&path).unwrap();

    assert_eq!(back.get("harness.claude.effort").as_deref(), Some("high"));

    back.set("harness.codex.model", None);
    back.set("harness.claude.effort", None);
    let text = crate::infra::config::save_file(&path, &base, &back).unwrap();

    assert!(!text.contains("harness"), "{text}");
    assert!(crate::infra::config::validate("harness.claude.effort", "huge").is_err());
    assert!(crate::infra::config::validate("harness.nope.model", "x").is_err());
    assert!(crate::infra::config::validate("harness.codex.color", "x").is_err());

    let _ = std::fs::remove_dir_all(directory);
}
