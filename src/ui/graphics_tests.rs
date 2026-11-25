fn environment(vars: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
    move |key| {
        vars.iter()
            .find(|(name, _)| *name == key)
            .map(|(_, value)| value.to_string())
    }
}

use super::*;

fn picker_of(proto: ProtocolType) -> Picker {
    Picker::new(FontSize::new(10, 20), proto)
}

/// A png of W×H RGBA8 with 0 pixel data: signature, IHDR, an empty IDAT (the decoder reads
/// the header up to it), IEND.
fn png_header(width: u32, height: u32) -> Vec<u8> {
    fn crc32(bytes: &[u8]) -> u32 {
        let mut crc = !0u32;

        for byte in bytes {
            crc32_byte(&mut crc, *byte);
        }

        !crc
    }

    let chunk = |kind: &[u8], data: &[u8]| {
        let body = [kind, data].concat();

        [
            &(data.len() as u32).to_be_bytes()[..],
            &body,
            &crc32(&body).to_be_bytes(),
        ]
        .concat()
    };

    // 8 bits, RGBA, deflate, 0 filter, 0 interlace
    let ihdr = [&width.to_be_bytes()[..], &height.to_be_bytes(), &[8, 6, 0, 0, 0]].concat();

    [
        &b"\x89PNG\r\n\x1a\n"[..],
        &chunk(b"IHDR", &ihdr),
        &chunk(b"IDAT", &[]),
        &chunk(b"IEND", &[]),
    ]
    .concat()
}

fn crc32_byte(crc: &mut u32, byte: u8) {
    *crc ^= u32::from(byte);

    for _ in 0..8 {
        *crc = if *crc & 1 == 1 {
            (*crc >> 1) ^ 0xEDB8_8320
        } else {
            *crc >> 1
        };
    }
}

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("codehead-graphics-{tag}-{}", std::process::id()));

    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn kinds_by_extension() {
    assert_eq!(kind(Path::new("/a/B.PNG")), Some(Kind::Image));
    assert_eq!(kind(Path::new("a.jpeg")), Some(Kind::Image));
    assert_eq!(kind(Path::new("a.webp")), Some(Kind::Image));
    assert_eq!(kind(Path::new("a.heic")), Some(Kind::Heic));
    assert_eq!(kind(Path::new("a.pdf")), Some(Kind::Pdf));
    assert_eq!(kind(Path::new("a.svg")), None);
    assert_eq!(kind(Path::new("notes.md")), None);
    assert_eq!(kind(Path::new("Makefile")), None);
}

#[test]
fn the_protocol_comes_from_the_terminals_environment() {
    assert_eq!(
        protocol_from(environment(&[("TERM_PROGRAM", "ghostty")])),
        Some(ProtocolType::Kitty)
    );
    assert_eq!(
        protocol_from(environment(&[("TERM", "xterm-ghostty"), ("TERM_PROGRAM", "Rex")])),
        Some(ProtocolType::Kitty)
    );
    assert_eq!(
        protocol_from(environment(&[("TERM", "xterm-kitty")])),
        Some(ProtocolType::Kitty)
    );
    assert_eq!(
        protocol_from(environment(&[("TERM", "rio"), ("TERM_PROGRAM", "rio")])),
        Some(ProtocolType::Iterm2)
    );
    assert_eq!(
        protocol_from(environment(&[("TERM_PROGRAM", "iTerm.app")])),
        Some(ProtocolType::Iterm2)
    );
    assert_eq!(
        protocol_from(environment(&[("TERM_PROGRAM", "WezTerm")])),
        Some(ProtocolType::Iterm2)
    );

    check_terminal_protocols();
}

fn check_terminal_protocols() {
    assert_eq!(
        protocol_from(environment(&[("TERM", "foot")])),
        Some(ProtocolType::Sixel)
    );
    assert_eq!(
        protocol_from(environment(&[("LC_TERMINAL", "iTerm2")])),
        Some(ProtocolType::Iterm2),
        "ssh from iTerm2"
    );

    // 0 images: another terminal, a leaked window var, a multiplexer
    assert_eq!(
        protocol_from(environment(&[
            ("TERM", "xterm-256color"),
            ("TERM_PROGRAM", "Apple_Terminal")
        ])),
        None
    );
    assert_eq!(
        protocol_from(environment(&[
            ("TERM", "xterm-256color"),
            ("TERM_PROGRAM", "vscode"),
            ("GHOSTTY_RESOURCES_DIR", "/x"),
            ("KITTY_WINDOW_ID", "1")
        ])),
        None
    );
    assert_eq!(
        protocol_from(environment(&[
            ("TERM", "xterm-256color"),
            ("TERM_PROGRAM", "vscode"),
            ("LC_TERMINAL", "iTerm2")
        ])),
        None
    );
    assert_eq!(
        protocol_from(environment(&[
            ("TERM_PROGRAM", "ghostty"),
            ("TMUX", "/tmp/tmux-1/default,1,0")
        ])),
        None
    );
    assert_eq!(
        protocol_from(environment(&[("TERM", "xterm-ghostty"), ("ZELLIJ", "0")])),
        None
    );
    assert_eq!(picker().map(|picker| picker.protocol_type()), None, "tests: 0 images");
}

#[test]
fn a_big_image_scales_down_to_the_area_before_the_encode() {
    let dir = temp_dir("scale");
    let path = dir.join("wide.png");

    image::RgbaImage::from_pixel(2000, 1000, image::Rgba([200, 10, 10, 255]))
        .save(&path)
        .unwrap();

    let pre = build(&path, Kind::Image, &picker_of(ProtocolType::Iterm2), 40, 20).unwrap();

    // 40×20 cells of 10×20 px: 400 px wide at most
    assert!(pre.info.starts_with("2000×1000 px · "), "{}", pre.info);

    let area = pre.protocol.size();

    assert!(area.width <= 40 && area.height <= 20 && area.width > 0, "{area:?}");
    assert_eq!(pre.kitty, None);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_broken_huge_or_roomless_file_gets_a_line_not_a_panic() {
    let dir = temp_dir("bad");
    let picker = picker_of(ProtocolType::Kitty);
    let bad = dir.join("bad.png");

    std::fs::write(&bad, b"not a png").unwrap();
    let ok = dir.join("ok.png");

    image::RgbaImage::from_pixel(8, 8, image::Rgba([0, 0, 0, 255]))
        .save(&ok)
        .unwrap();

    // over 50 MB: a sparse file, 0 bytes written
    let big = dir.join("big.png");

    std::fs::File::create(&big).unwrap().set_len(MAX_BYTES + 1).unwrap();

    assert!(build(&bad, Kind::Image, &picker, 40, 20).is_err());
    assert!(
        build(&big, Kind::Image, &picker, 40, 20)
            .err()
            .is_some_and(|message| message.contains("over 50 MB"))
    );
    assert!(build(&ok, Kind::Image, &picker, 0, 20).is_err());
    assert!(build(&ok, Kind::Image, &picker, 40, 0).is_err());
    assert!(
        build(&ok, Kind::Image, &picker, 1, 1).is_ok(),
        "1 cell: room for 1 cell"
    );
    assert!(raster(Path::new("-rf.pdf"), 100).unwrap_err().contains("relative"));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_huge_canvas_in_a_small_file_never_decodes() {
    let dir = temp_dir("canvas");
    let path = dir.join("tall.png");

    // 1×20000: 80 KB of pixels, over the side limit
    image::GrayImage::from_pixel(1, MAX_SIDE + 1, image::Luma([0]))
        .save(&path)
        .unwrap();

    // 16384×16384 RGBA: inside the side limit, 1 GiB of pixels; only the header (IHDR) is
    // needed: the limit stops the decode before the pixel data
    let big = dir.join("square.png");

    std::fs::write(&big, png_header(MAX_SIDE, MAX_SIDE)).unwrap();

    assert!(decode(&path).is_err());

    let error = decode(&big).err();

    assert!(
        error
            .as_deref()
            .is_some_and(|message| message.to_lowercase().contains("limit")),
        "over MAX_ALLOC: {error:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_tool_that_fails_hangs_or_is_missing_counts_as_failed() {
    let argv = |args: &[&str]| args.iter().map(OsString::from).collect::<Vec<_>>();

    assert!(run(&argv(&["true"]), Duration::from_secs(5)));
    assert!(!run(&argv(&["false"]), Duration::from_secs(5)));
    assert!(!run(&argv(&["/nonexistent/codehead-tool"]), Duration::from_secs(5)));

    let started = Instant::now();

    assert!(!run(&argv(&["sleep", "5"]), Duration::from_millis(100)));
    assert!(started.elapsed() < Duration::from_secs(2), "killed at the timeout");
}

#[test]
fn kitty_images_go_out_compressed_once_inside_the_area() {
    use ratatui::widgets::Widget;

    let dir = temp_dir("kitty");
    let path = dir.join("flat.png");

    image::RgbaImage::from_pixel(1600, 800, image::Rgba([30, 30, 200, 255]))
        .save(&path)
        .unwrap();
    let pre = build(&path, Kind::Image, &picker_of(ProtocolType::Kitty), 40, 20).unwrap();
    let area = ratatui::layout::Rect::new(0, 0, 40, 20);

    let draw = || {
        let mut buf = ratatui::buffer::Buffer::empty(area);

        ratatui_image::Image::new(&pre.protocol).render(area, &mut buf);
        buf.content()
            .iter()
            .map(|cell| cell.symbol().to_string())
            .collect::<String>()
    };

    // 1600×800 into 400×400 px: 400×200 px = 40×10 cells
    assert_eq!(pre.protocol.size(), Size::new(40, 10));

    let id = pre.kitty.unwrap();

    // the transmit: out of the cells, zlib, a few KB for a flat 400×200 image (raw: 320 KB)
    let transmit = pre.transmit.as_deref().unwrap();

    assert!(
        transmit.starts_with("\x1b_G")
            && transmit.ends_with("\x1b\\")
            && transmit.contains("o=z")
            && transmit.contains(&format!("i={id},a=T")),
        "{}",
        &transmit[..60]
    );
    assert!(!transmit.contains('\u{10EEEE}'));
    assert!(transmit.len() < 40_000, "{} bytes", transmit.len());

    // every draw: placeholders only (an overlay over cell 0 drops 0 image data)
    for _ in 0..2 {
        let cells = draw();

        assert!(!cells.contains("a=T") && cells.contains('\u{10EEEE}'));
    }

    assert_ne!(next_id(), next_id());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_transparent_pdf_page_gets_a_white_ground() {
    let img = DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(1, 1, image::Rgba([0, 0, 0, 0])));
    let ink = DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(1, 1, image::Rgba([0, 0, 0, 255])));

    assert_eq!(on_white(img).to_rgba8().get_pixel(0, 0).0, [255, 255, 255, 255]);
    assert_eq!(on_white(ink).to_rgba8().get_pixel(0, 0).0, [0, 0, 0, 255]);
}
