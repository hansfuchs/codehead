use super::*;
use ratatui::crossterm::event::KeyModifiers;
use std::path::Path;

fn key(path: &mut Paths, ta: &mut TextArea, code: KeyCode) -> bool {
    path.key(KeyEvent::new(code, KeyModifiers::NONE), ta)
}

fn typed(ta: &mut TextArea, text: &str) {
    ta.set_text(text);
    ta.move_end();
}

#[test]
fn a_name_with_a_space_gets_quotes_and_the_dropdown_goes_on_inside() {
    let dir = std::env::temp_dir().join(format!("codehead-quoted-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    std::fs::create_dir_all(dir.join("My Notes/sub dir")).unwrap();
    std::fs::write(dir.join("My Notes/a b.md"), "x").unwrap();
    let mut path = Paths::new(&dir);
    let mut ta = TextArea::new(false, "");

    typed(&mut ta, "see ./M");

    assert!(key(&mut path, &mut ta, KeyCode::Tab));
    assert_eq!(ta.text(), "see \"./My Notes/");

    // inside the quote: the entries of the dir with the space
    assert_eq!(path.names(&ta), ["sub dir/", "a b.md"]);

    key(&mut path, &mut ta, KeyCode::Down);

    assert!(key(&mut path, &mut ta, KeyCode::Tab));
    assert_eq!(ta.text(), "see \"./My Notes/a b.md\" ");

    // dirs only: 0 files
    path.dirs_only = true;
    typed(&mut ta, "\"./My Notes/");

    assert_eq!(path.names(&ta), ["sub dir/"]);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn lists_the_directory_as_the_path_is_typed() {
    let dir = std::env::temp_dir().join(format!("codehead-paths-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    for directory in ["apps/web/src", "apps/api", "docs", ".hidden"] {
        std::fs::create_dir_all(dir.join(directory)).unwrap();
    }

    for file in ["apps/readme.md", "apps/web/index.ts", "apps/.env", "notes.txt"] {
        std::fs::write(dir.join(file), "x").unwrap();
    }

    let mut path = Paths::new(&dir);
    let mut ta = TextArea::new(false, "");

    // relative: `apps/` → its entries, dirs first; hidden ones only after a `.`
    typed(&mut ta, "look at apps/");

    assert_eq!(path.names(&ta), ["api/", "web/", "readme.md"]);

    typed(&mut ta, "look at apps/.");

    assert_eq!(path.names(&ta), [".env"]);

    // typed name: entries that start with it, then entries that hold it
    typed(&mut ta, "apps/e");

    assert_eq!(path.names(&ta), ["web/", "readme.md"]);

    // tab: the entry in; a dir keeps the dropdown open on its entries
    typed(&mut ta, "see apps/w");

    assert!(key(&mut path, &mut ta, KeyCode::Tab));
    assert_eq!(ta.text(), "see apps/web/");
    assert_eq!(path.names(&ta), ["src/", "index.ts"]);

    // enter: the input's key, until ↑↓ picked a row
    assert!(!key(&mut path, &mut ta, KeyCode::Enter));
    assert!(key(&mut path, &mut ta, KeyCode::Down));
    assert!(key(&mut path, &mut ta, KeyCode::Enter));
    assert_eq!(ta.text(), "see apps/web/index.ts ");
    assert_eq!(path.names(&ta), [] as [std::string::String; 0]);

    // a full file name: nothing to offer
    typed(&mut ta, "apps/web/index.ts");

    assert_eq!(path.names(&ta), [] as [std::string::String; 0]);

    assert_path_modes(&dir, &mut path, &mut ta);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn root_entries_only_by_prefix() {
    let mut path = Paths::new(&std::env::temp_dir());
    let mut ta = TextArea::new(false, "");

    // `/us` → /usr (a prefix); `/zz-no-such` (a slash command): 0 dropdown
    typed(&mut ta, "/us");

    assert!(
        path.names(&ta).iter().any(|count| count == "usr/"),
        "{:?}",
        path.names(&ta)
    );

    typed(&mut ta, "/general:antislop");

    assert_eq!(path.names(&ta), [] as [std::string::String; 0]);

    typed(&mut ta, "/usr/");

    assert!(path.names(&ta).iter().any(|count_value| count_value == "bin/"));
}

#[test]
fn configured_aliases_complete_paths_and_expand_sent_text() {
    // The child isolates CODEHEAD_CONFIG and the process-wide config cache.
    if let Some(root) = std::env::var_os("CODEHEAD_ALIAS_TEST_ROOT") {
        let root = PathBuf::from(root);
        let mut paths = Paths::new(&root.join("elsewhere"));
        let mut input = TextArea::new(false, "");

        typed(&mut input, "read project:platform/");

        assert_eq!(paths.names(&input), ["reports/", "notes.md"]);
        assert!(key(&mut paths, &mut input, KeyCode::Tab));
        assert_eq!(input.text(), "read project:platform/reports/");
        assert_eq!(paths.names(&input), ["review.md"]);
        assert!(key(&mut paths, &mut input, KeyCode::Tab));
        assert_eq!(input.text(), "read project:platform/reports/review.md ");
        assert_eq!(
            crate::infra::config::expand_aliases(&input.text()),
            format!("read {}/reports/review.md ", root.join("notes").display())
        );

        typed(&mut input, "project:platform/../");

        assert!(paths.names(&input).contains(&"elsewhere/".to_string()));

        return;
    }

    let root = std::env::temp_dir().join(format!("codehead-alias-path-{}", std::process::id()));

    std::fs::create_dir_all(root.join("notes/reports")).unwrap();
    std::fs::create_dir_all(root.join("elsewhere")).unwrap();
    std::fs::write(root.join("notes/notes.md"), "notes").unwrap();
    std::fs::write(root.join("notes/reports/review.md"), "review").unwrap();
    let file = crate::infra::config::File {
        aliases: [("project:platform".into(), root.join("notes").display().to_string())].into(),
        ..Default::default()
    };

    crate::infra::config::save_file(&root.join("codehead.toml"), "", &file).unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "paths::tests::configured_aliases_complete_paths_and_expand_sent_text",
            "--nocapture",
        ])
        .env("CODEHEAD_ALIAS_TEST_ROOT", &root)
        .env("CODEHEAD_CONFIG", root.join("codehead.toml"))
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let _ = std::fs::remove_dir_all(root);
}

fn assert_path_modes(dir: &Path, path: &mut Paths, ta: &mut TextArea) {
    // absolute and ./ paths
    typed(ta, &format!("{}/apps/", dir.display()));

    assert_eq!(path.names(ta), ["api/", "web/", "readme.md"]);

    typed(ta, "./do");

    assert_eq!(path.names(ta), ["docs/"]);

    // 0 such dir, a url, a lone slash, an @ word: 0 dropdown, keys pass
    for text in ["and/or", "https://example.com/a", "a / b /", "@apps/", "x >apps/"] {
        typed(ta, text);

        assert!(path.names(ta).is_empty(), "{text}");
        assert!(!key(path, ta, KeyCode::Tab), "{text}");
    }

    // esc: closed for this word; a new word opens again
    typed(ta, "apps/");

    assert!(key(path, ta, KeyCode::Esc));
    assert_eq!(path.names(ta), [] as [std::string::String; 0]);

    ta.insert("w");

    assert_eq!(path.names(ta), [] as [std::string::String; 0]);

    ta.insert(" docs/../apps/");

    assert_eq!(path.names(ta).len(), 3);

    // drawn above the prompt box
    let backend = ratatui::backend::TestBackend::new(60, 20);
    let mut term = ratatui::Terminal::new(backend).unwrap();
    let input = Rect::new(0, 14, 60, 4);

    term.draw(|file_value| {
        ta.render(file_value, input, true);
        path.render(file_value, ta, file_value.area(), input);
    })
    .unwrap();

    assert!(
        path.area.height == 5 && path.area.bottom() <= input.y,
        "{:?}",
        path.area
    );
}
