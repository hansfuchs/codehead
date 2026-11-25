use super::*;

#[test]
fn mentions_get_the_agent_color_in_every_message_and_both_looks() {
    let mut thread = Thread::default();

    thread.push(&ev(
        None,
        Kind::Prompt {
            from: "owner".into(),
            to: vec!["ada".into()],
            text: "@ada continue, cc @bob".into(),
        },
    ));
    thread.push(&ev(
        Some("ada"),
        Kind::Text {
            text: "asked **@bob** here; `@bob` in code stays; mail@bob too\n\n```\n@bob in a block\n```".into(),
        },
    ));
    thread.push(&ev(
        Some("ada"),
        Kind::Prompt {
            from: "ada".into(),
            to: vec!["bob".into()],
            text: "@bob take the build".into(),
        },
    ));
    thread.push(&ev(
        Some("bob"),
        Kind::Status {
            state: "blocked".into(),
            note: "waits on @ada".into(),
        },
    ));
    let colors: Colors = [
        ("ada".to_string(), Color::Rgb(200, 10, 10)),
        ("bob".to_string(), Color::Rgb(10, 10, 200)),
    ]
    .into();
    let none = HashSet::new();
    let fold = Fold {
        runs: &none,
        all_runs: false,
        thoughts: Thoughts::Hidden,
        open_thoughts: &none,
        all_thoughts: false,
    };
    let ada = Some(Color::Rgb(200, 10, 10));
    let bob = Some(Color::Rgb(10, 10, 200));

    mentions_get_the_agent_color_in_every_message_and_both_looks_verify_directory(&thread, &colors, fold, ada, bob);
}

fn mentions_get_the_agent_color_in_every_message_and_both_looks_verify_directory(
    thread: &Thread,
    colors: &HashMap<String, Color>,
    fold: Fold<'_>,
    ada: Option<Color>,
    bob: Option<Color>,
) {
    for look in [Look::Bubbles, Look::Lines] {
        verify_mentions_in_look(thread, colors, fold, ada, bob, look);
    }

    // an unknown name: 0 color
    let spans = markdown::mention_spans("@zed and @ada", theme::text(), &[("ada".into(), Color::Red)]);

    assert_eq!(
        spans
            .iter()
            .filter(|color_value| color_value.style.fg == Some(Color::Red))
            .map(|text_value| text_value.content.as_ref())
            .collect::<Vec<_>>(),
        ["@ada"]
    );
}

fn verify_mentions_in_look(
    thread: &Thread,
    colors: &HashMap<String, Color>,
    fold: Fold<'_>,
    ada: Option<Color>,
    bob: Option<Color>,
    look: Look,
) {
    let directory = draw(thread, look, 70, colors, &[], &fold, &Picker::default());
    let rows = cells(&directory, 70);
    let all = |fg: Vec<Option<Color>>, color: Option<Color>| fg.iter().all(|x| *x == color);

    // the owner's bubble
    assert!(all(mention_fg(&rows, "ada", 0), ada), "{look:?} owner @ada");

    let owner = rows
        .iter()
        .position(|result| result.0.contains("@ada continue"))
        .unwrap();

    assert!(all(mention_fg(&rows, "bob", owner), bob), "{look:?} owner @bob");

    // agent text: bold markdown too; inline code, a mail address, a code block: plain
    let text = rows
        .iter()
        .position(|result_value| result_value.0.contains("asked"))
        .unwrap();

    assert!(all(mention_fg(&rows, "bob", text), bob), "{look:?} text @bob");

    let code_row = &rows[text..]
        .iter()
        .find(|result_2| result_2.0.contains("in code"))
        .unwrap();
    let x = code_row.0[..code_row
        .0
        .find("`@bob`")
        .map_or_else(|| code_row.0.find("@bob in code").unwrap(), |bytes| bytes + 1)]
        .chars()
        .count();

    assert_ne!(code_row.1[x], bob, "{look:?} inline code: 0 mention color");

    let mail = rows.iter().find(|result_3| result_3.0.contains("mail@bob")).unwrap();
    let x = mail.0[..mail.0.find("mail@bob").unwrap()].chars().count() + 4;

    assert_ne!(mail.1[x], bob, "{look:?} mail@bob: 0 mention");

    let block = rows
        .iter()
        .find(|result_4| result_4.0.contains("@bob in a block"))
        .unwrap();
    let x = block.0[..block.0.find("@bob in a block").unwrap()].chars().count();

    assert_ne!(block.1[x], bob, "{look:?} code block: 0 mention");

    // agent → agent message, a status note
    let send = rows
        .iter()
        .position(|result_5| result_5.0.contains("take the build"))
        .unwrap();

    assert!(all(mention_fg(&rows, "bob", send), bob), "{look:?} agent message");

    let note = rows
        .iter()
        .position(|result_6| result_6.0.contains("waits on"))
        .unwrap();

    assert!(all(mention_fg(&rows, "ada", note), ada), "{look:?} note");
}
