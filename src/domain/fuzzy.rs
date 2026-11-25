/// Fuzzy match: Q's chars in order inside TEXT (case-insensitive). Higher = better:
/// adjacent matches and word starts score more. None: no match.
pub(crate) fn fuzzy(query: &str, text: &str) -> Option<i64> {
    let chars: Vec<char> = text.to_lowercase().chars().collect();
    let mut score = 0i64;
    let mut ti = 0usize;
    let mut prev: Option<usize> = None;

    for qc in query.chars().filter(|character| !character.is_whitespace()) {
        let at = (ti..chars.len()).find(|&i| chars[i] == qc)?;

        score += 1;

        if prev.is_some_and(|last| last + 1 == at) {
            score += 5;
        }

        if at == 0 || !chars[at - 1].is_alphanumeric() {
            score += 3;
        }

        prev = Some(at);
        ti = at + 1;
    }

    Some(score)
}
