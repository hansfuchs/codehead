use super::*;

fn text(rows: &[Row]) -> Vec<String> {
    rows.iter()
        .map(|result| {
            result
                .line
                .spans
                .iter()
                .map(|text_value| text_value.content.as_ref())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect()
}

#[test]
fn html_tags_are_hidden_their_text_and_links_stay() {
    // a bot's table (Vercel): inline HTML in the cells
    let md = "| Project | Updated |\n| :--- | :----- |\n| <a href=\"https://v.com/p\"><sup><img src=\"https://v.com/a.png\" width=\"16\" alt=\"\" /></sup></a> [codehead](https://v.com/c) | <relative-time datetime=\"2026\">Nov 24, 2026 11:56am UTC</relative-time> |";
    let rows = markdown(md, 80, &[]);
    let thread = text(&rows).join("\n");

    assert!(!thread.contains('<') && !thread.contains("src="), "{thread}");
    assert!(
        thread.contains("codehead") && thread.contains("Nov 24, 2026 11:56am UTC"),
        "{thread}"
    );

    // an HTML block: tags out, <br> / </p> new rows, entities decoded, comments gone, <a> a link
    let md = "<!-- hidden -->\n<p align=\"center\">Hello &amp; <b>bye</b><br>next <a href=\"https://x.io\">site</a></p>\n\nafter";
    let rows = markdown(md, 80, &[]);
    let thread_value = text(&rows);

    assert_eq!(
        thread_value
            .iter()
            .filter(|line| !line.is_empty())
            .cloned()
            .collect::<Vec<_>>(),
        ["Hello & bye", "next site", "after"],
        "{thread_value:#?}"
    );
    assert!(
        rows.iter()
            .any(|result| result.links.iter().any(|line_value| line_value.2 == "https://x.io")),
        "the <a> text links"
    );

    // an image with alt text: as a Markdown image; a component name in prose: text
    let thread_2 = text(&markdown(
        "see <img src=\"x.png\" alt=\"logo\"> and the <Button> component",
        80,
        &[],
    ))
    .join("\n");

    assert_eq!(thread_2, "see [image: logo] and the <Button> component");
}

#[test]
fn empty_list_items_show_their_marker() {
    let thread = text(&markdown("1. a\n2. \n3. c", 40, &[]));

    assert_eq!(
        thread
            .iter()
            .filter(|line| !line.is_empty())
            .cloned()
            .collect::<Vec<_>>(),
        ["1. a", "2.", "3. c"],
        "{thread:#?}"
    );
}

#[test]
fn long_code_lines_wrap_at_word_ends_with_a_mark() {
    let md = "```md\n# HOME REPO\n- home repo: `~/projects/doinstruct/bonfire`. Its rules (notes, other repos): @~/projects/doinstruct/bonfire/AGENTS.md\n```";
    let rows = markdown(md, 50, &[]);
    let thread = text(&rows);

    // head row, the short line, the long line on several rows: 0 chars lost
    assert!(thread.len() > 3, "{thread:#?}");
    assert!(thread[2].starts_with(" - home repo:"), "{thread:#?}");
    assert!(
        thread[3..].iter().all(|line_value| line_value.starts_with(" ↪ ")),
        "{thread:#?}"
    );

    let joined: String = thread[2..]
        .iter()
        .flat_map(|line_2| line_2.trim_start().trim_start_matches("↪ ").chars())
        .filter(|character| !character.is_whitespace())
        .collect();

    assert!(joined.ends_with("@~/projects/doinstruct/bonfire/AGENTS.md"), "{joined}");

    // breaks after a space or a separator, 0 cut inside `repo:`
    assert!(
        thread[2].trim_end().ends_with(['.', ' ', '/', ',', ':', ')', '`'])
            || thread[2].trim_end().ends_with("bonfire"),
        "{thread:#?}"
    );

    // every row as wide as the block
    let width_value = rows[0].line.width();

    assert!(
        rows.iter().all(|result| result.line.width() == width_value),
        "{thread:#?}"
    );

    // the copy button copies the source lines, 0 marks
    assert_eq!(
        rows[0].copies[0].2,
        "# HOME REPO\n- home repo: `~/projects/doinstruct/bonfire`. Its rules (notes, other repos): @~/projects/doinstruct/bonfire/AGENTS.md"
    );
}

#[test]
fn wrap_code_breaks_hard_without_a_word_end() {
    let chars: Vec<(Color, char)> = "abcdefghijklmnopqrstuvwxyz"
        .chars()
        .map(|color| (Color::Reset, color))
        .collect();
    let segs = wrap_code(&chars, 10);

    assert_eq!(segs, [0..10, 10..18, 18..26]);
}

#[test]
fn inline_code_of_an_existing_file_is_a_file_link() {
    let directory = std::env::temp_dir().join(format!("codehead-filelink-{}", std::process::id()));

    std::fs::create_dir_all(&directory).unwrap();
    let file = directory.join("plan.md");

    std::fs::write(&file, "x").unwrap();
    let abs = file.display().to_string();

    assert_eq!(file_link(&abs), Some(format!("file://{abs}")));
    assert_eq!(file_link(&format!("{abs}:12")), Some(format!("file://{abs}#L12")));
    assert_eq!(file_link("cargo test"), None);
    assert_eq!(file_link(&directory.display().to_string()), None, "a dir: 0 link");
    assert_eq!(file_link("/no/such/file.rs"), None);

    let rows = markdown(&format!("see `{abs}:3` and `cargo test`"), 200, &[]);

    assert_eq!(rows[0].links.len(), 1);
    assert!(rows[0].links[0].2.ends_with("plan.md#L3"));
    assert!(
        rows[0].copies.iter().any(|character| character.2 == "cargo test"),
        "plain code still copies"
    );

    let _ = std::fs::remove_dir_all(directory);
}

#[test]
fn inline_code_has_a_background_links_do_not() {
    let rows = markdown("run `bun run link-branch` or see [docs](https://x.dev) now", 80, &[]);

    // per cell: (char, bg)
    let cells: Vec<(char, Option<Color>)> = rows[0]
        .line
        .spans
        .iter()
        .flat_map(|text_value| {
            text_value
                .content
                .chars()
                .map(move |character| (character, text_value.style.bg))
        })
        .collect();

    let text: String = cells.iter().map(|character_value| character_value.0).collect();
    let at = text.find("bun run link-branch").unwrap();
    let code = &cells[at..at + "bun run link-branch".len()];

    assert!(
        code.iter()
            .all(|character_2| character_2.1 == Some(theme::pal().surface0)),
        "every cell, spaces too: {code:?}"
    );
    assert_eq!(cells[at - 1].1, None, "0 bg before the code");

    let link = text.find("docs").unwrap();

    assert_eq!(cells[link].1, None, "a link: 0 bg");
}

#[test]
fn wraps_lists_code_and_links() {
    let md = "Some **bold** text that wraps here.\n\n- one\n- two\n\n```rust\nfn main() {}\n```\n\nsee [docs](https://x.dev) now";
    let rows = markdown(md, 20, &[Span::raw("  ")]);
    let thread = text(&rows);

    assert_eq!(thread[0], "  Some bold text");
    assert!(thread.contains(&"  • one".to_string()), "{thread:?}");
    assert!(
        thread.iter().any(|line_value| line_value.contains("fn main() {}")),
        "{thread:?}"
    );

    let link = rows.iter().find_map(|result| result.links.first().cloned()).unwrap();

    assert_eq!(link.2, "https://x.dev");
    assert!(
        rows.iter().all(|result_value| result_value.line.width() <= 20),
        "{thread:?}"
    );
}

#[test]
fn line_breaks_blank_lines_and_tables() {
    // a source line break stays, a list right below a line: 0 blank row
    let thread = text(&markdown("※ SUMMARY\nDONE. it works.\n\n✱ INFO\n- a\n- b", 40, &[]));

    assert_eq!(thread, vec!["※ SUMMARY", "DONE. it works.", "", "✱ INFO", "• a", "• b"]);

    let md = "| # | change | effect |\n|---|---|---|\n| 1 | a long change text here | short |";
    let rows = markdown(md, 30, &[Span::raw("  ")]);
    let thread_value = text(&rows);

    assert_eq!(thread_value[0], "  # │ change          │ effect");
    assert!(thread_value[1].starts_with("  ──┼─"), "{thread_value:?}");
    assert_eq!(thread_value[2], "  1 │ a long change   │ short");
    assert_eq!(thread_value[3], "    │ text here       │");
    assert!(rows.iter().all(|result| result.line.width() <= 30), "{thread_value:?}");
}

#[test]
fn glued_pieces_wrap_together() {
    let thread = text(&markdown("now uses `bun x:install`.", 20, &[]));

    assert_eq!(thread, vec!["now uses bun", "x:install."]);
}

#[test]
fn long_words_cut() {
    let rows = plain("abcdefghijklmnop", 6, theme::text(), &[]);

    assert_eq!(text(&rows), vec!["abcdef", "ghijkl", "mnop"]);
}
