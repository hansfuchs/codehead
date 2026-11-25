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
        .map(|title| Row {
            mark: ("○".into(), Style::default()),
            title: title.to_string(),
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
    let lines = text(&buf, area);

    assert!(lines[0].contains("◀────┐"), "{lines:?}");
    assert!(lines[1].contains("◀─┐  │"), "{lines:?}");
    assert!(lines[2].contains("──┘  │") && lines[2].ends_with("inner"), "{lines:?}");
    assert!(lines[3].contains("─────┘") && lines[3].ends_with("outer"), "{lines:?}");
}
