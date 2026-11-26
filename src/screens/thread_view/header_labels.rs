use super::{Card, HEADER_H, MAX_HEADER_H, Path, Rect};

/// Agents box of CARDS: left border + 1 space, the cards, 2 spaces + right border (the last card's
/// 4 gap columns hold 3 of those). 0 cards: 0.
pub(super) fn cards_w(cards: &[Card]) -> u16 {
    if cards.is_empty() {
        0
    } else {
        cards.iter().map(|card| card.width).sum::<u16>() + 1
    }
}

/// The home pill of the view bar: ` ⌂ home ` + ` ?N` (N: other threads that need the owner: an
/// ask, a blocked agent, a gate, an error, a step to review). True: N > 0.
pub(super) fn home_label(notes: &[crate::domain::notifications::Note]) -> (String, bool) {
    use crate::domain::notifications::Kind;

    let mut ids: Vec<&str> = notes
        .iter()
        .filter(|note| {
            matches!(
                note.kind,
                Kind::Ask | Kind::Blocked | Kind::Gate | Kind::Error | Kind::Step
            )
        })
        .map(|note| note.thread.as_str())
        .collect();

    ids.sort_unstable();
    ids.dedup();

    if ids.is_empty() {
        (" ⌂ home ".into(), false)
    } else {
        (format!(" ⌂ home ?{} ", ids.len()), true)
    }
}

/// Header rows (inside its frame + its top border) for COUNT agents on HARNESSES harnesses.
pub(super) fn header_rows(harnesses: usize, count: usize) -> u16 {
    // tokens box: 1 row per harness (its plan windows), 1 per agent (its context)
    (crate::features::cost::box_rows(harnesses, count) + 1).clamp(HEADER_H, MAX_HEADER_H)
}

/// Width of the input in its bubble: `│ ` left, ` │` right.
pub(super) fn input_width(cols: u16) -> u16 {
    cols.saturating_sub(4)
}

/// WORD with its 1st letter upper case: `claude` → `Claude`.
pub(super) fn title_case(word: &str) -> String {
    let mut chars = word.chars();

    chars
        .next()
        .map_or_else(String::new, |first| first.to_uppercase().chain(chars).collect())
}

/// Effort level as the header shows it: `medium` → `Medium`, `xhigh` → `XHigh`.
pub(super) fn effort_label(level: &str) -> String {
    if level == "xhigh" {
        "XHigh".into()
    } else {
        title_case(level)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum PromptHint {
    Directory,
    History,
    Shell,
    Empty,
    Text,
}

/// The hint row's left part: its width, short of the keys on the right (0 overlap).
pub(super) fn to_width(hint: Rect, mode: PromptHint) -> u16 {
    let keys = hint_keys(mode);

    hint.width
        .saturating_sub((keys.chars().count() as u16 + 1).min(hint.width - hint.width / 2))
}
pub(super) fn hint_keys(mode: PromptHint) -> &'static str {
    if mode == PromptHint::Directory {
        "tab: complete · ⏎ cd · esc / ↑: back "
    } else if mode == PromptHint::History {
        "↑↓ ctrl+r: pick · ⏎ take · esc close · type to filter "
    } else if mode == PromptHint::Shell {
        "⏎ run · tab complete · → suggestion · ↑↓ history · ctrl+r search · ↓ dir · output: to the agents with your next message "
    } else if mode == PromptHint::Empty {
        "! for shell mode · ⌘/ctrl+⏎ send · ↑↓ history "
    } else {
        "⌘/ctrl+⏎ send · ↑↓ history "
    }
}

/// PATH with the home dir as `~`.
pub(super) fn tilde(path: &Path) -> String {
    let home = crate::infra::config::home();

    match path.strip_prefix(&home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".into(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}
