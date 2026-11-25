use super::*;

fn ta(text: &str) -> TextArea {
    let mut input = TextArea::new(false, "");

    input.set_text(text);
    input
}

#[test]
fn image_marker_backspace_is_atomic_after_paste_and_undo_restores_it() {
    let mut input = ta("");

    input.insert("日本 [Image #1] ");

    // the space after the marker: 1 char, the marker stays
    input.backspace(false);

    assert_eq!(input.text(), "日本 [Image #1]");

    input.backspace(false);

    assert_eq!(input.text(), "日本 ");

    input.restore(true);

    assert_eq!(input.text(), "日本 [Image #1]");

    input = ta("日本 [Image #1] ");
    input.cur = Pos { row: 0, col: 7 };
    input.anchor = None;
    input.backspace(false);

    assert_eq!(input.text(), "日本  ");
}

#[test]
fn arrows_and_shift_arrows_select_complete_image_markers() {
    let mut input = ta("a [Image #2] z");

    input.cur = Pos { row: 0, col: 2 };
    input.key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));

    assert_eq!(input.selected_text().as_deref(), Some("[Image #2]"));

    input.key(KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE));

    assert_eq!(input.text(), "a  z");

    input.restore(true);
    input.cur = Pos { row: 0, col: 12 };
    input.anchor = None;
    input.key(KeyEvent::new(KeyCode::Left, KeyModifiers::SHIFT));

    assert_eq!(input.selected_text().as_deref(), Some("[Image #2]"));
}

#[test]
fn delete_inside_a_marker_removes_the_whole_marker() {
    let mut input = ta("[Image #3]");

    input.cur = Pos { row: 0, col: 4 };
    input.delete(false);

    assert_eq!(input.text(), "");

    input.restore(true);

    assert_eq!(input.text(), "[Image #3]");
}

#[test]
fn arrows_after_an_edit_without_a_render_read_fresh_rows() {
    // 1 event batch: the text shrinks, then ↑ / ↓ / end before the next render
    let mut input = ta("first line\nsecond line that is long\nthird");

    input.render_for_test(12);
    input.set_text("x");

    for code in [KeyCode::Up, KeyCode::Down, KeyCode::End, KeyCode::Home] {
        input.key(KeyEvent::new(code, KeyModifiers::NONE));
    }

    assert_eq!(input.text(), "x");
    assert!(input.cur.row == 0 && input.cur.col <= 1);
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
    let mut input = ta("one\ntwo three");

    input.anchor = Some(Pos { row: 0, col: 1 });
    input.cur = Pos { row: 1, col: 3 };

    assert_eq!(input.selected_text().unwrap(), "ne\ntwo");

    input.backspace(false);

    assert_eq!(input.text(), "o three");

    input.restore(true);

    assert_eq!(input.text(), "one\ntwo three");
}

#[test]
fn cmd_backspace_deletes_to_row_start() {
    let mut input = ta("one two\nthree four");

    input.render_for_test(40);
    input.cur = Pos { row: 1, col: 5 };
    input.kill_to_edge(false);

    assert_eq!(input.text(), "one two\n four");

    // at the row start: joins with the line above
    input.kill_to_edge(false);

    assert_eq!(input.text(), "one two four");

    input.restore(true);

    assert_eq!(input.text(), "one two\n four");

    input.cur = Pos { row: 0, col: 3 };
    input.kill_to_edge(true);

    assert_eq!(input.text(), "one\n four");
}

#[test]
fn double_space_never_adds_a_period() {
    let key = |code: KeyCode| KeyEvent::new(code, KeyModifiers::NONE);
    let t0 = Instant::now();
    let ms = |count: u64| t0 + Duration::from_millis(count);

    // macOS: 2nd space → `.` + space in 1 burst
    let mut input = ta("word");

    input.move_end();
    input.key_at(key(KeyCode::Char(' ')), ms(0));
    input.key_at(key(KeyCode::Char('.')), ms(150));
    input.key_at(key(KeyCode::Char(' ')), ms(151));

    assert_eq!(input.text(), "word  ");

    // terminal variant: ⌫ `.` space
    let mut input = ta("word");

    input.move_end();
    input.key_at(key(KeyCode::Char(' ')), ms(0));
    input.key_at(key(KeyCode::Backspace), ms(150));
    input.key_at(key(KeyCode::Char('.')), ms(150));
    input.key_at(key(KeyCode::Char(' ')), ms(151));

    assert_eq!(input.text(), "word  ");

    // the 2nd space as a key, then `.` + space (2 writes of 1 key press): 2 spaces, not 3
    let mut input = ta("word");

    input.move_end();
    input.key_at(key(KeyCode::Char(' ')), ms(0));
    input.key_at(key(KeyCode::Char(' ')), ms(150));
    input.key_at(key(KeyCode::Char('.')), ms(170));
    input.key_at(key(KeyCode::Char(' ')), ms(170));

    assert_eq!(input.text(), "word  ");

    // `.` + space, then the 2nd space as a key: 2 spaces, not 3
    let mut input = ta("word");

    input.move_end();
    input.key_at(key(KeyCode::Char(' ')), ms(0));
    input.key_at(key(KeyCode::Char('.')), ms(150));
    input.key_at(key(KeyCode::Char(' ')), ms(150));
    input.key_at(key(KeyCode::Char(' ')), ms(165));

    assert_eq!(input.text(), "word  ");

    // a 3rd space typed by the person: kept
    input.key_at(key(KeyCode::Char(' ')), ms(400));

    assert_eq!(input.text(), "word   ");

    // a person typing " . ": kept
    let mut input = ta("a");

    input.move_end();
    input.key_at(key(KeyCode::Char(' ')), ms(0));
    input.key_at(key(KeyCode::Char('.')), ms(120));
    input.key_at(key(KeyCode::Char(' ')), ms(240));

    assert_eq!(input.text(), "a . ");

    // end of a sentence: kept
    let mut input = ta("end");

    input.move_end();
    input.key_at(key(KeyCode::Char('.')), ms(0));
    input.key_at(key(KeyCode::Char(' ')), ms(1));

    assert_eq!(input.text(), "end. ");
}

#[test]
fn key_stamps_keep_a_slow_handled_burst_together() {
    let key = |code: KeyCode| KeyEvent::new(code, KeyModifiers::NONE);
    let t0 = Instant::now();
    let mut input = ta("word");

    input.move_end();
    stamp(Some(t0));
    input.key(key(KeyCode::Char(' ')));

    // the burst read at 150ms; handled later (slow dropdown work): the read time counts
    stamp(Some(t0 + Duration::from_millis(150)));
    input.key(key(KeyCode::Char('.')));
    std::thread::sleep(Duration::from_millis(20));
    stamp(Some(t0 + Duration::from_millis(151)));
    input.key(key(KeyCode::Char(' ')));
    stamp(None);

    assert_eq!(input.text(), "word  ");
}

#[test]
fn mentions_get_the_agent_color() {
    let marks = vec![("amon".to_string(), Color::Red), ("lilith".to_string(), Color::Blue)];
    let line: Vec<char> = "ask @amon, (@lilith) mail@amon @amonx".chars().collect();
    let colors = mention_colors(&line, &marks);
    let colored: String = line
        .iter()
        .zip(&colors)
        .filter(|(_, color)| color.is_some())
        .map(|(ch, _)| *ch)
        .collect();

    assert_eq!(colored, "@amon@lilith");
    assert_eq!(colors[4], Some(Color::Red));
    assert_eq!(colors[12], Some(Color::Blue));

    // a replace note: in the color of the new agent, @lilith in its own color
    let line: Vec<char> = "@amon (replaces @lilith) go".chars().collect();
    let colors = mention_colors(&line, &marks);
    let want = |i: usize| if (16..23).contains(&i) { Color::Blue } else { Color::Red };

    assert!(
        colors[..24]
            .iter()
            .enumerate()
            .all(|(i, x)| i == 5 || *x == Some(want(i))),
        "{colors:?}"
    );
    assert_eq!(colors[5], None);
    assert_eq!(colors[24..], [None, None, None]);
}

#[test]
fn mention_query_before_cursor() {
    let mut input = ta("hi @am");

    input.move_end();

    assert_eq!(input.mention(), Some((Pos { row: 0, col: 3 }, "am".into())));

    input.replace_to_cursor(Pos { row: 0, col: 3 }, "@amon ");

    assert_eq!(input.text(), "hi @amon ");
    assert_eq!(input.mention(), None);

    let mut input = ta("mail@x");

    input.move_end();

    assert_eq!(input.mention(), None);

    let mut input = ta("@");

    input.move_end();

    assert_eq!(input.mention(), Some((Pos { row: 0, col: 0 }, String::new())));
}
