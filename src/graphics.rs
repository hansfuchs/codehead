//! Images in the terminal (the files view's preview of images and pdfs): the protocol from the
//! environment or the `previews` setting (0 query: 0 startup wait, 0 bytes read from stdin), the
//! cell size from the tty; decode, scale and encode off the UI thread, 1x per (file, mtime, area).

use std::ffi::OsString;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use image::{DynamicImage, ImageDecoder};
use ratatui::layout::Size;
use ratatui_image::picker::{Picker, ProtocolType};
use ratatui_image::protocol::Protocol;
use ratatui_image::protocol::kitty::Kitty;
use ratatui_image::{FontSize, Resize};

/// Files over this size: 0 preview (a decode of them would stall the preview).
const MAX_BYTES: u64 = 50 << 20;
/// Images wider or taller than this: 0 decode (a small file can hold a huge canvas).
const MAX_SIDE: u32 = 16_384;
/// The most one decode may allocate.
const MAX_ALLOC: u64 = 256 << 20;
/// A rasterizer (sips, pdftoppm) gets this long.
const RASTER_TIMEOUT: Duration = Duration::from_secs(5);

/// What a file previews as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Decoded here: png, jpeg, gif (1st frame), webp.
    Image,
    /// Rastered by a tool first: heic (macOS sips).
    Heic,
    /// Page 1, rastered by pdftoppm or sips, on white.
    Pdf,
}

pub fn kind(p: &Path) -> Option<Kind> {
    let ext = p.extension()?.to_string_lossy().to_lowercase();
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" => Some(Kind::Image),
        "heic" | "heif" => Some(Kind::Heic),
        "pdf" => Some(Kind::Pdf),
        _ => None,
    }
}

/// The image protocol of the terminal from its environment. None: 0 images (a multiplexer, an
/// unknown terminal).
///
/// `TERM` first: each terminal sets it for its shells (VS Code, Apple Terminal reset it), while
/// window vars (`KITTY_WINDOW_ID`, `GHOSTTY_RESOURCES_DIR` …) leak into apps started from a shell.
fn protocol_from(var: impl Fn(&str) -> Option<String>) -> Option<ProtocolType> {
    let set = |k: &str| var(k).is_some_and(|v| !v.is_empty());

    // tmux, zellij, screen: passthrough unverified
    if set("TMUX") || set("ZELLIJ") || set("STY") {
        return None;
    }

    let term = var("TERM").unwrap_or_default().to_lowercase();
    let program = var("TERM_PROGRAM").unwrap_or_default().to_lowercase();

    // ghostty and terminals built on it (libghostty: TERM xterm-ghostty), kitty
    if term.contains("ghostty") || term.contains("kitty") || program == "ghostty" {
        return Some(ProtocolType::Kitty);
    }

    // rio, iTerm2, WezTerm: the iTerm2 protocol (WezTerm's kitty placeholders: broken)
    if term == "rio" || ["rio", "iterm.app", "wezterm"].contains(&program.as_str()) {
        return Some(ProtocolType::Iterm2);
    }

    // sixel terminals that say so in TERM
    if ["foot", "mlterm", "contour"].iter().any(|t| term.starts_with(t)) {
        return Some(ProtocolType::Sixel);
    }

    // over ssh 0 TERM_PROGRAM, iTerm2 still says LC_TERMINAL; a TERM_PROGRAM of another
    // terminal (vscode, Apple_Terminal): it wins over the leaked var
    if program.is_empty() && var("LC_TERMINAL").is_some_and(|t| t.contains("iTerm")) {
        return Some(ProtocolType::Iterm2);
    }

    None
}

/// The protocol: the `previews` setting (auto: from the env, 1x per process).
fn protocol() -> Option<ProtocolType> {
    static AUTO: std::sync::OnceLock<Option<ProtocolType>> = std::sync::OnceLock::new();

    match crate::config::previews().as_str() {
        "off" => None,
        "kitty" => Some(ProtocolType::Kitty),
        "iterm2" => Some(ProtocolType::Iterm2),
        "sixel" => Some(ProtocolType::Sixel),
        _ => *AUTO.get_or_init(|| protocol_from(|k| std::env::var(k).ok())),
    }
}

/// Cell size in pixels from the tty (TIOCGWINSZ); 10×20 when the tty says 0.
fn font_size() -> FontSize {
    let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
    // SAFETY: TIOCGWINSZ writes 1 winsize into WS
    let ok = unsafe { libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut ws) } == 0;

    if ok && ws.ws_col > 0 && ws.ws_row > 0 && ws.ws_xpixel > 0 && ws.ws_ypixel > 0 {
        return FontSize::new((ws.ws_xpixel / ws.ws_col).max(1), (ws.ws_ypixel / ws.ws_row).max(1));
    }

    FontSize::new(10, 20)
}

/// A picker for this terminal (its cell size now: a zoom changes it; the setting now: a change
/// applies at once). None: 0 images.
pub fn picker() -> Option<Picker> {
    if cfg!(test) {
        return None;
    }

    let proto = protocol()?;
    #[allow(deprecated)] // the query constructor reads stdin: env + ioctl here instead
    let mut p = Picker::from_fontsize(font_size());
    p.set_protocol_type(proto);
    Some(p)
}

/// A preview ready to draw: the encoded image, its line (`1920×1080 px · 812 KB`), its kitty id
/// (the terminal keeps kitty images until told to delete them) and its kitty transmit (until
/// sent).
pub struct Preview {
    pub protocol: Protocol,
    pub info: String,
    pub kitty: Option<u32>,
    /// The image data for the terminal, sent 1x straight to it (`send`), not in a cell: an
    /// overlay drawn over the image's 1st cell in that frame would drop it.
    pub transmit: Option<String>,
}

/// Path P (of KIND) encoded for PICKER to fit COLS × ROWS cells. Err: a line why not.
pub fn build(p: &Path, kind: Kind, picker: &Picker, cols: u16, rows: u16) -> Result<Preview, String> {
    if cols == 0 || rows == 0 {
        return Err("0 room for the image".into());
    }

    let bytes = std::fs::metadata(p).map_err(|e| e.to_string())?.len();
    if bytes > MAX_BYTES {
        return Err(format!("{} MB: over {} MB, 0 preview · ctrl+.: yazi", bytes >> 20, MAX_BYTES >> 20));
    }

    let fs = picker.font_size();
    let (pw, ph) = (u32::from(cols) * u32::from(fs.width), u32::from(rows) * u32::from(fs.height));
    let img = match kind {
        Kind::Image => decode(p)?,
        Kind::Heic | Kind::Pdf => raster(p, pw.max(ph))?,
    };
    let (w, h) = (img.width(), img.height());

    // 1 fast downscale to the area's pixels: the encoder gets 0 more than it draws
    let img = if w > pw || h > ph { img.thumbnail(pw, ph) } else { img };
    // pdf pages: transparent; on white (black text on a dark terminal: invisible)
    let img = if kind == Kind::Pdf { on_white(img) } else { img };

    let (protocol, kitty, transmit) = if picker.protocol_type() == ProtocolType::Kitty {
        // zlib (`o=z`, in the kitty spec): the picker turns it on only after a terminal query
        let cells = |px: u32, cell: u16, max: u16| (px.div_ceil(u32::from(cell.max(1))).min(u32::from(max)) as u16).max(1);
        let size = Size::new(cells(img.width(), fs.width, cols), cells(img.height(), fs.height, rows));
        let id = next_id();
        let protocol = Protocol::Kitty(Kitty::new(img, size, id, false, true).map_err(|e| e.to_string())?);
        let transmit = take_transmit(&protocol, size);
        (protocol, Some(id), transmit)
    } else {
        (picker.new_protocol(img, Size::new(cols, rows), Resize::Fit(None)).map_err(|e| e.to_string())?, None, None)
    };

    let what = if kind == Kind::Pdf { "page 1 · " } else { "" };
    Ok(Preview { protocol, info: format!("{what}{w}×{h} px · {} KB", bytes.div_ceil(1024)), kitty, transmit })
}

/// The kitty transmit of PROTOCOL (SIZE cells): its 1st render, into a scratch buffer, puts it
/// in cell 0 before the placeholder; later renders: placeholders only.
fn take_transmit(protocol: &Protocol, size: Size) -> Option<String> {
    use ratatui::widgets::Widget;

    let area = ratatui::layout::Rect::new(0, 0, size.width, size.height);
    let mut scratch = ratatui::buffer::Buffer::empty(area);
    ratatui_image::Image::new(protocol).render(area, &mut scratch);
    let first = scratch.content().first()?.symbol();
    first.rfind("\x1b\\").map(|i| first[..i + 2].to_string())
}

/// SEQ (a kitty transmit, a delete) straight to the terminal, between ratatui's cell writes.
pub fn send(seq: &str) {
    if cfg!(test) {
        return;
    }

    let mut out = std::io::stdout();
    let _ = out.write_all(seq.as_bytes());
    let _ = out.flush();
}

/// P decoded within the limits (side, allocation), turned as its EXIF orientation says (phone
/// photos).
fn decode(p: &Path) -> Result<DynamicImage, String> {
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    limits.max_alloc = Some(MAX_ALLOC);

    let mut reader = image::ImageReader::open(p).and_then(|r| r.with_guessed_format()).map_err(|e| e.to_string())?;
    reader.limits(limits.clone());
    let mut decoder = reader.into_decoder().map_err(|e| e.to_string())?;
    // the pixel buffer counts too (from_decoder reserves 0 of it): 16384² RGBA = 1 GiB
    limits.reserve(decoder.total_bytes()).map_err(|e| e.to_string())?;
    let turn = decoder.orientation().map_err(|e| e.to_string())?;
    let mut img = DynamicImage::from_decoder(decoder).map_err(|e| e.to_string())?;
    img.apply_orientation(turn);
    Ok(img)
}

/// A kitty image id: unique in the process, never 0 (its low 24 bits: the placeholders' color).
fn next_id() -> u32 {
    static N: AtomicU32 = AtomicU32::new(0);
    N.fetch_add(1, Ordering::Relaxed).wrapping_mul(2_654_435_761) | 1
}

/// Kitty image ID out of the terminal's store (its data and placements).
pub fn delete_kitty(id: u32) {
    send(&format!("\x1b_Ga=d,d=I,i={id},q=2\x1b\\"));
}

fn on_white(img: DynamicImage) -> DynamicImage {
    let mut rgba = img.into_rgba8();
    for px in rgba.pixels_mut() {
        let a = u16::from(px[3]);
        for c in 0..3 {
            px[c] = ((u16::from(px[c]) * a + 255 * (255 - a)) / 255) as u8;
        }
        px[3] = 255;
    }
    DynamicImage::ImageRgba8(rgba)
}

fn on_path(bin: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(bin).is_file()))
}

/// The rasterizers' dir: private (0700) under codehead's cache, made 1x. A tool never writes
/// through a link someone put in a shared /tmp.
fn raster_dir() -> Result<PathBuf, String> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};

    let dir = crate::config::home().join(".cache/codehead/raster");
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(&dir).map_err(|e| e.to_string())?;
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    Ok(dir)
}

/// P (pdf page 1, heic) as an image ≤ MAX px: pdftoppm (pdf), else sips (macOS); a temp png,
/// read, deleted.
fn raster(p: &Path, max: u32) -> Result<DynamicImage, String> {
    static N: AtomicU64 = AtomicU64::new(0);

    // an absolute path only: a leading `-` never reads as a tool's option
    if !p.is_absolute() {
        return Err("0 preview: a relative path".into());
    }

    let is_pdf = p.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf"));
    let pdftoppm = is_pdf && on_path("pdftoppm");
    if !pdftoppm && !cfg!(target_os = "macos") {
        return Err(if is_pdf { "0 pdf rasterizer: install poppler (pdftoppm) · ctrl+.: yazi".into() } else { "heic: 0 decoder here · ctrl+.: yazi".into() });
    }

    // 1 name per call: 2 rasters at once never share a file
    let stem = raster_dir()?.join(format!("{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    let out = stem.with_extension("png");
    let max = max.clamp(64, 4096).to_string();
    let argv: Vec<OsString> = if pdftoppm {
        let mut a: Vec<OsString> = ["pdftoppm", "-png", "-f", "1", "-l", "1", "-singlefile", "-scale-to", &max].iter().map(OsString::from).collect();
        a.extend([p.as_os_str().to_owned(), stem.into_os_string()]);
        a
    } else {
        let mut a: Vec<OsString> = ["sips", "-s", "format", "png", "-Z", &max].iter().map(OsString::from).collect();
        a.extend([p.as_os_str().to_owned(), "--out".into(), out.as_os_str().to_owned()]);
        a
    };

    let img = if run(&argv, RASTER_TIMEOUT) { decode(&out) } else { Err(format!("{} failed", argv[0].to_string_lossy())) };
    let _ = std::fs::remove_file(&out);
    img
}

/// Run ARGV (0 shell), at most TIMEOUT. True: exit 0.
fn run(argv: &[OsString], timeout: Duration) -> bool {
    let Ok(mut child) = Command::new(&argv[0]).args(&argv[1..]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() else {
        return false;
    };

    let end = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(s)) => return s.success(),
            Ok(None) if Instant::now() < end => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picker_of(proto: ProtocolType) -> Picker {
        #[allow(deprecated)]
        let mut p = Picker::from_fontsize(FontSize::new(10, 20));
        p.set_protocol_type(proto);
        p
    }

    /// A png of W×H RGBA8 with 0 pixel data: signature, IHDR, an empty IDAT (the decoder reads
    /// the header up to it), IEND.
    fn png_header(w: u32, h: u32) -> Vec<u8> {
        fn crc32(bytes: &[u8]) -> u32 {
            let mut c = !0u32;
            for b in bytes {
                c ^= u32::from(*b);
                for _ in 0..8 {
                    c = if c & 1 == 1 { (c >> 1) ^ 0xEDB8_8320 } else { c >> 1 };
                }
            }
            !c
        }

        let chunk = |kind: &[u8], data: &[u8]| {
            let body = [kind, data].concat();
            [&(data.len() as u32).to_be_bytes()[..], &body, &crc32(&body).to_be_bytes()].concat()
        };

        // 8 bits, RGBA, deflate, 0 filter, 0 interlace
        let ihdr = [&w.to_be_bytes()[..], &h.to_be_bytes(), &[8, 6, 0, 0, 0]].concat();

        [&b"\x89PNG\r\n\x1a\n"[..], &chunk(b"IHDR", &ihdr), &chunk(b"IDAT", &[]), &chunk(b"IEND", &[])].concat()
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
        let env = |vars: &'static [(&'static str, &'static str)]| move |k: &str| vars.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string());

        assert_eq!(protocol_from(env(&[("TERM_PROGRAM", "ghostty")])), Some(ProtocolType::Kitty));
        assert_eq!(protocol_from(env(&[("TERM", "xterm-ghostty"), ("TERM_PROGRAM", "Rex")])), Some(ProtocolType::Kitty));
        assert_eq!(protocol_from(env(&[("TERM", "xterm-kitty")])), Some(ProtocolType::Kitty));
        assert_eq!(protocol_from(env(&[("TERM", "rio"), ("TERM_PROGRAM", "rio")])), Some(ProtocolType::Iterm2));
        assert_eq!(protocol_from(env(&[("TERM_PROGRAM", "iTerm.app")])), Some(ProtocolType::Iterm2));
        assert_eq!(protocol_from(env(&[("TERM_PROGRAM", "WezTerm")])), Some(ProtocolType::Iterm2));
        assert_eq!(protocol_from(env(&[("TERM", "foot")])), Some(ProtocolType::Sixel));
        assert_eq!(protocol_from(env(&[("LC_TERMINAL", "iTerm2")])), Some(ProtocolType::Iterm2), "ssh from iTerm2");

        // 0 images: another terminal, a leaked window var, a multiplexer
        assert_eq!(protocol_from(env(&[("TERM", "xterm-256color"), ("TERM_PROGRAM", "Apple_Terminal")])), None);
        assert_eq!(protocol_from(env(&[("TERM", "xterm-256color"), ("TERM_PROGRAM", "vscode"), ("GHOSTTY_RESOURCES_DIR", "/x"), ("KITTY_WINDOW_ID", "1")])), None);
        assert_eq!(protocol_from(env(&[("TERM", "xterm-256color"), ("TERM_PROGRAM", "vscode"), ("LC_TERMINAL", "iTerm2")])), None);
        assert_eq!(protocol_from(env(&[("TERM_PROGRAM", "ghostty"), ("TMUX", "/tmp/tmux-1/default,1,0")])), None);
        assert_eq!(protocol_from(env(&[("TERM", "xterm-ghostty"), ("ZELLIJ", "0")])), None);
        assert_eq!(picker().map(|p| p.protocol_type()), None, "tests: 0 images");
    }

    #[test]
    fn a_big_image_scales_down_to_the_area_before_the_encode() {
        let dir = temp_dir("scale");
        let p = dir.join("wide.png");
        image::RgbaImage::from_pixel(2000, 1000, image::Rgba([200, 10, 10, 255])).save(&p).unwrap();

        let pre = build(&p, Kind::Image, &picker_of(ProtocolType::Iterm2), 40, 20).unwrap();

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
        image::RgbaImage::from_pixel(8, 8, image::Rgba([0, 0, 0, 255])).save(&ok).unwrap();
        // over 50 MB: a sparse file, 0 bytes written
        let big = dir.join("big.png");
        std::fs::File::create(&big).unwrap().set_len(MAX_BYTES + 1).unwrap();

        assert!(build(&bad, Kind::Image, &picker, 40, 20).is_err());
        assert!(build(&big, Kind::Image, &picker, 40, 20).err().is_some_and(|e| e.contains("over 50 MB")));
        assert!(build(&ok, Kind::Image, &picker, 0, 20).is_err());
        assert!(build(&ok, Kind::Image, &picker, 40, 0).is_err());
        assert!(build(&ok, Kind::Image, &picker, 1, 1).is_ok(), "1 cell: room for 1 cell");
        assert!(raster(Path::new("-rf.pdf"), 100).unwrap_err().contains("relative"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_huge_canvas_in_a_small_file_never_decodes() {
        let dir = temp_dir("canvas");
        let p = dir.join("tall.png");
        // 1×20000: 80 KB of pixels, over the side limit
        image::GrayImage::from_pixel(1, MAX_SIDE + 1, image::Luma([0])).save(&p).unwrap();
        // 16384×16384 RGBA: inside the side limit, 1 GiB of pixels; only the header (IHDR) is
        // needed: the limit stops the decode before the pixel data
        let big = dir.join("square.png");
        std::fs::write(&big, png_header(MAX_SIDE, MAX_SIDE)).unwrap();

        assert!(decode(&p).is_err());
        let e = decode(&big).err();
        assert!(e.as_deref().is_some_and(|e| e.to_lowercase().contains("limit")), "over MAX_ALLOC: {e:?}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_tool_that_fails_hangs_or_is_missing_counts_as_failed() {
        let argv = |a: &[&str]| a.iter().map(OsString::from).collect::<Vec<_>>();

        assert!(run(&argv(&["true"]), Duration::from_secs(5)));
        assert!(!run(&argv(&["false"]), Duration::from_secs(5)));
        assert!(!run(&argv(&["/nonexistent/codehead-tool"]), Duration::from_secs(5)));

        let t = Instant::now();
        assert!(!run(&argv(&["sleep", "5"]), Duration::from_millis(100)));
        assert!(t.elapsed() < Duration::from_secs(2), "killed at the timeout");
    }

    #[test]
    fn kitty_images_go_out_compressed_once_inside_the_area() {
        use ratatui::widgets::Widget;

        let dir = temp_dir("kitty");
        let p = dir.join("flat.png");
        image::RgbaImage::from_pixel(1600, 800, image::Rgba([30, 30, 200, 255])).save(&p).unwrap();
        let pre = build(&p, Kind::Image, &picker_of(ProtocolType::Kitty), 40, 20).unwrap();
        let area = ratatui::layout::Rect::new(0, 0, 40, 20);
        let draw = || {
            let mut buf = ratatui::buffer::Buffer::empty(area);
            ratatui_image::Image::new(&pre.protocol).render(area, &mut buf);
            buf.content().iter().map(|c| c.symbol().to_string()).collect::<String>()
        };

        // 1600×800 into 400×400 px: 400×200 px = 40×10 cells
        assert_eq!(pre.protocol.size(), Size::new(40, 10));
        let id = pre.kitty.unwrap();

        // the transmit: out of the cells, zlib, a few KB for a flat 400×200 image (raw: 320 KB)
        let t = pre.transmit.as_deref().unwrap();
        assert!(t.starts_with("\x1b_G") && t.ends_with("\x1b\\") && t.contains("o=z") && t.contains(&format!("i={id},a=T")), "{}", &t[..60]);
        assert!(!t.contains('\u{10EEEE}'));
        assert!(t.len() < 40_000, "{} bytes", t.len());

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
}
