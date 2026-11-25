use super::*;

#[test]
fn previews_takes_auto_a_protocol_or_off() {
    for ok in ["auto", "kitty", "iterm2", "sixel", "off"] {
        assert!(validate("previews", ok).is_ok(), "{ok}");
    }

    assert!(
        validate("previews", "on")
            .unwrap_err()
            .contains("auto, kitty, iterm2, sixel or off")
    );
    assert_eq!(parse("previews = \"off\"\n").unwrap().previews.as_deref(), Some("off"));
    assert!(parse("previews = \"yes\"\n").is_err());
}

#[test]
fn keys_and_colors_tables_round_trip() {
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let file =
        parse("[keys]\nmenu = \"ctrl+p\"\ncommand = \"%\"\n[colors]\nscheme = \"amber\"\naccent = \"#112233\"\n")
            .unwrap();

    assert_eq!(
        (
            file.menu_key.as_deref(),
            file.command_key.as_deref(),
            file.scheme.as_deref()
        ),
        (Some("ctrl+p"), Some("%"), Some("amber"))
    );
    assert_eq!(file.colors["accent"], "#112233");

    let palette = palette_of(&file);

    assert_eq!(palette.accent, ratatui::style::Color::Rgb(0x11, 0x22, 0x33));
    assert_eq!(palette.text, crate::domain::palette::AMBER.text);

    // written back: same tables; unset keys go
    let mut edited = file.clone();

    edited.set("colors.accent", None);
    edited.set("keys.command", None);
    let text = document(
        "[keys]\nmenu = \"ctrl+p\"\ncommand = \"%\"\n[colors]\nscheme = \"amber\"\naccent = \"#112233\"\n",
        &edited,
    )
    .unwrap();
    let back = parse(&text).unwrap();

    assert_eq!(back, edited);

    for bad in [
        "[keys]\nmenu = \"x\"",
        "[keys]\nmenu = \"ctrl+n\"",
        "[keys]\nmenu = \"F2\"",
        "[keys]\ncommand = \"#\"",
        "[keys]\ncommand = \"ab\"",
        "[colors]\nscheme = \"pink\"",
        "[colors]\naccent = \"red\"",
        "[colors]\nfoo = \"#000000\"",
    ] {
        assert!(parse(bad).is_err(), "{bad}");
    }

    let ctrl_p = Bind::parse("ctrl+p").unwrap();

    assert!(ctrl_p.matches(&KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL)));
    assert!(!ctrl_p.matches(&KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE)));

    // cmd+p (super) = ctrl+p
    assert!(ctrl_p.matches(&KeyEvent::new(KeyCode::Char('p'), KeyModifiers::SUPER)));
    assert!(
        Bind::parse("F2")
            .unwrap()
            .matches(&KeyEvent::new(KeyCode::F(2), KeyModifiers::NONE))
    );
}

#[test]
fn parses_keys() {
    let file = parse("notes = \"~/n\"\nagent = \"amon\"").unwrap();

    assert_eq!(file.notes.as_deref(), Some("~/n"));
    assert_eq!(file.agent.as_deref(), Some("amon"));
}

#[test]
fn rejects_unknown_and_non_string() {
    assert!(parse("note = \"x\"").is_err());
    assert!(parse("notes = 1").is_err());
}

#[test]
fn expands_home() {
    let home = home();

    assert_eq!(expand("~/a"), home.join("a"));
    assert_eq!(expand("~"), home);
    assert_eq!(expand("/abs"), PathBuf::from("/abs"));
    assert_eq!(expand("rel"), home.join("rel"));
}

#[test]
fn aliases_parse_resolve_and_expand_at_token_boundaries() {
    let file = parse("[aliases]\n\"project:platform\" = \"~/notes/platform/\"\nshort = \"/tmp/shared\"\n").unwrap();

    assert_eq!(
        alias_path(&file.aliases, "project:platform/reports/a.md"),
        Some(home().join("notes/platform/reports/a.md"))
    );
    assert_eq!(alias_path(&file.aliases, "short/"), Some(PathBuf::from("/tmp/shared/")));
    assert_eq!(alias_path(&file.aliases, "unknown/a"), None);
    assert_eq!(alias_path(&file.aliases, "shorter/a"), None);

    let text = "read `project:platform/a.md`, (short/b.md) https://example.com/short/a";

    assert_eq!(
        expand_with(&file.aliases, text),
        format!(
            "read `{}/a.md`, (/tmp/shared/b.md) https://example.com/short/a",
            home().join("notes/platform").display()
        )
    );

    for invalid in [
        "aliases = 'x'",
        "[aliases]\nwrong = 2",
        "[aliases]\n'bad/name' = '/tmp'",
        "[aliases]\nempty = ''",
    ] {
        assert!(parse(invalid).is_err(), "{invalid}");
    }
}

#[test]
fn device_names_are_slugs() {
    assert_eq!(slug("Hanss-MacBook-Pro.local"), "hanss-macbook-pro");
    assert_eq!(slug(" My Mini_2 "), "my-mini-2");
    assert_eq!(slug("..."), "device");
    assert_ne!(host_device(), "");
}

#[test]
fn scopes_route_paths_to_the_device_and_the_rest_to_shared() {
    assert_eq!(scope("home"), Scope::Local);
    assert_eq!(scope("editor"), Scope::Device);
    assert_eq!(scope("harness.codex.command"), Scope::Device);
    assert_eq!(scope("harness.claude.model"), Scope::Shared);
    assert_eq!(scope("colors.accent"), Scope::Shared);

    let mut file = parse("agent = 'REGI'\neditor = 'zed'\nhome = '/h'\n[harness.codex]\ncommand = 'x'\nmodel = 'm'\n[aliases]\na = '/tmp'\n").unwrap();

    file.scheme = Some("amber".into());
    let [line, text, directory] = split(&file);

    assert_eq!((line.home.as_deref(), line.agent.as_deref()), (Some("/h"), None));
    assert_eq!(
        (
            text.agent.as_deref(),
            text.scheme.as_deref(),
            text.get("harness.codex.model").as_deref()
        ),
        (Some("REGI"), Some("amber"), Some("m"))
    );
    assert_eq!(
        (
            directory.editor.as_deref(),
            directory.get("harness.codex.command").as_deref(),
            directory.aliases.len()
        ),
        (Some("zed"), Some("x"), 1)
    );

    // split, then merged: the same values
    let mut back = line.clone();

    overlay(&mut back, &text);
    overlay(&mut back, &directory);

    assert_eq!(back, file);
}

#[test]
fn layers_merge_device_over_shared_and_save_each_key_to_its_file() {
    let root = std::env::temp_dir().join(format!("codehead-layers-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let home = root.join("home");

    std::fs::create_dir_all(home.join("codehead/devices")).unwrap();
    let local = root.join("codehead.toml");

    std::fs::write(
        &local,
        format!("home = '{}'\ndevice = 'mbp'\nprefix = 'old'\n", home.display()),
    )
    .unwrap();
    std::fs::write(
        home.join("codehead/settings.toml"),
        "prefix = 'task'\neditor = 'code'\n",
    )
    .unwrap();
    std::fs::write(home.join("codehead/devices/mbp.toml"), "# this mac\neditor = 'zed'\n").unwrap();
    let layers = read_layers(&local).unwrap();
    let merged = layers.merged();

    assert_eq!(
        (
            merged.prefix.as_deref(),
            merged.editor.as_deref(),
            merged.device.as_deref()
        ),
        (Some("task"), Some("zed"), Some("mbp"))
    );

    let mut edited = merged.clone();

    edited.editor = Some("cursor".into());
    edited.messages = Some("lines".into());
    let after = save_layers(&layers, &edited).unwrap();

    assert_eq!(after.merged(), edited);

    let dev = std::fs::read_to_string(home.join("codehead/devices/mbp.toml")).unwrap();

    assert!(dev.contains("# this mac") && dev.contains("cursor"), "{dev}");

    let shared = std::fs::read_to_string(home.join("codehead/settings.toml")).unwrap();

    assert!(shared.contains("lines") && !shared.contains("editor"), "{shared}");

    // codehead.toml: the pointer only (its old prefix: now in shared)
    assert_eq!(after.local.file.prefix, None);

    // a new device: only the pointer changes, the new device's file starts empty
    let mut moved_file = after.merged();

    moved_file.device = Some("mini".into());
    moved_file.editor = Some("idea".into());
    let moved = save_layers(&after, &moved_file).unwrap();

    assert_eq!(moved.local.file.device.as_deref(), Some("mini"));

    let merged = moved.merged();

    // editor: a device key (the save took it out of shared), mini: 0 file yet
    assert_eq!(
        (merged.editor.as_deref(), merged.messages.as_deref()),
        (None, Some("lines"))
    );
    assert!(!home.join("codehead/devices/mini.toml").exists());

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn layers_without_the_codehead_dir_are_codehead_toml_alone() {
    let root = std::env::temp_dir().join(format!("codehead-layers-0-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);

    std::fs::create_dir_all(&root).unwrap();
    let local = root.join("codehead.toml");

    std::fs::write(&local, format!("home = '{}'\nprefix = 'x'\n", root.display())).unwrap();
    let layers = read_layers(&local).unwrap();

    assert!(layers.shared.is_none() && layers.device.is_none());

    let mut file = layers.merged();

    file.editor = Some("zed".into());
    save_layers(&layers, &file).unwrap();

    assert!(std::fs::read_to_string(&local).unwrap().contains("zed"));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn saves_preserve_comments_and_reject_external_changes() {
    let dir = std::env::temp_dir().join(format!("codehead-config-{}", std::process::id()));

    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("codehead.toml");
    let base = "# user settings\nnotes = '~/old' # notes comment\nprefix = 'task'\n\n[aliases]\n# shared notes\n\"project:platform\" = '~/old/' # alias comment\n";

    std::fs::write(&path, base).unwrap();
    let mut file = parse(base).unwrap();

    file.notes = Some("~/new".into());
    file.aliases.insert("project:platform".into(), "~/new/".into());
    let saved = save_file(&path, base, &file).unwrap();

    for comment in [
        "# user settings",
        "# notes comment",
        "# shared notes",
        "# alias comment",
    ] {
        assert!(saved.contains(comment), "{saved}");
    }

    assert_eq!(parse(&saved).unwrap(), file);

    std::fs::write(&path, "prefix = 'external'\n").unwrap();

    assert!(
        save_file(&path, &saved, &file)
            .unwrap_err()
            .contains("changed outside codehead")
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "prefix = 'external'\n");

    let _ = std::fs::remove_dir_all(&dir);
}
