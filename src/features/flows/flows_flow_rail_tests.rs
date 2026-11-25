use super::*;

fn text(buf: &Buffer, area: Rect) -> Vec<String> {
    (area.y..area.bottom())
        .map(|y| {
            (area.x..area.right())
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect()
}

#[test]
fn nested_arcs_take_their_own_columns() {
    let rows: Vec<Row> = ["a", "b", "c", "d"]
        .iter()
        .map(|style| Row {
            mark: ("○".into(), Style::default()),
            title: style.to_string(),
            ..Default::default()
        })
        .collect();
    let arcs = vec![
        Arc {
            from: 3,
            to: 0,
            label: "outer".into(),
            loops: true,
        },
        Arc {
            from: 2,
            to: 1,
            label: "inner".into(),
            loops: false,
        },
    ];
    let area = Rect::new(0, 0, 40, 4);
    let mut buf = Buffer::empty(area);

    draw(&mut buf, area, &rows, &arcs, Some(1), true, 0);
    let thread = text(&buf, area);

    assert!(thread[0].contains("◀────┐"), "{thread:?}");
    assert!(thread[1].contains("◀─┐  │"), "{thread:?}");
    assert!(
        thread[2].contains("──┘  │") && thread[2].ends_with("inner"),
        "{thread:?}"
    );
    assert!(
        thread[3].contains("─────┘") && thread[3].ends_with("outer"),
        "{thread:?}"
    );
}
