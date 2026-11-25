use super::*;

#[test]
fn osc4_replies() {
    let replies = "\x1b]4;1;rgb:cdcd/3131/3131\x1b\\\x1b]4;4;rgb:24/72/c8\x07\x1b[?62;22c";

    assert_eq!(parse_osc4(replies), vec![(1, (205, 49, 49)), (4, (36, 114, 200))]);
    assert!(da1_seen(replies.as_bytes()));
}

#[test]
fn hint_keys_stand_out() {
    let spans = hints("tab: menu · ctrl+f filter · enter");
    let keys: Vec<&str> = spans
        .iter()
        .filter(|text_value| text_value.style == text())
        .map(|text_2| text_2.content.as_ref())
        .collect();

    assert_eq!(keys, ["tab", "ctrl+f", "enter"]);
    assert_eq!(
        spans.iter().map(|text_3| text_3.content.as_ref()).collect::<String>(),
        "tab: menu · ctrl+f filter · enter"
    );
}

#[test]
fn mode_badges() {
    let bytes = mode_badge("shell", true);

    assert_eq!(bytes[0].content, " SHELL ");
    assert_eq!(
        (bytes[0].style.bg, bytes[0].style.fg),
        (Some(pal().caution()), Some(pal().on_accent))
    );
    assert_eq!(mode_badge("search", false)[0].style.bg, Some(pal().surface1));
}

#[test]
fn old_color_names_still_set_a_slot() {
    let path = Palette::build(Some("coral"), [("danger", "#112233"), ("panel", "#445566")]);

    assert_eq!(
        (path.red, path.surface0),
        (Color::Rgb(0x11, 0x22, 0x33), Color::Rgb(0x44, 0x55, 0x66))
    );
    assert_eq!(Palette::build(None, []), LAVENDER);
}

#[test]
fn lavender_accent_is_no_status_hue() {
    // the default's accent stays apart from every status hue (hue distance ≥ 40°)
    let hue = |color: Color| {
        let (result, group, bytes) = rgb(color);
        let (result_value, group_value, bytes_value) = (f32::from(result), f32::from(group), f32::from(bytes));
        let (max, min) = (
            result_value.max(group_value).max(bytes_value),
            result_value.min(group_value).min(bytes_value),
        );
        let directory = max - min;

        let height = if max.partial_cmp(&result_value) == Some(std::cmp::Ordering::Equal) {
            (group_value - bytes_value) / directory % 6.0
        } else if max.partial_cmp(&group_value) == Some(std::cmp::Ordering::Equal) {
            (bytes_value - result_value) / directory + 2.0
        } else {
            (result_value - group_value) / directory + 4.0
        };

        (height * 60.0 + 360.0) % 360.0
    };

    let path = LAVENDER;

    for character in [
        path.red,
        path.peach,
        path.yellow,
        path.green,
        path.teal,
        path.sky,
        path.mauve,
    ] {
        let directory_value = (hue(path.accent) - hue(character)).abs();

        assert!(directory_value.min(360.0 - directory_value) >= 40.0, "{character:?}");
    }
}

#[test]
fn lavender_base_is_violet_gray() {
    // the default's bg: cool violet gray, the site's warm brown #140700 stays on the site
    assert_eq!(LAVENDER.base, Color::Rgb(29, 26, 40));
    assert_eq!(LAVENDER.on_accent, LAVENDER.base);
}

#[test]
fn lavender_accent_contrast() {
    // accent on base: ≥ 7:1 (WCAG AAA)
    let lum = |color: Color| lavender_accent_contrast_lin(color);

    let ratio = (lum(LAVENDER.accent) + 0.05) / (lum(LAVENDER.base) + 0.05);

    assert!(ratio >= 7.0, "{ratio}");
}

fn lavender_accent_contrast_lin(color: Color) -> f32 {
    let (result, group, bytes) = rgb(color);

    let lin = |value: u8| {
        let value_value = f32::from(value) / 255.0;

        if value_value <= 0.040_45 {
            value_value / 12.92
        } else {
            ((value_value + 0.055) / 1.055).powf(2.4)
        }
    };

    0.2126 * lin(result) + 0.7152 * lin(group) + 0.0722 * lin(bytes)
}

#[test]
fn desaturation_keeps_hue() {
    // gray stays gray; red keeps its hue, 40% of its saturation
    assert_eq!(desaturate(Color::Rgb(128, 128, 128), 0.4), Color::Rgb(128, 128, 128));

    let Color::Rgb(color, color_value, color_2) = desaturate(Color::Rgb(255, 0, 0), 0.4) else {
        panic!()
    };

    assert_eq!((color, color_value, color_2), (179, 77, 77));
}
