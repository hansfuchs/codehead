use super::*;

#[test]
fn scrolled_back_keeps_the_top_row_on_new_rows_and_folds() {
    // 3 items of 10 rows; 30 rows, 5 high, 12 back: top row 13 = item 1, row 3
    let starts = [0, 10, 20];
    let anchor = top_anchor(&starts, 30, 12, 5).unwrap();

    assert_eq!(anchor, (1, 3));

    // 8 new rows at the end: back grows by 8
    assert_eq!(back_for(&starts, 38, anchor, 5), 20);

    // item 0 opened (+6 rows): the anchor moves with item 1
    assert_eq!(back_for(&[0, 16, 26], 36, anchor, 5), 12);

    // item 1 folded to 2 rows: its last row on top
    assert_eq!(back_for(&[0, 10, 12], 22, anchor, 5), 6);

    // the anchor near the end: 0 = follow the bottom
    assert_eq!(back_for(&starts, 30, (2, 9), 5), 0);
    assert_eq!(top_anchor(&[], 0, 3, 5), None);
}
