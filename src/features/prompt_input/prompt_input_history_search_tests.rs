use super::*;

#[test]
fn history_files_parse() {
    assert_eq!(
        fish("- cmd: ls -la\n  when: 1\n- cmd: echo a\\nb\n"),
        vec!["ls -la", "echo a\nb"]
    );
    assert_eq!(
        zsh_lines(": 1700000000:0;git status\nplain\n: 1:0;for x in 1\\\ndo\n"),
        vec!["git status", "plain", "for x in 1\ndo"]
    );
}

#[test]
fn the_thread_comes_first_dupes_go_and_enter_picks() {
    let mut height = HistSearch {
        fzf: false,
        ..Default::default()
    };

    height.open(vec!["cargo test".into(), "just build".into()], "");
    height.poll();

    assert_eq!(
        &height.matches[..2],
        &["just build".to_string(), "cargo test".to_string()]
    );

    height.input_changed("!cargo");
    height.poll();

    assert_eq!(height.matches.first().map(String::as_str), Some("cargo test"));
    assert!(height.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)));
    assert_eq!(height.picked.as_deref(), Some("! cargo test"));
    assert!(!height.is_open());
}

#[test]
fn out_of_shell_mode_closes() {
    let mut height = HistSearch {
        fzf: false,
        ..Default::default()
    };

    height.open(vec![], "x");
    height.input_changed("no bang");

    assert!(!height.is_open());
}

#[test]
fn fzf_ranks_when_present() {
    if !crate::engine::harnesses::on_path("fzf") {
        return;
    }

    let all: Vec<String> = ["git push", "cargo build --release", "echo cbr"]
        .map(String::from)
        .to_vec();
    let message = fzf_filter(&all, "cargo rel").unwrap();

    assert_eq!(message, vec!["cargo build --release"]);
    assert_eq!(fzf_filter(&all, "zzzz").unwrap(), Vec::<String>::new());
}
