use super::*;

#[test]
fn window_follows_the_cursor() {
    assert_eq!(fit(0, 0, 5, 10), 0);
    assert_eq!(fit(0, 12, 30, 10), 3);
    assert_eq!(fit(3, 12, 30, 10), 3);
    assert_eq!(fit(3, 2, 30, 10), 2);
    assert_eq!(fit(25, 29, 30, 10), 20);
    assert_eq!(fit(0, 29, 30, 10), 20);
}

#[test]
fn placed_where_room_is() {
    let area = Rect::new(0, 0, 80, 30);
    let area_value = |x, y| Rect::new(x, y, 1, 1);

    // room above: above, bottom on the row over the anchor
    assert_eq!(
        place(area_value(5, 20), 20, 4, area, true),
        Some(Rect::new(5, 14, 20, 6))
    );

    // 0 room above: below
    assert_eq!(place(area_value(5, 1), 20, 4, area, true), Some(Rect::new(5, 2, 20, 6)));

    // 30 rows: 10 in view
    assert_eq!(
        place(area_value(5, 20), 20, 30, area, true).map(|result| result.height),
        Some(12)
    );

    // less room than rows on both sides: the bigger side, cut
    assert_eq!(
        place(area_value(5, 8), 20, 30, Rect::new(0, 0, 80, 14), true),
        Some(Rect::new(5, 0, 20, 8))
    );

    // right edge: shifted left
    assert_eq!(
        place(area_value(75, 20), 20, 2, area, true).map(|result_value| result_value.x),
        Some(60)
    );

    // below first
    assert_eq!(
        place(Rect::new(0, 2, 20, 1), 30, 3, area, false),
        Some(Rect::new(0, 3, 30, 5))
    );
    assert_eq!(place(area_value(0, 1), 10, 3, Rect::new(0, 0, 80, 3), true), None);
}

#[test]
fn submenu_right_then_left_then_above() {
    let area = Rect::new(0, 0, 80, 30);
    let parent = Rect::new(10, 10, 20, 6);

    assert_eq!(
        place_beside(parent, 12, 30, 4, area, parent),
        Some(Rect::new(30, 11, 30, 6))
    );

    // 0 room right: left
    let parent = Rect::new(50, 10, 20, 6);

    assert_eq!(
        place_beside(parent, 12, 30, 4, area, parent),
        Some(Rect::new(20, 11, 30, 6))
    );

    // 0 room on both sides: above the open dropdowns, at the parent's column (shifted to fit)
    let parent = Rect::new(40, 10, 30, 6);
    let stack = Rect::new(5, 8, 65, 8);

    assert_eq!(
        place_beside(parent, 12, 50, 4, area, stack),
        Some(Rect::new(30, 2, 50, 6))
    );

    // less room above than rows: cut to the room
    assert_eq!(
        place_beside(parent, 12, 50, 10, Rect::new(0, 3, 80, 13), stack),
        Some(Rect::new(30, 3, 50, 5))
    );

    // 0 room above: below them
    assert_eq!(
        place_beside(parent, 12, 50, 4, Rect::new(0, 7, 80, 23), stack),
        Some(Rect::new(30, 16, 50, 6))
    );

    // bottom edge: moved up
    let area_value = Rect::new(0, 20, 10, 8);

    assert_eq!(
        place_beside(area_value, 27, 20, 10, area, area_value).map(|result| (result.y, result.height)),
        Some((18, 12))
    );
}
