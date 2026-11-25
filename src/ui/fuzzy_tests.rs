use super::*;

#[test]
fn substring_beats_subsequence_and_word_start_beats_middle() {
    let (agent, pa) = word("view", "src/view.rs").unwrap();
    let (bytes, _) = word("vw", "src/view.rs").unwrap();

    assert!(agent > bytes);
    assert_eq!(pa, vec![4, 5, 6, 7]);

    let (character, _) = word("rs", "src/rs.rs").unwrap();
    let (directory, _) = word("rs", "src/users.txt").unwrap();

    assert!(character > directory);
    assert!(word("xyz", "src/view.rs").is_none());
}

#[test]
fn every_word_must_match_and_name_hits_rank_first() {
    let files = [
        ("src/app.rs", "fn main() { view }"),
        ("src/view.rs", "fn draw()"),
        ("README.md", "nothing"),
    ];
    let result = rank(
        &files,
        "view",
        |file| file.0.to_string(),
        |file_value| file_value.1.to_string(),
    );

    assert_eq!(result.iter().map(|x| x.0).collect::<Vec<_>>(), vec![1, 0]);
    assert_eq!(result[0].1, vec![4, 5, 6, 7]);
    assert_eq!(
        rank(
            &files,
            "view nothing",
            |file_2| file_2.0.to_string(),
            |file_3| file_3.1.to_string()
        ),
        [] as [(usize, std::vec::Vec<usize>); 0]
    );
    assert_eq!(
        rank(
            &files,
            " ",
            |file_4| file_4.0.to_string(),
            |file_5| file_5.1.to_string()
        )
        .len(),
        3
    );
}

#[test]
fn spans_mark_the_hits() {
    let style = spans("abcd", &[1, 2], Style::default(), Style::default().bold());

    assert_eq!(
        style.iter().map(|text| text.content.to_string()).collect::<Vec<_>>(),
        vec!["a", "bc", "d"]
    );
}
