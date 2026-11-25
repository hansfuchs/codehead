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
use ratatui_image::picker::ProtocolType;
#[path = "graphics_picker.rs"]
mod picker_state;
pub(crate) use picker_state::Picker;
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
pub(crate) enum Kind {
    /// Decoded here: png, jpeg, gif (1st frame), webp.
    Image,
    /// Rastered by a tool first: heic (macOS sips).
    Heic,
    /// Page 1, rastered by pdftoppm or sips, on white.
    Pdf,
}

pub(crate) fn kind(path: &Path) -> Option<Kind> {
    let ext = path.extension()?.to_string_lossy().to_lowercase();

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
    let set = |key: &str| var(key).is_some_and(|value| !value.is_empty());

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
    if ["foot", "mlterm", "contour"]
        .iter()
        .any(|thread| term.starts_with(thread))
    {
        return Some(ProtocolType::Sixel);
    }

    // over ssh 0 TERM_PROGRAM, iTerm2 still says LC_TERMINAL; a TERM_PROGRAM of another
    // terminal (vscode, Apple_Terminal): it wins over the leaked var
    if program.is_empty() && var("LC_TERMINAL").is_some_and(|thread_value| thread_value.contains("iTerm")) {
        return Some(ProtocolType::Iterm2);
    }

    None
}

/// The protocol: the `previews` setting (auto: from the env, 1x per process).
fn protocol() -> Option<ProtocolType> {
    static AUTO: std::sync::OnceLock<Option<ProtocolType>> = std::sync::OnceLock::new();

    match crate::infra::config::previews().as_str() {
        "off" => None,
        "kitty" => Some(ProtocolType::Kitty),
        "iterm2" => Some(ProtocolType::Iterm2),
        "sixel" => Some(ProtocolType::Sixel),
        _ => *AUTO.get_or_init(|| protocol_from(|key| std::env::var(key).ok())),
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
pub(crate) fn picker() -> Option<Picker> {
    if cfg!(test) {
        return None;
    }

    let proto = protocol()?;
    let path = Picker::new(font_size(), proto);

    Some(path)
}

/// A preview ready to draw: the encoded image, its line (`1920×1080 px · 812 KB`), its kitty id
/// (the terminal keeps kitty images until told to delete them) and its kitty transmit (until
/// sent).
pub(crate) struct Preview {
    pub protocol: Protocol,
    pub info: String,
    pub kitty: Option<u32>,
    /// The image data for the terminal, sent 1x straight to it (`send`), not in a cell: an
    /// overlay drawn over the image's 1st cell in that frame would drop it.
    pub transmit: Option<String>,
}

/// Path P (of KIND) encoded for PICKER to fit COLS × ROWS cells. Err: a line why not.
pub(crate) fn build(path: &Path, kind: Kind, picker: &Picker, cols: u16, rows: u16) -> Result<Preview, String> {
    if cols == 0 || rows == 0 {
        return Err("0 room for the image".into());
    }

    let bytes = std::fs::metadata(path).map_err(|error| error.to_string())?.len();

    if bytes > MAX_BYTES {
        return Err(format!(
            "{} MB: over {} MB, 0 preview · ctrl+.: yazi",
            bytes >> 20,
            MAX_BYTES >> 20
        ));
    }

    let fs = picker.font_size();
    let (pw, ph) = (
        u32::from(cols) * u32::from(fs.width),
        u32::from(rows) * u32::from(fs.height),
    );

    let img = match kind {
        Kind::Image => decode(path)?,
        Kind::Heic | Kind::Pdf => raster(path, pw.max(ph))?,
    };

    let (width_value, height_value) = (img.width(), img.height());

    // 1 fast downscale to the area's pixels: the encoder gets 0 more than it draws
    let img = if width_value > pw || height_value > ph {
        img.thumbnail(pw, ph)
    } else {
        img
    };

    // pdf pages: transparent; on white (black text on a dark terminal: invisible)
    let img = if kind == Kind::Pdf { on_white(img) } else { img };

    let (protocol, kitty, transmit) = if picker.protocol_type() == ProtocolType::Kitty {
        // zlib (`o=z`, in the kitty spec): the picker turns it on only after a terminal query
        let cells =
            |px: u32, cell: u16, max: u16| (px.div_ceil(u32::from(cell.max(1))).min(u32::from(max)) as u16).max(1);
        let size = Size::new(cells(img.width(), fs.width, cols), cells(img.height(), fs.height, rows));
        let id = next_id();
        let protocol =
            Protocol::Kitty(Kitty::new(img, size, id, false, true).map_err(|error_value| error_value.to_string())?);
        let transmit = take_transmit(&protocol, size);

        (protocol, Some(id), transmit)
    } else {
        (
            picker
                .new_protocol(img, Size::new(cols, rows), &(Resize::Fit(None)))
                .map_err(|error_2| error_2.to_string())?,
            None,
            None,
        )
    };

    let what = if kind == Kind::Pdf { "page 1 · " } else { "" };

    Ok(Preview {
        protocol,
        info: format!("{what}{width_value}×{height_value} px · {} KB", bytes.div_ceil(1024)),
        kitty,
        transmit,
    })
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
pub(crate) fn send(seq: &str) {
    if cfg!(test) {
        return;
    }

    let mut out = std::io::stdout();
    let _ = out.write_all(seq.as_bytes());
    let _ = out.flush();
}

/// P decoded within the limits (side, allocation), turned as its EXIF orientation says (phone
/// photos).
fn decode(path: &Path) -> Result<DynamicImage, String> {
    let mut limits = image::Limits::default();

    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    limits.max_alloc = Some(MAX_ALLOC);

    let mut reader = image::ImageReader::open(path)
        .and_then(image::ImageReader::with_guessed_format)
        .map_err(|error| error.to_string())?;

    reader.limits(limits.clone());
    let mut decoder = reader.into_decoder().map_err(|error_value| error_value.to_string())?;

    // the pixel buffer counts too (from_decoder reserves 0 of it): 16384² RGBA = 1 GiB
    limits
        .reserve(decoder.total_bytes())
        .map_err(|error_2| error_2.to_string())?;
    let turn = decoder.orientation().map_err(|error_3| error_3.to_string())?;
    let mut img = DynamicImage::from_decoder(decoder).map_err(|error_4| error_4.to_string())?;

    img.apply_orientation(turn);

    Ok(img)
}

/// A kitty image id: unique in the process, never 0 (its low 24 bits: the placeholders' color).
fn next_id() -> u32 {
    static NEXT_IMAGE: AtomicU32 = AtomicU32::new(0);
    NEXT_IMAGE.fetch_add(1, Ordering::Relaxed).wrapping_mul(2_654_435_761) | 1
}

/// Kitty image ID out of the terminal's store (its data and placements).
pub(crate) fn delete_kitty(id: u32) {
    send(&format!("\x1b_Ga=d,d=I,i={id},q=2\x1b\\"));
}

fn on_white(img: DynamicImage) -> DynamicImage {
    let mut rgba = img.into_rgba8();

    for px in rgba.pixels_mut() {
        let agent = u16::from(px[3]);

        for character in 0..3 {
            px[character] = ((u16::from(px[character]) * agent + 255 * (255 - agent)) / 255) as u8;
        }

        px[3] = 255;
    }

    DynamicImage::ImageRgba8(rgba)
}

fn on_path(bin: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|directory| directory.join(bin).is_file()))
}

/// The rasterizers' dir: private (0700) under codehead's cache, made 1x. A tool never writes
/// through a link someone put in a shared /tmp.
fn raster_dir() -> Result<PathBuf, String> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};

    let dir = crate::infra::config::home().join(".cache/codehead/raster");

    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&dir)
        .map_err(|error| error.to_string())?;
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
        .map_err(|error_value| error_value.to_string())?;
    Ok(dir)
}

/// P (pdf page 1, heic) as an image ≤ MAX px: pdftoppm (pdf), else sips (macOS); a temp png,
/// read, deleted.
fn raster(path: &Path, max: u32) -> Result<DynamicImage, String> {
    static NEXT_IMAGE: AtomicU64 = AtomicU64::new(0);

    // an absolute path only: a leading `-` never reads as a tool's option
    if !path.is_absolute() {
        return Err("0 preview: a relative path".into());
    }

    let is_pdf = path.extension().is_some_and(|error| error.eq_ignore_ascii_case("pdf"));
    let pdftoppm = is_pdf && on_path("pdftoppm");

    if !pdftoppm && !cfg!(target_os = "macos") {
        return Err(if is_pdf {
            "0 pdf rasterizer: install poppler (pdftoppm) · ctrl+.: yazi".into()
        } else {
            "heic: 0 decoder here · ctrl+.: yazi".into()
        });
    }

    // 1 name per call: 2 rasters at once never share a file
    let stem = raster_dir()?.join(format!(
        "{}-{}",
        std::process::id(),
        NEXT_IMAGE.fetch_add(1, Ordering::Relaxed)
    ));
    let out = stem.with_extension("png");
    let max = max.clamp(64, 4096).to_string();

    let argv: Vec<OsString> = if pdftoppm {
        let mut agent: Vec<OsString> = [
            "pdftoppm",
            "-png",
            "-f",
            "1",
            "-l",
            "1",
            "-singlefile",
            "-scale-to",
            &max,
        ]
        .iter()
        .map(OsString::from)
        .collect();

        agent.extend([path.as_os_str().to_owned(), stem.into_os_string()]);
        agent
    } else {
        let mut agent_value: Vec<OsString> = ["sips", "-s", "format", "png", "-Z", &max]
            .iter()
            .map(OsString::from)
            .collect();

        agent_value.extend([path.as_os_str().to_owned(), "--out".into(), out.as_os_str().to_owned()]);
        agent_value
    };

    let img = if run(&argv, RASTER_TIMEOUT) {
        decode(&out)
    } else {
        Err(format!("{} failed", argv[0].to_string_lossy()))
    };

    let _ = std::fs::remove_file(&out);

    img
}

/// Run ARGV (0 shell), at most TIMEOUT. True: exit 0.
fn run(argv: &[OsString], timeout: Duration) -> bool {
    let Ok(mut child) = Command::new(&argv[0])
        .args(&argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };

    let end = Instant::now() + timeout;

    loop {
        match child.try_wait() {
            Ok(Some(text)) => return text.success(),
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
#[path = "graphics_tests.rs"]
mod tests;
