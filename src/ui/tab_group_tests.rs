use super::*;

fn labels(count: usize) -> Vec<String> {
    (0..count).map(|i| format!("repo-{i}")).collect()
}

fn row(group: &mut TabGroup, width: u16, line: &[String]) -> String {
    let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, 1)).unwrap();

    term.draw(|file| group.draw(file, file.area(), "repo", line, true))
        .unwrap();
    let buf = term.backend().buffer().clone();

    (0..width).map(|x| buf[(x, 0)].symbol().to_string()).collect()
}

#[test]
fn steps_wrap_clicks_pick_and_a_narrow_row_keeps_the_pick_in_view() {
    let line = labels(6);
    let mut group = TabGroup::default();

    assert!(group.step(-1, 6));
    assert_eq!(group.i, 5);
    assert!(group.step(1, 6));
    assert_eq!(group.i, 0);

    let wide = row(&mut group, 120, &line);

    assert!(
        wide.contains(" repo-0 ") && wide.contains("repo-5") && !wide.contains('‹'),
        "{wide}"
    );

    // narrow: the pick (5) in view, ‹ for the hidden ones before it
    group.i = 5;
    let narrow = row(&mut group, 40, &line);

    assert!(
        narrow.contains("repo-5") && narrow.contains('‹') && !narrow.contains("repo-0"),
        "{narrow}"
    );

    // a click on a drawn tab picks it
    let (result, key) = group.rects[0];

    assert!(group.click(Position {
        x: result.x,
        y: result.y
    }));
    assert_eq!(group.i, key);
    assert!(!group.click(Position { x: 0, y: 5 }));
}
