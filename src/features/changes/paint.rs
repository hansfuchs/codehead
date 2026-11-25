use super::{FileDiff, HashMap, Hunk, Kind, Modifier, Span, Style, theme};

/// A diff line ready to draw: its chars in syntax colors, the chars of the changed words.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Painted {
    pub(super) chars: Vec<(ratatui::style::Color, char)>,
    pub(super) marks: Vec<bool>,
}

/// The diff text of FILE (its lines and hunk contexts): what the filter matches as content.
pub(super) fn body_text(file: &FileDiff) -> String {
    let mut text = String::new();

    for hunk in &file.hunks {
        text.push_str(&hunk.ctx);
        text.push('\n');

        for line in &hunk.lines {
            text.push_str(&line.text);
            text.push('\n');
        }
    }

    text
}

/// Syntax token of PATH: its extension (`rs`, `ts` …), else `text`.
pub(super) fn lang_of(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);

    match name.rsplit_once('.') {
        Some((_, ext)) if !ext.is_empty() => ext,
        _ if name.eq_ignore_ascii_case("dockerfile") => "dockerfile",
        _ if name.eq_ignore_ascii_case("makefile") || name == "justfile" => "makefile",
        _ => "text",
    }
}

/// Word tokens of CHARS: a run of letters / digits / `_`, a run of spaces, else 1 char.
pub(super) fn tokens(chars: &[char]) -> Vec<std::ops::Range<usize>> {
    let mut out = Vec::new();
    let mut start = 0;

    while start < chars.len() {
        let class = token_class(chars[start]);
        let mut end = start + 1;

        while class != 2 && end < chars.len() && token_class(chars[end]) == class {
            end += 1;
        }

        out.push(start..end);
        start = end;
    }

    out
}

/// Changed chars of OLD and NEW (a removed line and the added line it pairs with): the tokens
/// outside their longest common token run. 0 marks when the lines share under 1/3 of their
/// tokens (a rewrite: the line tint says enough) or the lines are long.
pub(crate) fn word_marks(old: &[char], new: &[char]) -> (Vec<bool>, Vec<bool>) {
    let (old_tokens, new_tokens) = (tokens(old), tokens(new));
    let none = || (vec![false; old.len()], vec![false; new.len()]);

    if old_tokens.len() > 300 || new_tokens.len() > 300 {
        return none();
    }

    let equal = |old_range: &std::ops::Range<usize>, new_range: &std::ops::Range<usize>| {
        old[old_range.clone()] == new[new_range.clone()]
    };

    // LCS table over tokens
    let (old_count, new_count) = (old_tokens.len(), new_tokens.len());
    let mut table = vec![vec![0u16; new_count + 1]; old_count + 1];

    for i in (0..old_count).rev() {
        lcs_row(i, &old_tokens, &new_tokens, old, new, &mut table);
    }

    let (mut old_kept, mut new_kept) = (vec![false; old_count], vec![false; new_count]);
    let (mut i, mut j) = (0, 0);

    while i < old_count && j < new_count {
        if equal(&old_tokens[i], &new_tokens[j]) {
            old_kept[i] = true;
            new_kept[j] = true;
            i += 1;
            j += 1;
        } else if table[i + 1][j] >= table[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }

    let (old_shared, old_words) = solid_tokens(&old_tokens, old, &old_kept);
    let (new_shared, new_words) = solid_tokens(&new_tokens, new, &new_kept);

    if (old_shared + new_shared) * 3 < old_words + new_words {
        return none();
    }

    (
        changed_chars(&old_tokens, old.len(), &old_kept),
        changed_chars(&new_tokens, new.len(), &new_kept),
    )
}

fn token_class(character: char) -> u8 {
    if character.is_alphanumeric() || character == '_' {
        0
    } else if character.is_whitespace() {
        1
    } else {
        2
    }
}

fn lcs_row(
    i: usize,
    old_tokens: &[std::ops::Range<usize>],
    new_tokens: &[std::ops::Range<usize>],
    old: &[char],
    new: &[char],
    table: &mut [Vec<u16>],
) {
    for j in (0..new_tokens.len()).rev() {
        table[i][j] = if old[old_tokens[i].clone()] == new[new_tokens[j].clone()] {
            table[i + 1][j + 1] + 1
        } else {
            table[i + 1][j].max(table[i][j + 1])
        };
    }
}

fn solid_tokens(tokens: &[std::ops::Range<usize>], chars: &[char], kept: &[bool]) -> (usize, usize) {
    let words: Vec<usize> = (0..tokens.len())
        .filter(|&i| !chars[tokens[i].start].is_whitespace())
        .collect();

    (words.iter().filter(|&&i| kept[i]).count(), words.len())
}

fn changed_chars(tokens: &[std::ops::Range<usize>], length: usize, kept: &[bool]) -> Vec<bool> {
    let mut marks = vec![false; length];

    for (i, token) in tokens.iter().enumerate() {
        if !kept[i] {
            marks[token.clone()].fill(true);
        }
    }

    marks
}

/// The lines of HUNK of FILE painted: syntax colors (old side / new side highlighted apart), the
/// changed words of each removed line and the added line it pairs with.
pub(super) fn paint(file: &FileDiff, hunk: &Hunk) -> Vec<Painted> {
    let lang = lang_of(&file.path);

    let (new_colors, old_colors) = (
        highlight_side(hunk, lang, Kind::Del),
        highlight_side(hunk, lang, Kind::Add),
    );

    // the theme's colors, more saturated (a dozen colors per hunk: memo)
    let mut memo: HashMap<ratatui::style::Color, ratatui::style::Color> = HashMap::new();

    let mut vivid = |colored: &Vec<(ratatui::style::Color, char)>| -> Vec<(ratatui::style::Color, char)> {
        colored
            .iter()
            .map(|&(color, character)| {
                (
                    *memo
                        .entry(color)
                        .or_insert_with(|| theme::desaturate(color, SATURATION)),
                    character,
                )
            })
            .collect()
    };

    let (mut old_index, mut new_index) = (0, 0);

    let mut out: Vec<Painted> = hunk
        .lines
        .iter()
        .map(|line| {
            let colors = match line.kind {
                Kind::Del => {
                    old_index += 1;
                    old_colors.get(old_index - 1)
                }
                Kind::Add => {
                    new_index += 1;
                    new_colors.get(new_index - 1)
                }
                Kind::Ctx => {
                    old_index += 1;
                    new_index += 1;
                    new_colors.get(new_index - 1)
                }
            };

            let plain: Vec<char> = line.text.chars().collect();

            let chars = match colors {
                Some(colored) if colored.len() == plain.len() => vivid(colored),
                _ => plain.iter().map(|&character| (theme::pal().text, character)).collect(),
            };

            Painted {
                marks: vec![false; chars.len()],
                chars,
            }
        })
        .collect();

    // pairs: a run of removed lines with the added lines right after it
    apply_word_marks(hunk, &mut out);

    out
}

fn apply_word_marks(hunk: &Hunk, out: &mut [Painted]) {
    let lines = &hunk.lines;
    let mut offset = 0;

    while offset < lines.len() {
        if lines[offset].kind != Kind::Del {
            offset += 1;
            continue;
        }

        let removed = lines[offset..].iter().take_while(|line| line.kind == Kind::Del).count();
        let added = lines[offset + removed..]
            .iter()
            .take_while(|line| line.kind == Kind::Add)
            .count();

        for j in 0..removed.min(added) {
            let (removed_at, added_at) = (offset + j, offset + removed + j);
            let old_chars: Vec<char> = out[removed_at].chars.iter().map(|&(_, character)| character).collect();
            let new_chars: Vec<char> = out[added_at].chars.iter().map(|&(_, character)| character).collect();
            let (old_marks, new_marks) = word_marks(&old_chars, &new_chars);

            out[removed_at].marks = old_marks;
            out[added_at].marks = new_marks;
        }

        offset += removed + added;
    }
}

/// Syntax colors: the theme's saturation times this.
pub(super) const SATURATION: f32 = 1.45;

/// ` + ` / ` - ` / 3 spaces: the sign column, on the add / remove color (0 tint on the line).
pub(super) fn sign_span(kind: Kind) -> Span<'static> {
    let (bg, _) = tints(kind);

    let sign = match kind {
        Kind::Add => "+",
        Kind::Del => "-",
        Kind::Ctx => " ",
    };

    let style = Style::default().fg(theme::pal().text).add_modifier(Modifier::BOLD);

    Span::styled(format!(" {sign} "), bg.map_or(style, |bg| style.bg(bg)))
}

/// Sign column color and changed-word tint of KIND (0 for context lines).
pub(super) fn tints(kind: Kind) -> (Option<ratatui::style::Color>, Option<ratatui::style::Color>) {
    let base = theme::pal().surface0;

    match kind {
        Kind::Add => (
            Some(theme::tint(theme::pal().success(), base, 0.7)),
            Some(theme::tint(theme::pal().success(), base, 0.32)),
        ),
        Kind::Del => (
            Some(theme::tint(theme::pal().error(), base, 0.7)),
            Some(theme::tint(theme::pal().error(), base, 0.32)),
        ),
        Kind::Ctx => (None, None),
    }
}

/// PAINTED as spans in ROOM columns (cut with `…`), padded to ROOM; changed words on their tint.
pub(super) fn painted_spans(painted: &Painted, kind: Kind, room: usize) -> Vec<Span<'static>> {
    let (_, mark) = tints(kind);

    let style = |color, marked: bool| {
        let plain = Style::default().fg(color);

        match (marked, mark) {
            (true, Some(tint)) => plain.bg(tint),
            _ => plain,
        }
    };

    let total: usize = painted
        .chars
        .iter()
        .map(|&(_, character)| unicode_width::UnicodeWidthChar::width(character).unwrap_or(0))
        .sum();
    let limit = if total > room { room.saturating_sub(1) } else { room };
    let mut out: Vec<Span<'static>> = Vec::new();
    let (mut run, mut run_key, mut width) = (String::new(), None, 0);

    for (i, &(color, character)) in painted.chars.iter().enumerate() {
        let char_width = unicode_width::UnicodeWidthChar::width(character).unwrap_or(0);

        if width + char_width > limit {
            break;
        }

        let next_key = (color, painted.marks.get(i).copied().unwrap_or(false));

        if run_key != Some(next_key) && !run.is_empty() {
            let (run_color, run_marked) = run_key.unwrap_or(next_key);

            out.push(Span::styled(std::mem::take(&mut run), style(run_color, run_marked)));
        }

        run_key = Some(next_key);
        run.push(character);
        width += char_width;
    }

    if let Some((run_color, run_marked)) = run_key
        && !run.is_empty()
    {
        out.push(Span::styled(run, style(run_color, run_marked)));
    }

    if total > room {
        out.push(Span::styled("…", style(theme::pal().subtext0, false)));
        width += 1;
    }

    out.push(Span::raw(" ".repeat(room.saturating_sub(width))));

    out
}

fn highlight_side(hunk: &Hunk, lang: &str, keep: Kind) -> crate::ui::code::Lines {
    let text: Vec<&str> = hunk
        .lines
        .iter()
        .filter(|line| line.kind != keep)
        .map(|line| line.text.as_str())
        .collect();

    crate::ui::code::highlight(&text.join("\n"), lang)
}
