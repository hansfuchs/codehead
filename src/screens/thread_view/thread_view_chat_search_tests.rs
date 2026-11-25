use super::*;
use ratatui::backend::TestBackend;

fn view(id: &str) -> View {
    let (tx, _rx) = crate::infra::bus::channel();
    let mut view = View::new(id, Path::new("/tmp"), "t", tx);

    for i in 0..40u64 {
        let text = if i % 10 == 3 {
            format!("message {i}: the needle is here")
        } else {
            format!("message {i}: filler text")
        };

        view.thread.push(&store::Event {
            ts: i + 1,
            agent: Some("ada".into()),
            kind: store::Kind::Text { text },
        });
    }

    view.stream.dirty = true;
    view
}

fn key(view: &mut View, character: KeyCode, message: KeyModifiers) {
    view.key(KeyEvent::new(character, message));
}

fn typed(view: &mut View, text: &str) {
    for character in text.chars() {
        key(view, KeyCode::Char(character), KeyModifiers::NONE);
    }
}

fn screen(view: &mut View) -> Vec<String> {
    let mut thread = Terminal::new(TestBackend::new(100, 30)).unwrap();

    thread.draw(|file| view.draw(file)).unwrap();
    let bytes = thread.backend().buffer().clone();

    (0..30)
        .map(|y| (0..100).map(|x| bytes[(x, y)].symbol().to_string()).collect::<String>())
        .collect()
}

#[test]
fn ctrl_f_searches_the_chat_enter_cycles_esc_closes_the_draft_stays() {
    let mut value = view("chatsearch-0000");

    typed(&mut value, "draft");
    screen(&mut value);
    key(&mut value, KeyCode::Char('f'), KeyModifiers::CONTROL);

    assert!(value.find.open && value.find.focus);

    typed(&mut value, "needle");

    assert_eq!(
        value.prompt.input.text(),
        "draft",
        "the query goes to the bar, not to the prompt"
    );

    let text = screen(&mut value);
    let bar = text
        .iter()
        .position(|line| line.contains(" find "))
        .unwrap_or_else(|| panic!("{text:#?}"));

    println!("--- chat search\n{}", text.join("\n"));

    assert!(
        text[bar].contains("needle") && text[bar].contains("4/4"),
        "{}",
        text[bar]
    );
    assert!(
        text.iter().any(|line| line.contains("message 33: the needle")),
        "the newest match in view: {text:#?}"
    );

    // enter: the next older match, scrolled into view with the marker
    key(&mut value, KeyCode::Enter, KeyModifiers::NONE);
    let text = screen(&mut value);

    assert!(
        text.iter().any(|line| line.contains("message 23: the needle")),
        "{text:#?}"
    );
    assert!(text.iter().any(|line| line.contains("3/4")), "{text:#?}");
    assert!(
        text.iter()
            .any(|line| line.starts_with('▌') && line.contains("message 23")),
        "the marker: {text:#?}"
    );

    // ↓: newer again; wraps past the newest to the oldest
    key(&mut value, KeyCode::Down, KeyModifiers::NONE);
    key(&mut value, KeyCode::Down, KeyModifiers::NONE);
    let text = screen(&mut value);

    assert!(
        text.iter().any(|line| line.contains("1/4")) && text.iter().any(|line| line.contains("message 3: the needle")),
        "{text:#?}"
    );

    // esc: the bar goes, the stream stays, the keys go to the prompt
    let back = value.stream.back;

    key(&mut value, KeyCode::Esc, KeyModifiers::NONE);

    assert!(!value.find.open && value.stream.back == back);

    let text = screen(&mut value);

    assert!(!text.iter().any(|line| line.contains(" find ")), "{text:#?}");

    typed(&mut value, "!");

    assert_eq!(value.prompt.input.text(), "draft!");
}

#[test]
fn the_footer_shows_the_mode_as_a_badge() {
    let mut value = view("chatsearch-0003");
    let last = |text: &[String]| text.last().cloned().unwrap_or_default();

    assert!(!last(&screen(&mut value)).contains(" SHELL "));

    typed(&mut value, "!ls");
    let text = screen(&mut value);

    assert!(last(&text).trim_start().starts_with("SHELL"), "{}", last(&text));

    value.prompt.input.set_text("");
    key(&mut value, KeyCode::Char('f'), KeyModifiers::CONTROL);
    let text = screen(&mut value);

    assert!(last(&text).trim_start().starts_with("SEARCH"), "{}", last(&text));

    key(&mut value, KeyCode::Esc, KeyModifiers::NONE);

    assert!(!last(&screen(&mut value)).contains("SEARCH"));
}

#[test]
fn a_query_with_0_matches_says_so_and_keeps_the_stream() {
    let mut value = view("chatsearch-0001");

    screen(&mut value);
    key(&mut value, KeyCode::Char('f'), KeyModifiers::CONTROL);
    typed(&mut value, "zzqq");
    let text = screen(&mut value);

    assert!(text.iter().any(|line| line.contains("0 matches")), "{text:#?}");
    assert_eq!(value.stream.back, 0);
}

#[test]
fn ctrl_f_in_another_view_is_its_filter_not_the_chat_search() {
    let mut value = view("chatsearch-0002");

    key(&mut value, KeyCode::F(4), KeyModifiers::NONE);

    assert_eq!(value.panes.pane, Pane::Files);

    key(&mut value, KeyCode::Char('f'), KeyModifiers::CONTROL);

    assert!(value.panes.files.typing() && !value.find.open);
}
