use super::*;

#[test]
fn agents_float_at_their_own_speed() {
    let (gigas, ada) = (default_for("x").desynced("gigas"), default_for("x").desynced("ada"));

    assert_ne!(gigas.avatar.speed, ada.avatar.speed);
    assert!((0.94..=1.06).contains(&gigas.avatar.speed) && gigas.id == default_for("x").id);
}
use unicode_width::UnicodeWidthStr;

#[test]
fn default_is_stable_regi() {
    assert_eq!(default_for("abc"), default_for("abc"));

    for name in ["a", "b", "c", "d"] {
        assert_eq!(default_for(name).avatar.cat, Cat::Regi);
    }
}

#[test]
fn old_ids_are_not_avatars() {
    assert!(from_id("pcb-042").is_none() && from_id("custom").is_none());

    let sprite = from_id("geo:3 speed=2").unwrap();

    assert_eq!(
        (sprite.avatar.cat, sprite.avatar.idx, sprite.avatar.speed),
        (Cat::Geo, 3, 2.0)
    );
    assert!(sprite.rows.iter().all(|row| row.width() == WIDTH));
}

#[test]
fn regi_lines_dim_faint_cells() {
    // a regi over 10 s of float: the text of the live frame, some cells in the agent color
    // dimmed (the tones of the line coverage), never brighter than the agent color
    let sprite = from_id("regi:36 eyes=11").unwrap();
    let red = Color::Rgb(200, 10, 10);
    let mut dimmed = 0;

    for step in 0..100 {
        assert_toned_lines(&sprite, red, &mut dimmed, step);
    }

    crate::domain::avatar::freeze(None);

    assert!(dimmed > 0, "0 dimmed cells in 10 s");
}

fn assert_toned_lines(sprite: &Sprite, red: Color, dimmed: &mut usize, step: i32) {
    let secs = step as f32 / 10.0;

    crate::domain::avatar::freeze(Some(secs));
    let lines = lines_as(sprite, red, Status::Working, Vec::new());
    let rows = sprite.avatar.rows(Status::Working, secs);

    for (line, row) in lines.iter().zip(&rows) {
        assert_dimmed_line(dimmed, secs, line, row);
    }
}

fn assert_dimmed_line(dimmed: &mut usize, secs: f32, line: &Line<'_>, row: &String) {
    assert_eq!(
        line.spans.iter().map(|span| span.content.as_ref()).collect::<String>(),
        *row,
        "t {secs}: the text"
    );

    for span in &line.spans {
        let Some(Color::Rgb(red_level, _, _)) = span.style.fg else {
            continue;
        };

        assert!(red_level <= 200, "t {secs}: brighter than the agent color");

        *dimmed += usize::from(red_level < 200 && span.content.chars().any(|ch| ch != '\u{2800}' && ch != ' '));
    }
}
