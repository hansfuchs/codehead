pub(crate) fn open_url(url: &str) {
    if cfg!(test) || url.is_empty() || crate::infra::tutorial::active().is_some() {
        return;
    }

    let bin = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    let _ = std::process::Command::new(bin)
        .arg(url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}
