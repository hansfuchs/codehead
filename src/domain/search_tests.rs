use super::*;

fn msgs() -> Vec<Msg> {
    vec![
        Msg::new("t1", "deploy", "owner", 1, "fix the deploy script please"),
        Msg::new(
            "t1",
            "deploy",
            "ada",
            2,
            "The deploy script runs again. Redeployed twice.",
        ),
        Msg::new("t2", "docs", "owner", 3, "update the docs for the deploy"),
        Msg::new("t2", "docs", "bob", 4, "nothing here"),
    ]
}

#[test]
fn every_word_must_match_best_first_then_oldest() {
    let message = msgs();

    // "deploy" at a word start in 3 messages; the 2nd has 2 hits
    assert_eq!(find(&message, "deploy"), vec![1, 0, 2]);
    assert_eq!(find(&message, "deploy docs"), vec![2]);

    // fuzzy inside 1 word
    assert_eq!(find(&message, "scrpt"), vec![0, 1]);
    assert!(find(&message, "zebra").is_empty() && find(&message, "  ").is_empty());

    // case: any
    assert_eq!(find(&message, "DEPLOY SCRIPT"), vec![1, 0]);
}

#[test]
fn the_snippet_centers_the_match_and_marks_it() {
    let long = format!("{} needle {}", "hay ".repeat(40), "hay ".repeat(40));
    let message = Msg::new("t", "x", "owner", 1, &long);
    let (text, marks) = snippet(&message, "needle", 30);

    assert_eq!(text.chars().count(), 30, "{text}");
    assert!(
        text.starts_with('…') && text.ends_with('…') && text.contains("needle"),
        "{text}"
    );

    let at = text.chars().collect::<Vec<_>>();

    assert_eq!(marks.iter().map(|&i| at[i]).collect::<String>(), "needle");

    // fuzzy marks
    let (text_value, marks) = snippet(&Msg::new("t", "x", "owner", 1, "run the script"), "scrpt", 40);
    let at = text_value.chars().collect::<Vec<_>>();

    assert_eq!(marks.iter().map(|&i| at[i]).collect::<String>(), "scrpt");
}

#[test]
fn a_jump_is_for_its_thread_once() {
    jump_to("tj", 42);

    assert_eq!(take_jump("other"), None);
    assert_eq!(take_jump("tj"), Some(42));
    assert_eq!(take_jump("tj"), None);
}
