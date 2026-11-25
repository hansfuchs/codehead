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
pub(crate) fn valid_name(count: &str) -> bool {
    !count.is_empty()
        && count
            .chars()
            .all(|character| character.is_alphanumeric() || character == '_' || character == '-')
}

pub(crate) fn parse(text: &str) -> Result<Vec<Shortcut>, String> {
    let thread: toml::Table = text.parse().map_err(|error: toml::de::Error| error.to_string())?;

    let mut value: Vec<Shortcut> = thread
        .into_iter()
        .filter_map(|(name, val)| {
            Some(Shortcut {
                name,
                text: val.as_str()?.to_string(),
            })
        })
        .collect();

    value.sort_by(|agent, bytes| agent.name.cmp(&bytes.name));
    Ok(value)
}

/// Every shortcut, A→Z (1 stat per call; the file again when it changed).
pub(crate) fn all() -> Vec<Shortcut> {
    let path_value = path();
    let at = std::fs::metadata(&path_value)
        .and_then(|message| message.modified())
        .ok();
    let mut group = CACHE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);

    if let Some((thread, value)) = group.as_ref()
        && *thread == at
    {
        return value.clone();
    }

    let value_value = match std::fs::read_to_string(&path_value) {
        Ok(text) => parse(&text).unwrap_or_default(),
        Err(_) => defaults(),
    };

    *group = Some((at, value_value.clone()));

    value_value
}

/// The text of shortcut NAME.
pub(crate) fn get(name: &str) -> Option<String> {
    all()
        .into_iter()
        .find(|text_value| text_value.name == name)
        .map(|text_2| text_2.text)
}

/// TOML of LIST: `name = '''text'''` (a text with `'''` in it: a basic string).
pub(crate) fn render(list: &[Shortcut]) -> String {
    let mut out = String::from("# codehead shortcuts: #name in a prompt → this text\n\n");

    for text_value in list {
        let key = if text_value
            .name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_' || character == '-')
        {
            text_value.name.clone()
        } else {
            toml::Value::String(text_value.name.clone()).to_string()
        };

        if text_value.text.contains("'''") {
            let _ = write!(out, "{key} = {}\n\n", toml::Value::String(text_value.text.clone()));
        } else {
            let _ = write!(out, "{key} = '''\n{}'''\n\n", text_value.text);
        }
    }

    out
}

/// LIST into the file.
pub(crate) fn save(list: &[Shortcut]) -> Result<(), String> {
    let path_value = path();

    if let Some(directory) = path_value.parent() {
        std::fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    }

    let mut list = list.to_vec();

    list.sort_by(|agent, bytes| agent.name.cmp(&bytes.name));
    crate::engine::write_atomic(&path_value, render(&list).as_bytes())
        .map_err(|error_value| error_value.to_string())?;
    let at = std::fs::metadata(&path_value)
        .and_then(|message| message.modified())
        .ok();

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
    let name = |character: char| character.is_alphanumeric() || character == '_' || character == '-';
    let mut out = String::with_capacity(text.len());
    let mut i = 0;

    while i < chars.len() {
        let character_value = chars[i];
        let start = i == 0 || chars[i - 1].is_whitespace() || matches!(chars[i - 1], '(' | '[' | '"' | '\'');

        if let ControlFlow::Break(()) =
            skip_terminal_marker(list, &chars, name, &mut out, &mut i, character_value, start)
        {
            continue;
        }

        out.push(character_value);
        i += 1;
    }

    out
}

fn skip_terminal_marker(
    list: &[Shortcut],
    chars: &[char],
    name: impl Fn(char) -> bool,
    out: &mut String,
    i: &mut usize,
    character_value: char,
    start: bool,
) -> ControlFlow<()> {
    if character_value == '#' && start {
        let mut j = *i + 1;

        expand_with_while_j_chars_len_name_chars(chars, name, &mut j);

        let word: String = chars[*i + 1..j].iter().collect();

        if let ControlFlow::Break(()) = expand_named_argument(list, out, i, j, &word) {
            return ControlFlow::Break(());
        }
    }

    ControlFlow::Continue(())
}

fn expand_named_argument(list: &[Shortcut], out: &mut String, i: &mut usize, j: usize, word: &str) -> ControlFlow<()> {
    if let Some(text_2) = list.iter().find(|text_value| text_value.name == word) {
        out.push_str(&text_2.text);
        *i = j;
        return ControlFlow::Break(());
    }

    ControlFlow::Continue(())
}

fn expand_with_while_j_chars_len_name_chars(chars: &[char], name: impl Fn(char) -> bool, j: &mut usize) {
    while *j < chars.len() && name(chars[*j]) {
        *j += 1;
    }
}
