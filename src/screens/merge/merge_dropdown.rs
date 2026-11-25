use super::{Frame, Menu, Merge, Rect, Stop, UnicodeWidthStr, shift};

pub(super) fn draw_dropdown(merge: &mut Merge, frame: &mut Frame, focused: bool) {
    let Some(tab) = merge.tab_ref() else {
        return;
    };

    let Some(menu) = &tab.input.menu else {
        return;
    };

    let anchor = menu_anchor(merge, menu);

    let (label, rows, cursor, note): MenuContent = menu_content(merge, tab, menu);

    let menu_width = rows
        .iter()
        .map(|row| row.text.width() + 6)
        .max()
        .unwrap_or(20)
        .clamp(28, 60) as u16;

    let Some(menu_area) = crate::ui::dropdown::place(
        Rect {
            y: anchor.y,
            height: 1,
            ..anchor
        },
        menu_width,
        rows.len(),
        merge.layout.area,
        false,
    ) else {
        return;
    };

    let (_, hits) = crate::ui::dropdown::list(
        frame,
        menu_area,
        crate::ui::dropdown::List {
            label: &label,
            note: &note,
            rows: &rows,
            cursor,
            offset: 0,
            empty: if matches!(menu, Menu::Labels { .. }) {
                "0 labels"
            } else {
                "0 options"
            },
        },
    );
    let _ = focused;

    merge.layout.menu_hits = hits;
}

pub(super) fn menu_anchor(merge: &Merge, menu: &Menu) -> Rect {
    let anchor_stop = match menu {
        Menu::Status(..) => Stop::Status,
        Menu::Mine { thread, comment, .. } => Stop::Comment(*thread, *comment),
        Menu::Labels { .. } => Stop::AddLabel,
    };

    merge.layout.hits.iter().find(|(_, stop)| *stop == anchor_stop).map_or(
        Rect {
            x: merge.layout.body_area.x,
            y: merge.layout.body_area.y,
            width: 1,
            height: 1,
        },
        |(area, _)| *area,
    )
}

pub(super) fn menu_content(merge: &Merge, tab: &super::super::Tab, menu: &Menu) -> MenuContent {
    match menu {
        Menu::Status(cursor, options) => (
            "status".into(),
            options
                .iter()
                .map(|(option, _)| crate::ui::dropdown::Row {
                    text: option.to_string(),
                    ..Default::default()
                })
                .collect(),
            *cursor,
            String::new(),
        ),
        Menu::Mine { cur, armed, .. } => comment_menu_content(*cur, *armed),
        Menu::Labels { cur, filter } => label_menu_content(merge, tab, *cur, filter),
    }
}

pub(super) fn label_menu_content(
    merge: &Merge,
    tab: &super::super::Tab,
    cursor: usize,
    filter: &crate::ui::textarea::TextArea,
) -> MenuContent {
    let query = filter.text();
    let options = merge.label_opts(&query);
    let loading = tab
        .pr
        .as_ref()
        .is_some_and(|pr| !merge.labels.contains_key(&pr.pr.repo));

    let rows = options
        .iter()
        .map(|label| {
            let marks = query
                .split_whitespace()
                .filter_map(|word| crate::domain::matcher::word(word, &label.name))
                .flat_map(|(_, positions)| positions)
                .collect();

            crate::ui::dropdown::Row {
                text: format!("● {}", label.name),
                marks: shift(marks, 2),
                ..Default::default()
            }
        })
        .collect();

    (
        format!(
            "labels: {}",
            if query.is_empty() {
                "type to filter".to_string()
            } else {
                query
            }
        ),
        rows,
        cursor,
        if loading { "reading…".into() } else { String::new() },
    )
}

pub(super) fn comment_menu_content(cursor: usize, armed: bool) -> MenuContent {
    (
        "your comment".into(),
        vec![
            crate::ui::dropdown::Row {
                text: "edit".into(),
                ..Default::default()
            },
            crate::ui::dropdown::Row {
                text: if armed {
                    "delete: enter again".into()
                } else {
                    "delete".into()
                },
                ..Default::default()
            },
        ],
        cursor,
        String::new(),
    )
}

pub(super) type MenuContent = (String, Vec<crate::ui::dropdown::Row>, usize, String);
