use std::fmt::Write as _;
use std::ops::ControlFlow;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::SystemTime;
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Shortcut {
    pub name: String,
    pub text: String,
}

/// The shortcuts of a new install (0 file yet).
pub(crate) fn defaults() -> Vec<Shortcut> {
    vec![Shortcut {
        name: "review".into(),
        text: "bring in another agent to review the code you wrote and have it write comments for each finding".into(),
    }]
}

pub(crate) fn path() -> PathBuf {
    // tests: the test data dir, never the owner's config
    if cfg!(test) {
        return crate::engine::data_dir().join("shortcuts.toml");
    }

    crate::engine::shared_dir().join("shortcuts.toml")
}

/// The last read: the file's time, the shortcuts.
static CACHE: Mutex<Option<(Option<SystemTime>, Vec<Shortcut>)>> = Mutex::new(None);

/// A valid name: letters, digits, `_`, `-`.
pub(crate) fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|character| character.is_alphanumeric() || character == '_' || character == '-')
}

pub(crate) fn parse(text: &str) -> Result<Vec<Shortcut>, String> {
    let table: toml::Table = text.parse().map_err(|error: toml::de::Error| error.to_string())?;

    let mut shortcuts: Vec<Shortcut> = table
        .into_iter()
        .filter_map(|(name, value)| {
            Some(Shortcut {
                name,
                text: value.as_str()?.to_string(),
            })
        })
        .collect();

    shortcuts.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(shortcuts)
}

/// Every shortcut, A→Z (1 stat per call; the file again when it changed).
pub(crate) fn all() -> Vec<Shortcut> {
    let file = path();
    let at = std::fs::metadata(&file).and_then(|meta| meta.modified()).ok();
    let mut cache = CACHE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);

    if let Some((cached_at, cached)) = cache.as_ref()
        && *cached_at == at
    {
        return cached.clone();
    }

    let shortcuts = match std::fs::read_to_string(&file) {
        Ok(text) => parse(&text).unwrap_or_default(),
        Err(_) => defaults(),
    };

    *cache = Some((at, shortcuts.clone()));

    shortcuts
}

/// The text of shortcut NAME.
pub(crate) fn get(name: &str) -> Option<String> {
    all()
        .into_iter()
        .find(|shortcut| shortcut.name == name)
        .map(|shortcut| shortcut.text)
}

/// TOML of LIST: `name = '''text'''` (a text with `'''` in it: a basic string).
pub(crate) fn render(list: &[Shortcut]) -> String {
    let mut out = String::from("# codehead shortcuts: #name in a prompt → this text\n\n");

    for shortcut in list {
        let key = if shortcut
            .name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_' || character == '-')
        {
            shortcut.name.clone()
        } else {
            toml::Value::String(shortcut.name.clone()).to_string()
        };

        if shortcut.text.contains("'''") {
            let _ = write!(out, "{key} = {}\n\n", toml::Value::String(shortcut.text.clone()));
        } else {
            let _ = write!(out, "{key} = '''\n{}'''\n\n", shortcut.text);
        }
    }

    out
}

/// LIST into the file.
pub(crate) fn save(list: &[Shortcut]) -> Result<(), String> {
    let file = path();

    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|error| error.to_string())?;
    }

    let mut list = list.to_vec();

    list.sort_by(|left, right| left.name.cmp(&right.name));
    crate::engine::write_atomic(&file, render(&list).as_bytes()).map_err(|error| error.to_string())?;
    let at = std::fs::metadata(&file).and_then(|meta| meta.modified()).ok();

    *CACHE.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some((at, list));

    Ok(())
}

/// TEXT with each `#name` of a shortcut (at a word start, the name whole) as the shortcut's text.
pub(crate) fn expand(text: &str) -> String {
    if !text.contains('#') {
        return text.to_string();
    }

    expand_with(&all(), text)
}

pub(crate) fn expand_with(list: &[Shortcut], text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let name_char = |character: char| character.is_alphanumeric() || character == '_' || character == '-';
    let mut out = String::with_capacity(text.len());
    let mut i = 0;

    while i < chars.len() {
        let character = chars[i];
        let start = i == 0 || chars[i - 1].is_whitespace() || matches!(chars[i - 1], '(' | '[' | '"' | '\'');

        if let ControlFlow::Break(()) = expand_shortcut_at(list, &chars, name_char, &mut out, &mut i, character, start)
        {
            continue;
        }

        out.push(character);
        i += 1;
    }

    out
}

fn expand_shortcut_at(
    list: &[Shortcut],
    chars: &[char],
    name_char: impl Fn(char) -> bool,
    out: &mut String,
    i: &mut usize,
    character: char,
    start: bool,
) -> ControlFlow<()> {
    if character == '#' && start {
        let mut j = *i + 1;

        skip_name_chars(chars, name_char, &mut j);

        let word: String = chars[*i + 1..j].iter().collect();

        if let ControlFlow::Break(()) = push_shortcut_text(list, out, i, j, &word) {
            return ControlFlow::Break(());
        }
    }

    ControlFlow::Continue(())
}

fn push_shortcut_text(list: &[Shortcut], out: &mut String, i: &mut usize, j: usize, word: &str) -> ControlFlow<()> {
    if let Some(shortcut) = list.iter().find(|shortcut| shortcut.name == word) {
        out.push_str(&shortcut.text);
        *i = j;
        return ControlFlow::Break(());
    }

    ControlFlow::Continue(())
}

fn skip_name_chars(chars: &[char], name_char: impl Fn(char) -> bool, j: &mut usize) {
    while *j < chars.len() && name_char(chars[*j]) {
        *j += 1;
    }
}
