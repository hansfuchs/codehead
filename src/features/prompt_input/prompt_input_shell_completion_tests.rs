use super::*;

fn typed(ta: &mut TextArea, text: &str) {
    ta.set_text(text);
    ta.move_end();
}

fn key(completion: &mut ShellComp, ta: &mut TextArea, dir: &Path, code: KeyCode) -> bool {
    completion.key(KeyEvent::new(code, KeyModifiers::NONE), ta, dir, &[])
}

/// A project with a justfile, a package.json (pnpm) and a Makefile; 0 fish (deterministic).
fn project(name: &str) -> (PathBuf, ShellComp) {
    let dir = std::env::temp_dir().join(format!("codehead-shellcomp-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    std::fs::create_dir_all(dir.join(".git")).unwrap();
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    std::fs::write(dir.join("justfile"), "set shell := [\"bash\", \"-c\"]\nversion := \"1\"\nalias b := build\n\n# build the app\nbuild target='x': deps\n    cargo build\n\n[private]\nhidden:\n    true\n\n_internal:\n    true\n\n[doc(\"run the tests\")]\n@test *args:\n    cargo test\n").unwrap();
    std::fs::write(
        dir.join("package.json"),
        r#"{"name":"x","scripts":{"dev":"vite","build:web":"vite build","lint":"eslint ."}}"#,
    )
    .unwrap();
    std::fs::write(dir.join("pnpm-lock.yaml"), "").unwrap();
    std::fs::write(dir.join("Makefile"), ".PHONY: all clean\nCC := cc\n# make it all\nall: main.o\n\tcc\nclean: ## remove the build\n\trm -rf out\n%.o: %.c\n\tcc\n").unwrap();
    let completion = ShellComp {
        fish: false,
        catalog: crate::features::prompt_input::shell_completion::CommandCatalog {
            commands: Some(Arc::new(vec!["bunx".into(), "busybox".into(), "cargo".into()])),
            history: Some(Arc::new(vec!["cargo test --release".into(), "cargo build".into()])),
            ..Default::default()
        },

        ..Default::default()
    };

    (dir, completion)
}

#[test]
fn project_files_parse() {
    let (dir, _) = project("parse");
    let found = tasks(&dir.join("sub"));
    let runs: Vec<(&str, &str)> = found
        .iter()
        .map(|task| (task.run.as_str(), task.doc.as_str()))
        .collect();

    assert_eq!(
        runs,
        [
            ("just build", "build the app"),
            ("just test", "run the tests"),
            ("pnpm dev", "vite"),
            ("pnpm build:web", "vite build"),
            ("pnpm lint", "eslint ."),
            ("make all", "make it all"),
            ("make clean", "remove the build")
        ]
    );

    std::fs::remove_file(dir.join("pnpm-lock.yaml")).unwrap();

    assert_eq!(
        tasks(&dir)
            .iter()
            .find(|task| task.name == "dev")
            .map(|task| task.run.as_str()),
        Some("npm run dev")
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_first_word_offers_tasks_then_commands_and_tab_takes_one() {
    let (dir, mut completion) = project("first");
    let mut ta = TextArea::new(false, "");

    // `! `: 0 dropdown (↑ stays the history's); tab opens the tasks
    typed(&mut ta, "! ");

    assert_eq!(super::texts(&ta, &dir, &mut completion), [] as [std::string::String; 0]);
    assert!(key(&mut completion, &mut ta, &dir, KeyCode::Tab));
    assert_eq!(super::texts(&ta, &dir, &mut completion).len(), 7);
    assert_eq!(ta.text(), "! just build", "the 1st row in the text");

    // a word: tasks by name, then the commands on PATH
    typed(&mut ta, "! bu");

    assert_eq!(
        super::texts(&ta, &dir, &mut completion),
        ["just build ", "pnpm build:web ", "bunx ", "busybox "]
    );

    // passive: enter, ↑↓ stay the input's; tab: active on the 1st row, tab / ↓ the next
    assert!(!key(&mut completion, &mut ta, &dir, KeyCode::Enter));
    assert!(!key(&mut completion, &mut ta, &dir, KeyCode::Down));
    assert!(key(&mut completion, &mut ta, &dir, KeyCode::Tab));
    assert_eq!(ta.text(), "! just build");
    assert!(key(&mut completion, &mut ta, &dir, KeyCode::Tab));
    assert_eq!(ta.text(), "! pnpm build:web");
    assert_eq!(
        super::texts(&ta, &dir, &mut completion).len(),
        4,
        "the typed word stays the filter"
    );
    assert!(key(&mut completion, &mut ta, &dir, KeyCode::Enter));
    assert_eq!(ta.text(), "! pnpm build:web ");

    // esc: closed for this word
    typed(&mut ta, "! bu");

    assert!(key(&mut completion, &mut ta, &dir, KeyCode::Esc));
    assert_eq!(super::texts(&ta, &dir, &mut completion), [] as [std::string::String; 0]);
    assert!(!key(&mut completion, &mut ta, &dir, KeyCode::Down));

    // a path word: the path dropdown's; out of shell mode: 0 dropdown
    for input in ["! ./bu", "bu", "! cargo \"bu"] {
        typed(&mut ta, input);

        assert!(super::texts(&ta, &dir, &mut completion).is_empty(), "{input}");
    }

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_runner_offers_its_tasks_at_once() {
    let (dir, mut completion) = project("runner");
    let mut ta = TextArea::new(false, "");

    typed(&mut ta, "! just ");

    assert_eq!(super::texts(&ta, &dir, &mut completion), ["build ", "test "]);
    assert!(key(&mut completion, &mut ta, &dir, KeyCode::Tab));
    assert!(key(&mut completion, &mut ta, &dir, KeyCode::Enter));
    assert_eq!(ta.text(), "! just build ");

    typed(&mut ta, "! pnpm run b");

    assert_eq!(super::texts(&ta, &dir, &mut completion), ["build:web "]);

    // 1 row: tab takes it
    typed(&mut ta, "! make c");

    assert_eq!(super::texts(&ta, &dir, &mut completion), ["clean "]);
    assert!(key(&mut completion, &mut ta, &dir, KeyCode::Tab));
    assert_eq!(ta.text(), "! make clean ");

    // after `&&`: a new command (its 1st word)
    typed(&mut ta, "! make clean && just t");

    assert_eq!(super::texts(&ta, &dir, &mut completion), ["test "]);

    typed(&mut ta, "! cd sub && te");

    assert_eq!(super::texts(&ta, &dir, &mut completion), ["just test "]);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_history_suggests_the_rest_and_right_takes_it() {
    let (dir, mut completion) = project("suggest");
    let mut ta = TextArea::new(false, "");

    typed(&mut ta, "! cargo t");

    assert_eq!(completion.suggestion(&ta, &[]).as_deref(), Some("est --release"));

    // the thread's own commands first (newest first)
    let commands = vec!["cargo tree".to_string()];

    assert_eq!(completion.suggestion(&ta, &commands).as_deref(), Some("ree"));

    // alt+→: 1 word; →: the rest
    typed(&mut ta, "! cargo");

    assert!(completion.accept(KeyEvent::new(KeyCode::Right, KeyModifiers::ALT), &mut ta, &[]));
    assert_eq!(ta.text(), "! cargo test");
    assert!(completion.accept(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE), &mut ta, &[]));
    assert_eq!(ta.text(), "! cargo test --release");
    assert!(!completion.accept(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE), &mut ta, &[]));

    // the cursor not at the end: 0 suggestion
    typed(&mut ta, "! cargo t");
    ta.move_start();

    assert!(completion.suggestion(&ta, &[]).is_none());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn fish_completes_arguments_with_descriptions() {
    if !crate::engine::harnesses::on_path("fish") {
        return;
    }

    let rows = fish_complete("git chec", Path::new("/tmp"));

    assert!(
        rows.iter()
            .any(|(word, description)| word == "checkout" && !description.is_empty()),
        "{rows:?}"
    );

    let (dir, mut completion) = project("fish");

    completion.fish = true;
    let mut ta = TextArea::new(false, "");

    typed(&mut ta, "! git chec");

    assert!(super::texts(&ta, &dir, &mut completion).contains(&"checkout ".to_string()));

    let _ = std::fs::remove_dir_all(&dir);
}
