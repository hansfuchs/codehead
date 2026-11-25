use super::*;

#[test]
fn substring_beats_subsequence_and_word_start_beats_middle() {
    let (substring, pa) = word("view", "src/view.rs").unwrap();
    let (subsequence, _) = word("vw", "src/view.rs").unwrap();

    assert!(substring > subsequence);
    assert_eq!(pa, vec![4, 5, 6, 7]);

    let (start, _) = word("rs", "src/rs.rs").unwrap();
    let (middle, _) = word("rs", "src/users.txt").unwrap();

    assert!(start > middle);
    assert!(word("xyz", "src/view.rs").is_none());
}

#[test]
fn every_word_must_match_and_name_hits_rank_first() {
    let files = [
        ("src/app.rs", "fn main() { view }"),
        ("src/view.rs", "fn draw()"),
        ("README.md", "nothing"),
    ];
    let ranked = rank(&files, "view", |file| file.0.to_string(), |file| file.1.to_string());

    assert_eq!(ranked.iter().map(|hit| hit.0).collect::<Vec<_>>(), vec![1, 0]);
    assert_eq!(ranked[0].1, vec![4, 5, 6, 7]);
    assert_eq!(
        rank(
            &files,
            "view nothing",
            |file| file.0.to_string(),
            |file| file.1.to_string()
        ),
        [] as [(usize, std::vec::Vec<usize>); 0]
    );
    assert_eq!(
        rank(&files, " ", |file| file.0.to_string(), |file| file.1.to_string()).len(),
        3
    );
}

#[test]
fn spans_mark_the_hits() {
    let spans = spans("abcd", &[1, 2], Style::default(), Style::default().bold());

    assert_eq!(
        spans.iter().map(|span| span.content.to_string()).collect::<Vec<_>>(),
        vec!["a", "bc", "d"]
    );
}
