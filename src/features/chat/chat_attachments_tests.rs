use super::*;

#[test]
fn attachments_parse_both_formats() {
    let (body, agent) = attachments(
        "see [Image #2]\n\nAttachments (read these files):\n- [Image #2]: /p/shot.png\n- /q/image-3.png\n- /q/notes.txt\n- [Pasted text #1 +3 lines]: /q/pasted-text-1.txt",
    );
    let message: Vec<(Option<&str>, &str)> = agent
        .iter()
        .map(|agent_value| (agent_value.marker.as_deref(), agent_value.path.as_str()))
        .collect();

    assert_eq!(body, "see [Image #2]\n\n");
    assert_eq!(
        message,
        [
            (Some("[Image #2]"), "/p/shot.png"),
            (Some("[Image #3]"), "/q/image-3.png"),
            (None, "/q/notes.txt"),
            (Some("[Pasted text #1 +3 lines]"), "/q/pasted-text-1.txt")
        ]
    );
    assert_eq!(agent[0].name(), "shot.png");
    assert_eq!(attachments("plain").1, []);
}

#[test]
fn attachments_link_their_markers_and_clip_rows_to_the_files() {
    let img = attached_file("links", "shot.png");
    let gone = img.with_file_name("gone.png");
    let msg = format!(
        "look at [Image #1] and [Image #2] here\n\nAttachments (read these files):\n- [Image #1]: {}\n- [Image #2]: {}",
        img.display(),
        gone.display()
    );
    let url = format!("file://{}", img.display());

    for look in [Look::Bubbles, Look::Lines] {
        let color = draw_attachment_thread(&msg, look);
        let text_2 = text(&color);

        verify_marker_links(&color, look, &url, &text_2);
        verify_clip_link(&color, look, &url, &text_2);
    }
}

fn draw_attachment_thread(msg: &str, look: Look) -> Drawn {
    let mut thread = Thread::default();

    thread.push(&ev(
        None,
        Kind::Prompt {
            from: "owner".into(),
            to: vec!["ada".into()],
            text: msg.to_string(),
        },
    ));

    draw(
        &thread,
        look,
        70,
        &Colors::new(),
        &[],
        &Fold {
            runs: &HashSet::new(),
            all_runs: false,
            thoughts: Thoughts::Hidden,
            open_thoughts: &HashSet::new(),
            all_thoughts: false,
        },
        &Picker::default(),
    )
}

fn verify_marker_links(color: &Drawn, look: Look, url: &str, text_2: &str) {
    // the marker of the existing file: a link over its cols
    let row = color
        .rows
        .iter()
        .find(|result_value| {
            result_value.links.iter().any(|line_value| line_value.2 == url) && !row_text(result_value).contains('📎')
        })
        .unwrap_or_else(|| panic!("{look:?}\n{text_2}"));

    let (agent, bytes, _) = row.links.iter().find(|line_2| line_2.2 == url).unwrap();
    let line = row_text(row);
    let at = line.find("[Image #1]").unwrap();

    assert_eq!(
        (*agent as usize, *bytes as usize),
        (line[..at].width(), line[..at].width() + 10),
        "{look:?}\n{text_2}"
    );

    // markdown splits the brackets off: the marker's pieces underlined, 0 other text
    let under: String = row
        .line
        .spans
        .iter()
        .filter(|text_3| text_3.style.add_modifier.contains(Modifier::UNDERLINED))
        .map(|text_4| text_4.content.as_ref())
        .collect();

    assert_eq!(under, "[Image #1]", "{look:?}");

    // the gone file: 0 link, `· missing`
    assert!(
        color
            .rows
            .iter()
            .all(|result_2| !result_2.links.iter().any(|line_3| line_3.2.contains("gone.png"))),
        "{look:?}"
    );
    assert!(text_2.contains("· missing"), "{look:?}\n{text_2}");
}

fn verify_clip_link(color: &Drawn, look: Look, url: &str, text_2: &str) {
    // the 📎 row: a link from the clip on
    let clip = color
        .rows
        .iter()
        .find(|result_3| row_text(result_3).contains("📎") && row_text(result_3).contains("shot.png"))
        .unwrap();
    let line = row_text(clip);
    let start = line.find('📎').map(|i| line[..i].width()).unwrap();

    assert!(
        clip.links
            .iter()
            .any(|(agent_value, _, user)| *user == url && *agent_value as usize == start),
        "{look:?}\n{text_2}"
    );
}

fn row_text(row: &Row) -> String {
    row.line.spans.iter().map(|span| span.content.as_ref()).collect()
}
