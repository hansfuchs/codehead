use super::{Path, PathBuf, theme};

/// Share the files of FILES under HOME (the home repo): `git add` + `git commit` of only those
/// paths (message: notes of TITLE), then a push in the background when the branch has an
/// upstream. Ok(what happened).
pub(crate) fn share(home: &Path, files: &[PathBuf], title: &str) -> Result<String, String> {
    let rel: Vec<String> = files
        .iter()
        .filter_map(|path| path.strip_prefix(home).ok())
        .map(|path_value| path_value.display().to_string())
        .collect();

    if rel.is_empty() {
        return Err("0 home repo files in this thread".into());
    }

    let git = |args: &[&str]| -> Result<String, String> {
        let output_value = std::process::Command::new("git")
            .arg("-C")
            .arg(home)
            .args(args)
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|error| format!("git: {error}"))?;

        if output_value.status.success() {
            Ok(String::from_utf8_lossy(&output_value.stdout).into_owned())
        } else {
            Err(String::from_utf8_lossy(&output_value.stderr)
                .lines()
                .next()
                .unwrap_or("git failed")
                .to_string())
        }
    };

    let mut add = vec!["add", "--"];

    add.extend(rel.iter().map(String::as_str));
    git(&add)?;
    let mut st = vec!["diff", "--cached", "--name-only", "--"];

    st.extend(rel.iter().map(String::as_str));
    let staged = git(&st)?;

    if staged.trim().is_empty() {
        return Ok("home repo: 0 changes to share".into());
    }

    let msg = format!("notes: {}", title.lines().next().unwrap_or("thread"));
    let mut commit = vec!["commit", "-q", "-m", &msg, "--"];

    commit.extend(rel.iter().map(String::as_str));
    git(&commit)?;
    let count_value = staged.lines().count();

    if git(&["rev-parse", "--abbrev-ref", "@{u}"]).is_ok() {
        let _ = std::process::Command::new("git")
            .arg("-C")
            .arg(home)
            .args(["push", "-q"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();

        Ok(format!("home repo: {count_value} files committed, pushing"))
    } else {
        Ok(format!("home repo: {count_value} files committed (0 upstream: 0 push)"))
    }
}
/// yazi's config dir owned by codehead: a theme from the palette (0 change to ~/.config/yazi).
pub(crate) fn yazi_config() -> PathBuf {
    let dir = crate::infra::config::home().join(".cache/codehead/yazi");
    let _ = std::fs::create_dir_all(&dir);
    let hex = |character| theme::hex(character);
    let path = theme::pal();
    let theme_toml = format!(
        "[mgr]\ncwd = {{ fg = \"{acc}\" }}\nhovered = {{ fg = \"{on}\", bg = \"{acc}\" }}\npreview_hovered = {{ underline = true }}\nborder_style = {{ fg = \"{line}\" }}\n\n[status]\nsep_left = {{ open = \"\", close = \"\" }}\nsep_right = {{ open = \"\", close = \"\" }}\n\n[mode]\nnormal_main = {{ fg = \"{on}\", bg = \"{acc}\", bold = true }}\nnormal_alt = {{ fg = \"{acc}\", bg = \"{panel}\" }}\n",
        acc = hex(path.accent),
        on = hex(path.on_accent),
        line = hex(path.overlay0),
        panel = hex(path.surface0)
    );
    let _ = std::fs::write(dir.join("theme.toml"), theme_toml);

    // enter on a file: pick it (back to codehead), as yazi.nvim does
    let _ = std::fs::write(
        dir.join("keymap.toml"),
        "[[mgr.prepend_keymap]]\non = \"<Enter>\"\nrun = \"open\"\ndesc = \"pick: back to codehead\"\n",
    );

    dir
}
/// The yazi command for TARGET (a dir, or a file to reveal): argv, env, the chooser file.
pub(crate) fn yazi_cmd(target: &Path) -> (Vec<String>, Vec<(String, String)>, PathBuf) {
    let chooser = std::env::temp_dir().join(format!("codehead-yazi-{}", std::process::id()));
    let _ = std::fs::remove_file(&chooser);
    let argv = vec![
        "yazi".to_string(),
        target.display().to_string(),
        format!("--chooser-file={}", chooser.display()),
    ];

    (
        argv,
        vec![("YAZI_CONFIG_HOME".into(), yazi_config().display().to_string())],
        chooser,
    )
}
