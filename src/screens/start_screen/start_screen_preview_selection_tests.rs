use super::start_screen_tests::app_with;
use super::*;
use ratatui::backend::TestBackend;

fn screen(app: &mut App) -> Vec<String> {
    let mut thread = ratatui::Terminal::new(TestBackend::new(120, 30)).unwrap();

    thread.draw(|file| app.draw(file)).unwrap();
    let bytes = thread.backend().buffer().clone();

    (0..30)
        .map(|y| (0..120).map(|x| bytes[(x, y)].symbol().to_string()).collect::<String>())
        .collect()
}

fn mouse(app: &mut App, kind: MouseEventKind, x: u16, y: u16) {
    app.mouse(MouseEvent {
        kind,
        column: x,
        row: y,
        modifiers: KeyModifiers::NONE,
    });
}

#[test]
fn the_preview_selects_and_copies() {
    let mut app = app_with(None);

    app.agent_list.agents = vec![Agent {
        name: "zagan".into(),
        harness: "amp".into(),
        mode: "plan".into(),
        ..Default::default()
    }];
    app.switch_tab(Tab::Agents);
    app.agent_list.agent_reads = Reads::default();
    app.tabs_focus = false;
    let text = screen(&mut app);
    let path = app.preview.regions.preview;
    let y = (path.y..path.bottom())
        .find(|&y| text[y as usize].contains("mode "))
        .unwrap_or_else(|| panic!("{text:#?}"));
    let x = text[y as usize]
        .find("plan")
        .map(|bytes| text[y as usize][..bytes].chars().count())
        .unwrap() as u16;

    assert!(path.contains(Position { x, y }));

    // a drag over "plan"
    mouse(&mut app, MouseEventKind::Down(MouseButton::Left), x, y);
    mouse(&mut app, MouseEventKind::Drag(MouseButton::Left), x + 3, y);
    mouse(&mut app, MouseEventKind::Up(MouseButton::Left), x + 3, y);

    assert_eq!(crate::infra::clip::LAST.with_borrow(std::clone::Clone::clone), "plan");

    // drawn: accent cells
    let mut thread = ratatui::Terminal::new(TestBackend::new(120, 30)).unwrap();

    thread.draw(|file| app.draw(file)).unwrap();

    assert_eq!(thread.backend().buffer()[(x, y)].bg, theme::pal().accent);
}
