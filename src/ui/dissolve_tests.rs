use super::*;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;

fn paint(
    thread: &mut ratatui::Terminal<ratatui::backend::TestBackend>,
    directory: &mut Dissolve,
    text: &str,
    version: u64,
) -> String {
    thread
        .draw(|file| {
            file.render_widget(Paragraph::new(Line::raw(text.to_string())), file.area());
            directory.apply(file, file.area(), version);
        })
        .unwrap();

    let bytes = thread.backend().buffer().clone();

    (0..bytes.area.width)
        .map(|x| bytes[(x, 0)].symbol().to_string())
        .collect()
}

#[test]
fn new_data_dissolves_in_a_cursor_move_shows_at_once() {
    let mut thread = ratatui::Terminal::new(ratatui::backend::TestBackend::new(20, 1)).unwrap();
    let mut directory = Dissolve::default();

    // the 1st paint: at once
    assert_eq!(paint(&mut thread, &mut directory, "old text", 1).trim_end(), "old text");

    // the same generation, other cells (a cursor move): at once
    assert_eq!(
        paint(&mut thread, &mut directory, "old text!", 1).trim_end(),
        "old text!"
    );

    // a new generation: a mix, then (after TIME) the new text
    let mid = paint(&mut thread, &mut directory, "new words", 2);

    assert_ne!(mid.trim_end(), "new words");
    assert!(active());

    directory.morph.as_mut().unwrap().1 -= TIME;

    assert_eq!(
        paint(&mut thread, &mut directory, "new words", 2).trim_end(),
        "new words"
    );
    assert!(directory.morph.is_none());
}

#[test]
fn a_mix_moves_only_the_changed_cells() {
    let mut from = Buffer::empty(Rect::new(0, 0, 10, 1));
    let mut to = Buffer::empty(Rect::new(0, 0, 10, 1));

    from.set_string(0, 0, "ab   ", ratatui::style::Style::default());
    to.set_string(0, 0, "ab cde", ratatui::style::Style::default());
    let message = mix(&from, &to, 0.0);

    assert_eq!((message[(0, 0)].symbol(), message[(1, 0)].symbol()), ("a", "b"));
    assert_eq!(mix(&from, &to, 1.0), to);
}
