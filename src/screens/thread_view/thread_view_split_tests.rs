use super::*;
use ratatui::backend::TestBackend;

fn screen(view: &mut View, width: u16) -> Vec<String> {
    let mut term = Terminal::new(TestBackend::new(width, 30)).unwrap();

    term.draw(|frame| view.draw(frame)).unwrap();
    let buf = term.backend().buffer().clone();

    (0..30)
        .map(|y| (0..width).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>())
        .collect()
}

#[test]
fn wide_changes_view_has_the_chat_right_and_alt_backslash_toggles() {
    let (tx, _rx) = crate::infra::bus::channel();
    let mut view = View::new("splittest-0000", Path::new("/tmp"), "t", tx);

    view.set_pane(Pane::Changes);
    let text = screen(&mut view, 220);

    // the divider at 3/5 of the width: the changes left, the chat column right
    assert!(text[2].chars().nth(132) == Some('│'), "{}", text[2]);
    assert!(
        view.layout.view_rect.x == 0 && view.layout.view_rect.right() <= 132 && view.layout.stream_rect.x == 133,
        "{:?} {:?}",
        view.layout.stream_rect,
        view.layout.view_rect
    );

    // the prompt in the chat column
    assert!(view.layout.input_rect.x >= 133, "{:?}", view.layout.input_rect);

    view.key(KeyEvent::new(KeyCode::Char('\\'), KeyModifiers::ALT));
    screen(&mut view, 220);

    assert_eq!(view.layout.view_rect.x, 0, "full width");

    // narrow: 0 split
    view.key(KeyEvent::new(KeyCode::Char('\\'), KeyModifiers::ALT));
    screen(&mut view, 150);

    assert_eq!(view.layout.view_rect.x, 0);
}

#[test]
fn a_thread_outside_git_opens_in_the_chat_even_when_wide() {
    let (tx, _rx) = crate::infra::bus::channel();
    let mut view = View::new("splittest-0001", Path::new("/"), "t", tx);

    screen(&mut view, 220);

    assert_eq!(view.panes.pane, Pane::Chat);
}
