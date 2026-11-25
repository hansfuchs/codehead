use super::*;
use std::path::PathBuf;

fn tmp(name: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!("codehead-homeinit-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);

    std::fs::create_dir_all(&directory).unwrap();
    directory
}

#[test]
fn init_moves_files_links_back_and_splits_settings() {
    let root = tmp("init");
    let (home, config) = (root.join("home"), root.join("config"));

    std::fs::create_dir_all(home.join(".git")).unwrap();
    std::fs::create_dir_all(config.join("agents")).unwrap();
    std::fs::write(config.join("agents/REGI.toml"), "harness = \"claude\"\n").unwrap();
    std::fs::write(config.join("mcp.toml"), "[linear]\nurl = \"https://x\"\n").unwrap();
    let local = config.join("codehead.toml");
    let text = format!(
        "# mine\nagent = \"REGI\"\nnotes = \"{0}/notes/\"\nhome = \"{0}\"\neditor = \"zed\"\n\n[colors]\nscheme = \"amber\"\n\n[aliases]\n\"x:y\" = \"/tmp/y\"\n",
        home.display()
    );

    std::fs::write(&local, &text).unwrap();
    let before = config::read_layers(&local).unwrap().merged();

    let lines = init(&home, &local, &config).unwrap();
    let dir = home.join("codehead");

    assert!(dir.join("agents/REGI.toml").is_file(), "{lines:?}");
    assert!(dir.join("mcp.toml").is_file());
    assert!(
        std::fs::symlink_metadata(config.join("agents"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(config.join("agents/REGI.toml").is_file(), "the old path still reads");
    assert!(dir.join("README.md").is_file());
    assert!(
        std::fs::read_to_string(home.join(".gitattributes"))
            .unwrap()
            .contains(UNION)
    );

    let layers = config::read_layers(&local).unwrap();
    let (shared, device) = (layers.shared.clone().unwrap(), layers.device.clone().unwrap());

    assert_eq!(shared.file.agent.as_deref(), Some("REGI"));
    assert_eq!(shared.file.scheme.as_deref(), Some("amber"));
    assert_eq!(device.file.editor.as_deref(), Some("zed"));
    assert_eq!(device.file.aliases.get("x:y").map(String::as_str), Some("/tmp/y"));
    assert!(device.path.starts_with(dir.join("devices")));

    // codehead.toml: the pointer only
    let line = &layers.local.file;

    assert_eq!(
        (line.agent.as_ref(), line.editor.as_ref(), line.notes.as_ref()),
        (None, None, None)
    );
    assert!(line.home.is_some() && line.device.is_some());

    // the same values, notes = the default now
    let mut after = layers.merged();

    after.notes = before.notes.clone();
    after.device = None;

    assert_eq!(after, before);

    // again: 0 change
    let again = init(&home, &local, &config).unwrap();

    assert!(
        again
            .iter()
            .all(|line_value| !line_value.starts_with("moved") && !line_value.starts_with("settings")),
        "{again:?}"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn init_keeps_what_home_has_and_needs_git() {
    let root = tmp("keep");
    let (home, config) = (root.join("home"), root.join("config"));

    assert!(
        init(&home, &config.join("codehead.toml"), &config)
            .unwrap_err()
            .contains("not a git repo")
    );

    std::fs::create_dir_all(home.join(".git")).unwrap();
    std::fs::create_dir_all(home.join("codehead/agents")).unwrap();
    std::fs::write(home.join("codehead/agents/REGI.toml"), "harness = \"codex\"\n").unwrap();
    std::fs::create_dir_all(config.join("agents")).unwrap();
    std::fs::write(config.join("agents/REGI.toml"), "harness = \"claude\"\n").unwrap();
    std::fs::write(config.join("agents/ADA.toml"), "harness = \"claude\"\n").unwrap();
    std::fs::write(config.join("codehead.toml"), format!("home = \"{}\"\n", home.display())).unwrap();
    let lines = init(&home, &config.join("codehead.toml"), &config).unwrap();

    assert!(
        lines
            .iter()
            .any(|line| line.starts_with("kept") && line.contains("REGI")),
        "{lines:?}"
    );
    assert!(home.join("codehead/agents/ADA.toml").is_file());
    assert_eq!(
        std::fs::read_to_string(home.join("codehead/agents/REGI.toml")).unwrap(),
        "harness = \"codex\"\n"
    );

    // 1 file left: the old dir stays, 0 symlink
    assert!(config.join("agents/REGI.toml").is_file() && !config.join("agents/ADA.toml").exists());

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn status_names_the_missing_dir() {
    let root = tmp("status");
    let text = status(&root, "mbp");

    assert!(
        text.iter().any(|line| line.starts_with("codehead dir: missing")),
        "{text:?}"
    );

    let _ = std::fs::remove_dir_all(&root);
}
