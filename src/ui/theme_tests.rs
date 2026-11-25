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
        .filter(|span| span.style == text())
        .map(|span| span.content.as_ref())
        .collect();

    assert_eq!(keys, ["tab", "ctrl+f", "enter"]);
    assert_eq!(
        spans.iter().map(|span| span.content.as_ref()).collect::<String>(),
        "tab: menu · ctrl+f filter · enter"
    );
}

#[test]
fn mode_badges() {
    let badge = mode_badge("shell", true);

    assert_eq!(badge[0].content, " SHELL ");
    assert_eq!(
        (badge[0].style.bg, badge[0].style.fg),
        (Some(pal().caution()), Some(pal().on_accent))
    );
    assert_eq!(mode_badge("search", false)[0].style.bg, Some(pal().surface1));
}

#[test]
fn old_color_names_still_set_a_slot() {
    let palette = Palette::build(Some("coral"), [("danger", "#112233"), ("panel", "#445566")]);

    assert_eq!(
        (palette.red, palette.surface0),
        (Color::Rgb(0x11, 0x22, 0x33), Color::Rgb(0x44, 0x55, 0x66))
    );
    assert_eq!(Palette::build(None, []), LAVENDER);
}

#[test]
fn lavender_accent_is_no_status_hue() {
    // the default's accent stays apart from every status hue (hue distance ≥ 40°)
    let hue = |color: Color| {
        let (red, green, blue) = rgb(color);
        let (red, green, blue) = (f32::from(red), f32::from(green), f32::from(blue));
        let (max, min) = (red.max(green).max(blue), red.min(green).min(blue));
        let chroma = max - min;

        let sector = if max.partial_cmp(&red) == Some(std::cmp::Ordering::Equal) {
            (green - blue) / chroma % 6.0
        } else if max.partial_cmp(&green) == Some(std::cmp::Ordering::Equal) {
            (blue - red) / chroma + 2.0
        } else {
            (red - green) / chroma + 4.0
        };

        (sector * 60.0 + 360.0) % 360.0
    };

    let palette = LAVENDER;

    for color in [
        palette.red,
        palette.peach,
        palette.yellow,
        palette.green,
        palette.teal,
        palette.sky,
        palette.mauve,
    ] {
        let distance = (hue(palette.accent) - hue(color)).abs();

        assert!(distance.min(360.0 - distance) >= 40.0, "{color:?}");
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
    let (red, green, blue) = rgb(color);

    let lin = |value: u8| {
        let channel = f32::from(value) / 255.0;

        if channel <= 0.040_45 {
            channel / 12.92
        } else {
            ((channel + 0.055) / 1.055).powf(2.4)
        }
    };

    0.2126 * lin(red) + 0.7152 * lin(green) + 0.0722 * lin(blue)
}

#[test]
fn desaturation_keeps_hue() {
    // gray stays gray; red keeps its hue, 40% of its saturation
    assert_eq!(desaturate(Color::Rgb(128, 128, 128), 0.4), Color::Rgb(128, 128, 128));

    let Color::Rgb(red, green, blue) = desaturate(Color::Rgb(255, 0, 0), 0.4) else {
        panic!()
    };

    assert_eq!((red, green, blue), (179, 77, 77));
}
