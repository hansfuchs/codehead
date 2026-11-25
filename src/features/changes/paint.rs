use super::{FileDiff, HashMap, Hunk, Kind, Modifier, Span, Style, theme};

/// A diff line ready to draw: its chars in syntax colors, the chars of the changed words.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Painted {
    pub(super) chars: Vec<(ratatui::style::Color, char)>,
    pub(super) marks: Vec<bool>,
}

/// The diff text of F (its lines and hunk contexts): what the filter matches as content.
pub(super) fn body_text(file: &FileDiff) -> String {
    let mut text_value = String::new();

    for hunk in &file.hunks {
        text_value.push_str(&hunk.ctx);
        text_value.push('\n');

        for line in &hunk.lines {
            text_value.push_str(&line.text);
            text_value.push('\n');
        }
    }

    text_value
}

/// Syntax token of PATH: its extension (`rs`, `ts` …), else `text`.
pub(super) fn lang_of(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);

    match name.rsplit_once('.') {
        Some((_, error)) if !error.is_empty() => error,
        _ if name.eq_ignore_ascii_case("dockerfile") => "dockerfile",
        _ if name.eq_ignore_ascii_case("makefile") || name == "justfile" => "makefile",
        _ => "text",
    }
}

/// Word tokens of CHARS: a run of letters / digits / `_`, a run of spaces, else 1 char.
pub(super) fn tokens(chars: &[char]) -> Vec<std::ops::Range<usize>> {
    let mut out = Vec::new();
    let mut text = 0;

    while text < chars.len() {
        let key = token_class(chars[text]);
        let mut error = text + 1;

        while key != 2 && error < chars.len() && token_class(chars[error]) == key {
            error += 1;
        }

        out.push(text..error);
        text = error;
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

    let equal =
        |agent: &std::ops::Range<usize>, bytes: &std::ops::Range<usize>| old[agent.clone()] == new[bytes.clone()];

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

/// The lines of hunk H of F painted: syntax colors (old side / new side highlighted apart), the
/// changed words of each removed line and the added line it pairs with.
pub(super) fn paint(file: &FileDiff, hunk: &Hunk) -> Vec<Painted> {
    let lang = lang_of(&file.path);

    let (new_c, old_c) = (
        highlight_side(hunk, lang, Kind::Del),
        highlight_side(hunk, lang, Kind::Add),
    );

    // the theme's colors, more saturated (a dozen colors per hunk: memo)
    let mut memo: HashMap<ratatui::style::Color, ratatui::style::Color> = HashMap::new();

    let mut vivid = |color: &Vec<(ratatui::style::Color, char)>| -> Vec<(ratatui::style::Color, char)> {
        color
            .iter()
            .map(|&(col, ch)| {
                (
                    *memo.entry(col).or_insert_with(|| theme::desaturate(col, SATURATION)),
                    ch,
                )
            })
            .collect()
    };

    let (mut io, mut inew) = (0, 0);

    let mut out: Vec<Painted> = hunk
        .lines
        .iter()
        .map(|line_2| {
            let src = match line_2.kind {
                Kind::Del => {
                    io += 1;
                    old_c.get(io - 1)
                }
                Kind::Add => {
                    inew += 1;
                    new_c.get(inew - 1)
                }
                Kind::Ctx => {
                    io += 1;
                    inew += 1;
                    new_c.get(inew - 1)
                }
            };

            let plain: Vec<char> = line_2.text.chars().collect();

            let chars = match src {
                Some(character) if character.len() == plain.len() => vivid(character),
                _ => plain
                    .iter()
                    .map(|&character_value| (theme::pal().text, character_value))
                    .collect(),
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
    let lines_value = &hunk.lines;
    let mut offset = 0;

    while offset < lines_value.len() {
        if lines_value[offset].kind != Kind::Del {
            offset += 1;
            continue;
        }

        let removed = lines_value[offset..]
            .iter()
            .take_while(|line_3| line_3.kind == Kind::Del)
            .count();
        let added = lines_value[offset + removed..]
            .iter()
            .take_while(|line_4| line_4.kind == Kind::Add)
            .count();

        for j in 0..removed.min(added) {
            let (output, count_value) = (offset + j, offset + removed + j);
            let oc: Vec<char> = out[output].chars.iter().map(|x| x.1).collect();
            let nc: Vec<char> = out[count_value].chars.iter().map(|x| x.1).collect();
            let (mo, mn) = word_marks(&oc, &nc);

            out[output].marks = mo;
            out[count_value].marks = mn;
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

    let st = Style::default().fg(theme::pal().text).add_modifier(Modifier::BOLD);

    Span::styled(format!(" {sign} "), bg.map_or(st, |bytes| st.bg(bytes)))
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

/// P as spans in ROOM columns (cut with `…`), padded to ROOM; changed words on their tint.
pub(super) fn painted_spans(path: &Painted, kind: Kind, room: usize) -> Vec<Span<'static>> {
    let (_, mark) = tints(kind);

    let style = |color, marked: bool| {
        let style_value = Style::default().fg(color);

        match (marked, mark) {
            (true, Some(message)) => style_value.bg(message),
            _ => style_value,
        }
    };

    let total: usize = path
        .chars
        .iter()
        .map(|x| unicode_width::UnicodeWidthChar::width(x.1).unwrap_or(0))
        .sum();
    let limit = if total > room { room.saturating_sub(1) } else { room };
    let mut out: Vec<Span<'static>> = Vec::new();
    let (mut run, mut key, mut width_value) = (String::new(), None, 0);

    for (i, &(color, character)) in path.chars.iter().enumerate() {
        let cw = unicode_width::UnicodeWidthChar::width(character).unwrap_or(0);

        if width_value + cw > limit {
            break;
        }

        let key_value = (color, path.marks.get(i).copied().unwrap_or(false));

        if key != Some(key_value) && !run.is_empty() {
            let (c0, m0) = key.unwrap_or(key_value);

            out.push(Span::styled(std::mem::take(&mut run), style(c0, m0)));
        }

        key = Some(key_value);
        run.push(character);
        width_value += cw;
    }

    if let Some((c0, m0)) = key
        && !run.is_empty()
    {
        out.push(Span::styled(run, style(c0, m0)));
    }

    if total > room {
        out.push(Span::styled("…", style(theme::pal().subtext0, false)));
        width_value += 1;
    }

    out.push(Span::raw(" ".repeat(room.saturating_sub(width_value))));

    out
}

fn highlight_side(hunk: &Hunk, lang: &str, keep: Kind) -> crate::ui::code::Lines {
    let text: Vec<&str> = hunk
        .lines
        .iter()
        .filter(|line| line.kind != keep)
        .map(|line_value| line_value.text.as_str())
        .collect();

    crate::ui::code::highlight(&text.join("\n"), lang)
}
