use super::*;
use ratatui::backend::TestBackend;

const WIDTH: u16 = 60;
const HEIGHT: u16 = 30;

fn buffer(view: &mut View) -> ratatui::buffer::Buffer {
    let mut term = Terminal::new(TestBackend::new(WIDTH, HEIGHT)).unwrap();

    term.draw(|frame| view.draw(frame)).unwrap();
    term.backend().buffer().clone()
}

fn text(buf: &ratatui::buffer::Buffer) -> Vec<String> {
    (0..HEIGHT)
        .map(|y| (0..WIDTH).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>())
        .collect()
}

/// The rows with the hover bg in the chat column.
fn lit(buf: &ratatui::buffer::Buffer) -> Vec<u16> {
    (0..HEIGHT)
        .filter(|&y| (0..WIDTH).any(|x| buf[(x, y)].bg == theme::pal().surface1))
        .collect()
}

fn mouse(view: &mut View, kind: MouseEventKind, x: u16, y: u16) {
    view.mouse(MouseEvent {
        kind,
        column: x,
        row: y,
        modifiers: KeyModifiers::NONE,
    });
}

fn click(view: &mut View, y: u16) {
    mouse(view, MouseEventKind::Down(MouseButton::Left), 10, y);
    mouse(view, MouseEventKind::Up(MouseButton::Left), 10, y);
}

fn row_of(rows: &[String], needle: &str) -> u16 {
    rows.iter()
        .position(|row| row.contains(needle))
        .unwrap_or_else(|| panic!("{needle} not drawn: {rows:#?}")) as u16
}

fn tool(view: &mut View, id: &str, name: &str, input: serde_json::Value) {
    view.thread.push(&store::Event {
        ts: 2,
        agent: Some("ada".into()),
        kind: store::Kind::Tool {
            id: id.into(),
            name: name.into(),
            input,
        },
    });
    view.thread.push(&store::Event {
        ts: 3,
        agent: Some("ada".into()),
        kind: store::Kind::ToolResult {
            id: id.into(),
            ok: true,
            output: "done".into(),
        },
    });
}

/// A thread with 1 run of 2 calls, then the agent's answer.
fn run_view(id: &str) -> View {
    let (tx, _rx) = crate::infra::bus::channel();
    let mut view = View::new(id, Path::new("/tmp"), "t", tx);

    view.agents.members = vec!["ada".into()];
    view.thread.push(&store::Event {
        ts: 1,
        agent: Some("ada".into()),
        kind: store::Kind::TurnStart,
    });
    tool(
        &mut view,
        "1",
        "Bash",
        serde_json::json!({"command": "ls", "description": "list"}),
    );
    tool(&mut view, "2", "Read", serde_json::json!({"file_path": "x.rs"}));
    view.thread.push(&store::Event {
        ts: 4,
        agent: Some("ada".into()),
        kind: store::Kind::Text {
            text: "the answer".into(),
        },
    });

    view
}

#[test]
fn the_hover_lights_the_whole_open_run_and_a_body_click_folds_it() {
    let mut view = run_view("sectiontest-0000");
    let rows = text(&buffer(&mut view));
    let head = row_of(&rows, "2 calls");

    click(&mut view, head);
    let rows = text(&buffer(&mut view));
    let head = row_of(&rows, "2 calls");
    let body = row_of(&rows, "Read x.rs");

    assert!(body > head + 1, "the run opened: {rows:#?}");

    mouse(&mut view, MouseEventKind::Moved, 10, body);
    let buf = buffer(&mut view);
    let lit_rows = lit(&buf);
    let answer = row_of(&text(&buf), "the answer");

    assert!(lit_rows.contains(&head) && lit_rows.contains(&body), "{lit_rows:?}");
    assert!((head..=body).all(|y| lit_rows.contains(&y)), "{lit_rows:?}");
    assert!(!lit_rows.contains(&answer), "the answer: 0 hover");

    click(&mut view, body);
    let rows = text(&buffer(&mut view));

    assert!(
        !rows.iter().any(|row| row.contains("Read x.rs")),
        "the run folded: {rows:#?}"
    );
}

#[test]
fn a_drag_over_an_open_run_selects_and_keeps_it_open() {
    let mut view = run_view("sectiontest-0001");
    let rows = text(&buffer(&mut view));

    click(&mut view, row_of(&rows, "2 calls"));
    let rows = text(&buffer(&mut view));
    let body = row_of(&rows, "Read x.rs");

    mouse(&mut view, MouseEventKind::Down(MouseButton::Left), 4, body);
    mouse(&mut view, MouseEventKind::Drag(MouseButton::Left), 20, body);
    mouse(&mut view, MouseEventKind::Up(MouseButton::Left), 20, body);
    let rows = text(&buffer(&mut view));

    assert!(
        rows.iter().any(|row| row.contains("Read x.rs")),
        "still open: {rows:#?}"
    );
    assert!(!crate::infra::clip::LAST.with_borrow(String::is_empty), "drag copied");
}
