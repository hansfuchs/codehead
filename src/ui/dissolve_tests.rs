use super::*;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;

fn paint(
    term: &mut ratatui::Terminal<ratatui::backend::TestBackend>,
    dissolve: &mut Dissolve,
    text: &str,
    version: u64,
) -> String {
    term.draw(|frame| {
        frame.render_widget(Paragraph::new(Line::raw(text.to_string())), frame.area());
        dissolve.apply(frame, frame.area(), version);
    })
    .unwrap();

    let buf = term.backend().buffer().clone();

    (0..buf.area.width).map(|x| buf[(x, 0)].symbol().to_string()).collect()
}

#[test]
fn new_data_dissolves_in_a_cursor_move_shows_at_once() {
    let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(20, 1)).unwrap();
    let mut dissolve = Dissolve::default();

    // the 1st paint: at once
    assert_eq!(paint(&mut term, &mut dissolve, "old text", 1).trim_end(), "old text");

    // the same generation, other cells (a cursor move): at once
    assert_eq!(paint(&mut term, &mut dissolve, "old text!", 1).trim_end(), "old text!");

    // a new generation: a mix, then (after TIME) the new text
    let mid = paint(&mut term, &mut dissolve, "new words", 2);

    assert_ne!(mid.trim_end(), "new words");
    assert!(active());

    dissolve.morph.as_mut().unwrap().1 -= TIME;

    assert_eq!(paint(&mut term, &mut dissolve, "new words", 2).trim_end(), "new words");
    assert!(dissolve.morph.is_none());
}

#[test]
fn a_mix_moves_only_the_changed_cells() {
    let mut from = Buffer::empty(Rect::new(0, 0, 10, 1));
    let mut to = Buffer::empty(Rect::new(0, 0, 10, 1));

    from.set_string(0, 0, "ab   ", ratatui::style::Style::default());
    to.set_string(0, 0, "ab cde", ratatui::style::Style::default());
    let mixed = mix(&from, &to, 0.0);

    assert_eq!((mixed[(0, 0)].symbol(), mixed[(1, 0)].symbol()), ("a", "b"));
    assert_eq!(mix(&from, &to, 1.0), to);
}
