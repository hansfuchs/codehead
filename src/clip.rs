//! System clipboard through the usual CLI tools (Wayland first).

use std::io::Write;
use std::process::{Command, Stdio};

fn have(bin: &str) -> bool {
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| d.join(bin).is_file()))
        .unwrap_or(false)
}

fn wayland() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some()
}

/// The copy tools, in order: (binary, args). Wayland first.
const TOOLS: [(&str, &[&str]); 4] = [("wl-copy", &[]), ("xclip", &["-selection", "clipboard"]), ("xsel", &["-ib"]), ("pbcopy", &[])];

/// Copy TEXT. false = 0 clipboard tools found. The tool runs off the UI thread (a spawn and its
/// wait: 5-15 ms); the text is in the clipboard a few ms after the return.
pub fn copy(text: &str) -> bool {
    // tests: 0 writes to the owner's clipboard; the text for asserts
    #[cfg(test)]
    {
        LAST.with_borrow_mut(|l| *l = text.to_string());
        return true;
    }
    #[allow(unreachable_code)]
    copy_with(&TOOLS, text, false)
}

/// `copy` that waits for the tool: the process ends right after (`codehead -c`). false = 0 tool,
/// or it failed.
pub fn copy_wait(text: &str) -> bool {
    copy_with(&TOOLS, text, true)
}

#[cfg(test)]
thread_local! {
    /// The text the last `copy` of this test thread got.
    pub static LAST: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

/// TEXT to the 1st of TOOLS on $PATH. WAIT: the tool's end (its exit status), else off the
/// calling thread (true: a tool runs). false = 0 tool.
pub fn copy_with(tools: &[(&str, &[&str])], text: &str, wait: bool) -> bool {
    let Some((bin, args)) = tools.iter().find(|(bin, _)| have(bin) && (*bin != "wl-copy" || wayland())) else { return false };
    let (bin, args, text) = (bin.to_string(), args.iter().map(|a| a.to_string()).collect::<Vec<_>>(), text.to_string());
    let run = move || {
        let Ok(mut c) = Command::new(&bin).args(&args).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() else { return false };
        if let Some(mut stdin) = c.stdin.take() {
            let _ = stdin.write_all(text.as_bytes());
        }
        c.wait().is_ok_and(|s| s.success())
    };
    if wait {
        return run();
    }
    std::thread::spawn(run);
    true
}

pub fn paste() -> Option<String> {
    let tools: [(&str, &[&str]); 4] =
        [("wl-paste", &["--no-newline"]), ("xclip", &["-selection", "clipboard", "-o"]), ("xsel", &["-ob"]), ("pbpaste", &[])];
    for (bin, args) in tools {
        if !have(bin) || (bin == "wl-paste" && !wayland()) {
            continue;
        }
        if let Ok(o) = Command::new(bin).args(args).stdin(Stdio::null()).stderr(Stdio::null()).output()
            && o.status.success()
        {
            return Some(String::from_utf8_lossy(&o.stdout).into_owned());
        }
    }
    None
}

/// Clipboard image → PNG file at DEST. false: 0 image in the clipboard (or 0 tools).
/// macOS: osascript (PNGf). Wayland: wl-paste. X11: xclip.
pub fn paste_image(dest: &std::path::Path) -> bool {
    if let Some(dir) = dest.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let png = |b: &[u8]| b.starts_with(b"\x89PNG");
    if have("osascript") {
        let p = dest.display().to_string().replace('\\', "\\\\").replace('"', "\\\"");
        let script = format!(
            "try\nset d to (the clipboard as «class PNGf»)\non error\nreturn \"none\"\nend try\n\
             set f to open for access POSIX file \"{p}\" with write permission\nset eof f to 0\nwrite d to f\nclose access f\nreturn \"ok\""
        );
        let ok = Command::new("osascript").args(["-e", &script]).stdin(Stdio::null()).stderr(Stdio::null()).output();
        if ok.is_ok_and(|o| String::from_utf8_lossy(&o.stdout).trim() == "ok") {
            return std::fs::read(dest).is_ok_and(|b| png(&b));
        }
        return false;
    }
    let tools: [(&str, &[&str]); 2] = [("wl-paste", &["--type", "image/png"]), ("xclip", &["-selection", "clipboard", "-t", "image/png", "-o"])];
    for (bin, args) in tools {
        if !have(bin) || (bin == "wl-paste" && !wayland()) {
            continue;
        }
        if let Ok(o) = Command::new(bin).args(args).stdin(Stdio::null()).stderr(Stdio::null()).output()
            && o.status.success()
            && png(&o.stdout)
        {
            return std::fs::write(dest, &o.stdout).is_ok();
        }
    }
    false
}
