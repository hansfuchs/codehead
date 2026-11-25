use super::*;

fn ta(text: &str) -> TextArea {
    let mut thread = TextArea::new(false, "");

    thread.set_text(text);
    thread
}

#[test]
fn image_marker_backspace_is_atomic_after_paste_and_undo_restores_it() {
    let mut thread = ta("");

    thread.insert("日本 [Image #1] ");

    // the space after the marker: 1 char, the marker stays
    thread.backspace(false);

    assert_eq!(thread.text(), "日本 [Image #1]");

    thread.backspace(false);

    assert_eq!(thread.text(), "日本 ");

    thread.restore(true);

    assert_eq!(thread.text(), "日本 [Image #1]");

    thread = ta("日本 [Image #1] ");
    thread.cur = Pos { row: 0, col: 7 };
    thread.anchor = None;
    thread.backspace(false);

    assert_eq!(thread.text(), "日本  ");
}

#[test]
fn arrows_and_shift_arrows_select_complete_image_markers() {
    let mut thread = ta("a [Image #2] z");

    thread.cur = Pos { row: 0, col: 2 };
    thread.key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));

    assert_eq!(thread.selected_text().as_deref(), Some("[Image #2]"));

    thread.key(KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE));

    assert_eq!(thread.text(), "a  z");

    thread.restore(true);
    thread.cur = Pos { row: 0, col: 12 };
    thread.anchor = None;
    thread.key(KeyEvent::new(KeyCode::Left, KeyModifiers::SHIFT));

    assert_eq!(thread.selected_text().as_deref(), Some("[Image #2]"));
}

#[test]
fn delete_inside_a_marker_removes_the_whole_marker() {
    let mut thread = ta("[Image #3]");

    thread.cur = Pos { row: 0, col: 4 };
    thread.delete(false);

    assert_eq!(thread.text(), "");

    thread.restore(true);

    assert_eq!(thread.text(), "[Image #3]");
}

#[test]
fn arrows_after_an_edit_without_a_render_read_fresh_rows() {
    // 1 event batch: the text shrinks, then ↑ / ↓ / end before the next render
    let mut thread = ta("first line\nsecond line that is long\nthird");

    thread.render_for_test(12);
    thread.set_text("x");

    for code in [KeyCode::Up, KeyCode::Down, KeyCode::End, KeyCode::Home] {
        thread.key(KeyEvent::new(code, KeyModifiers::NONE));
    }

    assert_eq!(thread.text(), "x");
    assert!(thread.cur.row == 0 && thread.cur.col <= 1);
}

#[test]
fn wrap_breaks_after_spaces() {
    let line: Vec<char> = "hello world foo".chars().collect();

    assert_eq!(wrap(&line, 11), vec![(0, 6), (6, 15)]);
    assert_eq!(wrap(&line, 8), vec![(0, 6), (6, 12), (12, 15)]);

    let long: Vec<char> = "abcdefgh".chars().collect();

    assert_eq!(wrap(&long, 3), vec![(0, 3), (3, 6), (6, 8)]);
}

#[test]
fn backspace_removes_whole_selection() {
    let mut thread = ta("one\ntwo three");

    thread.anchor = Some(Pos { row: 0, col: 1 });
    thread.cur = Pos { row: 1, col: 3 };

    assert_eq!(thread.selected_text().unwrap(), "ne\ntwo");

    thread.backspace(false);

    assert_eq!(thread.text(), "o three");

    thread.restore(true);

    assert_eq!(thread.text(), "one\ntwo three");
}

#[test]
fn cmd_backspace_deletes_to_row_start() {
    let mut thread = ta("one two\nthree four");

    thread.render_for_test(40);
    thread.cur = Pos { row: 1, col: 5 };
    thread.kill_to_edge(false);

    assert_eq!(thread.text(), "one two\n four");

    // at the row start: joins with the line above
    thread.kill_to_edge(false);

    assert_eq!(thread.text(), "one two four");

    thread.restore(true);

    assert_eq!(thread.text(), "one two\n four");

    thread.cur = Pos { row: 0, col: 3 };
    thread.kill_to_edge(true);

    assert_eq!(thread.text(), "one\n four");
}

#[test]
fn double_space_never_adds_a_period() {
    let key = |character: KeyCode| KeyEvent::new(character, KeyModifiers::NONE);
    let t0 = Instant::now();
    let ms = |count: u64| t0 + Duration::from_millis(count);

    // macOS: 2nd space → `.` + space in 1 burst
    let mut thread = ta("word");

    thread.move_end();
    thread.key_at(key(KeyCode::Char(' ')), ms(0));
    thread.key_at(key(KeyCode::Char('.')), ms(150));
    thread.key_at(key(KeyCode::Char(' ')), ms(151));

    assert_eq!(thread.text(), "word  ");

    // terminal variant: ⌫ `.` space
    let mut thread_value = ta("word");

    thread_value.move_end();
    thread_value.key_at(key(KeyCode::Char(' ')), ms(0));
    thread_value.key_at(key(KeyCode::Backspace), ms(150));
    thread_value.key_at(key(KeyCode::Char('.')), ms(150));
    thread_value.key_at(key(KeyCode::Char(' ')), ms(151));

    assert_eq!(thread_value.text(), "word  ");

    // the 2nd space as a key, then `.` + space (2 writes of 1 key press): 2 spaces, not 3
    let mut thread_2 = ta("word");

    thread_2.move_end();
    thread_2.key_at(key(KeyCode::Char(' ')), ms(0));
    thread_2.key_at(key(KeyCode::Char(' ')), ms(150));
    thread_2.key_at(key(KeyCode::Char('.')), ms(170));
    thread_2.key_at(key(KeyCode::Char(' ')), ms(170));

    assert_eq!(thread_2.text(), "word  ");

    // `.` + space, then the 2nd space as a key: 2 spaces, not 3
    let mut thread_3 = ta("word");

    thread_3.move_end();
    thread_3.key_at(key(KeyCode::Char(' ')), ms(0));
    thread_3.key_at(key(KeyCode::Char('.')), ms(150));
    thread_3.key_at(key(KeyCode::Char(' ')), ms(150));
    thread_3.key_at(key(KeyCode::Char(' ')), ms(165));

    assert_eq!(thread_3.text(), "word  ");

    // a 3rd space typed by the person: kept
    thread_3.key_at(key(KeyCode::Char(' ')), ms(400));

    assert_eq!(thread_3.text(), "word   ");

    // a person typing " . ": kept
    let mut thread_4 = ta("a");

    thread_4.move_end();
    thread_4.key_at(key(KeyCode::Char(' ')), ms(0));
    thread_4.key_at(key(KeyCode::Char('.')), ms(120));
    thread_4.key_at(key(KeyCode::Char(' ')), ms(240));

    assert_eq!(thread_4.text(), "a . ");

    // end of a sentence: kept
    let mut thread_5 = ta("end");

    thread_5.move_end();
    thread_5.key_at(key(KeyCode::Char('.')), ms(0));
    thread_5.key_at(key(KeyCode::Char(' ')), ms(1));

    assert_eq!(thread_5.text(), "end. ");
}

#[test]
fn key_stamps_keep_a_slow_handled_burst_together() {
    let key_value = |character: KeyCode| KeyEvent::new(character, KeyModifiers::NONE);
    let t0 = Instant::now();
    let mut thread_value = ta("word");

    thread_value.move_end();
    stamp(Some(t0));
    thread_value.key(key_value(KeyCode::Char(' ')));

    // the burst read at 150ms; handled later (slow dropdown work): the read time counts
    stamp(Some(t0 + Duration::from_millis(150)));
    thread_value.key(key_value(KeyCode::Char('.')));
    std::thread::sleep(Duration::from_millis(20));
    stamp(Some(t0 + Duration::from_millis(151)));
    thread_value.key(key_value(KeyCode::Char(' ')));
    stamp(None);

    assert_eq!(thread_value.text(), "word  ");
}

#[test]
fn mentions_get_the_agent_color() {
    let marks = vec![("amon".to_string(), Color::Red), ("lilith".to_string(), Color::Blue)];
    let line: Vec<char> = "ask @amon, (@lilith) mail@amon @amonx".chars().collect();
    let character = mention_colors(&line, &marks);
    let colored: String = line
        .iter()
        .zip(&character)
        .filter(|(_, character_value)| character_value.is_some())
        .map(|(ch, _)| *ch)
        .collect();

    assert_eq!(colored, "@amon@lilith");
    assert_eq!(character[4], Some(Color::Red));
    assert_eq!(character[12], Some(Color::Blue));

    // a replace note: in the color of the new agent, @lilith in its own color
    let line_value: Vec<char> = "@amon (replaces @lilith) go".chars().collect();
    let character_2 = mention_colors(&line_value, &marks);
    let want = |i: usize| if (16..23).contains(&i) { Color::Blue } else { Color::Red };

    assert!(
        character_2[..24]
            .iter()
            .enumerate()
            .all(|(i, x)| i == 5 || *x == Some(want(i))),
        "{character_2:?}"
    );
    assert_eq!(character_2[5], None);
    assert_eq!(character_2[24..], [None, None, None]);
}

#[test]
fn mention_query_before_cursor() {
    let mut thread = ta("hi @am");

    thread.move_end();

    assert_eq!(thread.mention(), Some((Pos { row: 0, col: 3 }, "am".into())));

    thread.replace_to_cursor(Pos { row: 0, col: 3 }, "@amon ");

    assert_eq!(thread.text(), "hi @amon ");
    assert_eq!(thread.mention(), None);

    let mut thread_value = ta("mail@x");

    thread_value.move_end();

    assert_eq!(thread_value.mention(), None);

    let mut thread_2 = ta("@");

    thread_2.move_end();

    assert_eq!(thread_2.mention(), Some((Pos { row: 0, col: 0 }, String::new())));
}
