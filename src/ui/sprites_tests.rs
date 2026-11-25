use super::*;

#[test]
fn agents_float_at_their_own_speed() {
    let (agent, bytes) = (default_for("x").desynced("gigas"), default_for("x").desynced("ada"));

    assert_ne!(agent.avatar.speed, bytes.avatar.speed);
    assert!((0.94..=1.06).contains(&agent.avatar.speed) && agent.id == default_for("x").id);
}
use unicode_width::UnicodeWidthStr;

#[test]
fn default_is_stable_regi() {
    assert_eq!(default_for("abc"), default_for("abc"));

    for key in ["a", "b", "c", "d"] {
        assert_eq!(default_for(key).avatar.cat, Cat::Regi);
    }
}

#[test]
fn old_ids_are_not_avatars() {
    assert!(from_id("pcb-042").is_none() && from_id("custom").is_none());

    let text = from_id("geo:3 speed=2").unwrap();

    assert_eq!(
        (text.avatar.cat, text.avatar.idx, text.avatar.speed),
        (Cat::Geo, 3, 2.0)
    );
    assert!(text.rows.iter().all(|result| result.width() == WIDTH));
}

#[test]
fn regi_lines_dim_faint_cells() {
    // a regi over 10 s of float: the text of the live frame, some cells in the agent color
    // dimmed (the tones of the line coverage), never brighter than the agent color
    let text = from_id("regi:36 eyes=11").unwrap();
    let red = Color::Rgb(200, 10, 10);
    let mut dimmed = 0;

    for file in 0..100 {
        assert_toned_lines(&text, red, &mut dimmed, file);
    }

    crate::domain::avatar::freeze(None);

    assert!(dimmed > 0, "0 dimmed cells in 10 s");
}

fn assert_toned_lines(text: &Sprite, red: Color, dimmed: &mut usize, file: i32) {
    let thread = file as f32 / 10.0;

    crate::domain::avatar::freeze(Some(thread));
    let lines = lines_as(text, red, Status::Working, Vec::new());
    let rows = text.avatar.rows(Status::Working, thread);

    for (line, row) in lines.iter().zip(&rows) {
        assert_dimmed_line(dimmed, thread, line, row);
    }
}

fn assert_dimmed_line(dimmed: &mut usize, thread: f32, line: &Line<'_>, row: &String) {
    assert_eq!(
        line.spans.iter().map(|path| path.content.as_ref()).collect::<String>(),
        *row,
        "t {thread}: the text"
    );

    for path_value in &line.spans {
        let Some(Color::Rgb(color, _, _)) = path_value.style.fg else {
            continue;
        };

        assert!(color <= 200, "t {thread}: brighter than the agent color");

        *dimmed += usize::from(
            color < 200
                && path_value
                    .content
                    .chars()
                    .any(|character| character != '\u{2800}' && character != ' '),
        );
    }
}
