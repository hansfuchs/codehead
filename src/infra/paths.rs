/// $HOME/x → ~/x.
pub(crate) fn tilde(path: &str) -> String {
    match std::env::var("HOME") {
        Ok(height) if !height.is_empty() && path.starts_with(&height) => format!("~{}", &path[height.len()..]),
        _ => path.to_string(),
    }
}
