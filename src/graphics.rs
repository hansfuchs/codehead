//! Images in the terminal (the files view's preview of images and pdfs): the protocol from the
//! environment (0 query: 0 startup wait, 0 bytes read from stdin), the cell size from the tty;
//! decode, scale and encode off the UI thread, 1x per (file, mtime, area).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use image::DynamicImage;
use ratatui::layout::Size;
use ratatui_image::picker::{Picker, ProtocolType};
use ratatui_image::protocol::Protocol;
use ratatui_image::protocol::kitty::Kitty;
use ratatui_image::{FontSize, Resize};

/// Files over this size: 0 preview (a decode of them would stall the preview).
const MAX_BYTES: u64 = 50 << 20;
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

/// The image protocol of the terminal from its environment. None: 0 images (tmux, unknown
/// terminals, the `previews = off` setting).
fn protocol_from(var: impl Fn(&str) -> Option<String>) -> Option<ProtocolType> {
    // tmux: passthrough unverified
    if var("TMUX").is_some_and(|v| !v.is_empty()) {
        return None;
    }
    let term = var("TERM").unwrap_or_default().to_lowercase();
    let program = var("TERM_PROGRAM").unwrap_or_default().to_lowercase();
    // ghostty and terminals built on it (libghostty: TERM xterm-ghostty), kitty
    if program == "ghostty" || term.contains("ghostty") || term.contains("kitty") || var("KITTY_WINDOW_ID").is_some() || var("GHOSTTY_RESOURCES_DIR").is_some() {
        return Some(ProtocolType::Kitty);
    }
    // rio, iTerm2, WezTerm: the iTerm2 protocol (WezTerm's kitty placeholders: broken)
    if ["rio", "iterm.app", "wezterm"].contains(&program.as_str()) || term == "rio" || var("LC_TERMINAL").is_some_and(|t| t.contains("iTerm")) || var("WEZTERM_EXECUTABLE").is_some() {
        return Some(ProtocolType::Iterm2);
    }
    // sixel terminals that say so in TERM
    if ["foot", "mlterm", "contour"].iter().any(|t| term.starts_with(t)) {
        return Some(ProtocolType::Sixel);
    }
    None
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

/// The protocol, once per process (env vars stay).
fn protocol() -> Option<ProtocolType> {
    static P: std::sync::OnceLock<Option<ProtocolType>> = std::sync::OnceLock::new();
    *P.get_or_init(|| if crate::config::previews() { protocol_from(|k| std::env::var(k).ok()) } else { None })
}

/// A picker for this terminal (its cell size now: a zoom changes it). None: 0 images.
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

/// A preview ready to draw: the encoded image, its line (`1920×1080 px · 812 KB`).
pub struct Preview {
    pub protocol: Protocol,
    pub info: String,
}

/// Path P (of KIND) encoded for PICKER to fit COLS × ROWS cells. Err: a line why not.
pub fn build(p: &Path, kind: Kind, picker: &Picker, cols: u16, rows: u16) -> Result<Preview, String> {
    let bytes = std::fs::metadata(p).map_err(|e| e.to_string())?.len();
    if bytes > MAX_BYTES {
        return Err(format!("{} MB: over {} MB, 0 preview · y: yazi", bytes >> 20, MAX_BYTES >> 20));
    }
    let fs = picker.font_size();
    let (pw, ph) = (u32::from(cols) * u32::from(fs.width), u32::from(rows) * u32::from(fs.height));
    let img = match kind {
        Kind::Image => image::ImageReader::open(p).and_then(|r| r.with_guessed_format()).map_err(|e| e.to_string())?.decode().map_err(|e| e.to_string())?,
        Kind::Heic | Kind::Pdf => raster(p, pw.max(ph))?,
    };
    let (w, h) = (img.width(), img.height());
    // 1 fast downscale to the area's pixels: the encoder gets 0 more than it draws
    let img = if w > pw || h > ph { img.thumbnail(pw, ph) } else { img };
    // pdf pages: transparent; on white (black text on a dark terminal: invisible)
    let img = if kind == Kind::Pdf { on_white(img) } else { img };
    let protocol = if picker.protocol_type() == ProtocolType::Kitty {
        // zlib (`o=z`, in the kitty spec): the picker turns it on only after a terminal query
        let size = Size::new((img.width().div_ceil(u32::from(fs.width)) as u16).clamp(1, cols), (img.height().div_ceil(u32::from(fs.height)) as u16).clamp(1, rows));
        Protocol::Kitty(Kitty::new(img, size, next_id(), false, true).map_err(|e| e.to_string())?)
    } else {
        picker.new_protocol(img, Size::new(cols, rows), Resize::Fit(None)).map_err(|e| e.to_string())?
    };
    let what = match kind {
        Kind::Pdf => "page 1 · ",
        _ => "",
    };
    Ok(Preview { protocol, info: format!("{what}{w}×{h} px · {} KB", bytes.div_ceil(1024)) })
}

/// A kitty image id: unique in the process, never 0 (its low 24 bits: the placeholders' color).
fn next_id() -> u32 {
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    // pid in the high bits: 2 codeheads in 1 terminal get other ids
    ((std::process::id() << 16) ^ n.wrapping_mul(2_654_435_761)) | 1
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

/// P (pdf page 1, heic) as an image ≤ MAX px: pdftoppm (pdf), else sips (macOS); a temp png,
/// read, deleted.
fn raster(p: &Path, max: u32) -> Result<DynamicImage, String> {
    let max = max.clamp(64, 4096).to_string();
    let tmp = std::env::temp_dir().join(format!("codehead-raster-{}-{}", std::process::id(), crate::engine::now_ms()));
    let is_pdf = p.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf"));
    let (cmd, out): (Vec<String>, PathBuf) = if is_pdf && on_path("pdftoppm") {
        let out = tmp.with_extension("png");
        (["pdftoppm", "-png", "-f", "1", "-l", "1", "-singlefile", "-scale-to", &max].iter().map(|s| s.to_string()).chain([p.display().to_string(), tmp.display().to_string()]).collect(), out)
    } else if cfg!(target_os = "macos") {
        let out = tmp.with_extension("png");
        (["sips", "-s", "format", "png", "-Z", &max].iter().map(|s| s.to_string()).chain([p.display().to_string(), "--out".into(), out.display().to_string()]).collect(), out)
    } else {
        return Err(if is_pdf { "0 pdf rasterizer: install poppler (pdftoppm) · y: yazi".into() } else { "heic: 0 decoder here · y: yazi".into() });
    };
    let ok = run(&cmd);
    let img = if ok { image::open(&out).map_err(|e| e.to_string()) } else { Err(format!("{} failed", cmd[0])) };
    let _ = std::fs::remove_file(&out);
    img
}

/// Run CMD (argv, 0 shell), at most RASTER_TIMEOUT. True: exit 0.
fn run(cmd: &[String]) -> bool {
    let Ok(mut child) = Command::new(&cmd[0]).args(&cmd[1..]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() else { return false };
    let end = Instant::now() + RASTER_TIMEOUT;
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
        assert_eq!(protocol_from(env(&[("TERM", "xterm-ghostty")])), Some(ProtocolType::Kitty));
        assert_eq!(protocol_from(env(&[("TERM", "xterm-kitty")])), Some(ProtocolType::Kitty));
        assert_eq!(protocol_from(env(&[("TERM", "rio"), ("TERM_PROGRAM", "rio")])), Some(ProtocolType::Iterm2));
        assert_eq!(protocol_from(env(&[("TERM_PROGRAM", "iTerm.app")])), Some(ProtocolType::Iterm2));
        assert_eq!(protocol_from(env(&[("TERM_PROGRAM", "WezTerm")])), Some(ProtocolType::Iterm2));
        assert_eq!(protocol_from(env(&[("TERM", "foot")])), Some(ProtocolType::Sixel));
        assert_eq!(protocol_from(env(&[("TERM", "xterm-256color"), ("TERM_PROGRAM", "Apple_Terminal")])), None);
        assert_eq!(protocol_from(env(&[("TERM_PROGRAM", "ghostty"), ("TMUX", "/tmp/tmux-1/default,1,0")])), None);
        assert_eq!(picker().map(|p| p.protocol_type()), None, "tests: 0 images");
    }

    #[test]
    fn a_big_image_scales_down_to_the_area_before_the_encode() {
        let dir = std::env::temp_dir().join(format!("codehead-graphics-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("wide.png");
        image::RgbaImage::from_pixel(2000, 1000, image::Rgba([200, 10, 10, 255])).save(&p).unwrap();
        #[allow(deprecated)]
        let mut picker = Picker::from_fontsize(FontSize::new(10, 20));
        picker.set_protocol_type(ProtocolType::Iterm2);
        let pre = build(&p, Kind::Image, &picker, 40, 20).unwrap();
        assert!(pre.info.starts_with("2000×1000 px · "), "{}", pre.info);
        // 40×20 cells of 10×20 px: 400 px wide at most, 2:1 → 20 cols × … rows fit inside
        let area = pre.protocol.size();
        assert!(area.width <= 40 && area.height <= 20 && area.width > 0, "{area:?}");
        // a broken file: a line why
        let bad = dir.join("bad.png");
        std::fs::write(&bad, b"not a png").unwrap();
        assert!(build(&bad, Kind::Image, &picker, 40, 20).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn kitty_images_go_out_compressed_once_inside_the_area() {
        use ratatui::widgets::Widget;
        let dir = std::env::temp_dir().join(format!("codehead-graphics-kitty-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("flat.png");
        image::RgbaImage::from_pixel(1600, 800, image::Rgba([30, 30, 200, 255])).save(&p).unwrap();
        #[allow(deprecated)]
        let mut picker = Picker::from_fontsize(FontSize::new(10, 20));
        picker.set_protocol_type(ProtocolType::Kitty);
        let pre = build(&p, Kind::Image, &picker, 40, 20).unwrap();
        // 1600×800 into 400×400 px: 400×200 px = 40×10 cells
        assert_eq!(pre.protocol.size(), Size::new(40, 10));
        let area = ratatui::layout::Rect::new(0, 0, 40, 20);
        let draw = || {
            let mut buf = ratatui::buffer::Buffer::empty(area);
            ratatui_image::Image::new(&pre.protocol).render(area, &mut buf);
            buf.content().iter().map(|c| c.symbol().to_string()).collect::<String>()
        };
        let first = draw();
        assert!(first.contains("o=z") && first.contains("a=T"), "zlib transmit");
        // a flat 400×200 image: a few KB, not the raw 320 KB
        assert!(first.len() < 40_000, "{} bytes", first.len());
        // the next draw: placeholders only, 0 transmit
        let next = draw();
        assert!(!next.contains("a=T") && next.contains('\u{10EEEE}'));
        assert_ne!(next_id(), next_id());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_transparent_pdf_page_gets_a_white_ground() {
        let img = DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(1, 1, image::Rgba([0, 0, 0, 0])));
        assert_eq!(on_white(img).to_rgba8().get_pixel(0, 0).0, [255, 255, 255, 255]);
        let ink = DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(1, 1, image::Rgba([0, 0, 0, 255])));
        assert_eq!(on_white(ink).to_rgba8().get_pixel(0, 0).0, [0, 0, 0, 255]);
    }
}
