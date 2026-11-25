use super::*;

#[test]
fn attachments_parse_both_formats() {
    let (body, files) = attachments(
        "see [Image #2]\n\nAttachments (read these files):\n- [Image #2]: /p/shot.png\n- /q/image-3.png\n- /q/notes.txt\n- [Pasted text #1 +3 lines]: /q/pasted-text-1.txt",
    );
    let parsed: Vec<(Option<&str>, &str)> = files
        .iter()
        .map(|file| (file.marker.as_deref(), file.path.as_str()))
        .collect();

    assert_eq!(body, "see [Image #2]\n\n");
    assert_eq!(
        parsed,
        [
            (Some("[Image #2]"), "/p/shot.png"),
            (Some("[Image #3]"), "/q/image-3.png"),
            (None, "/q/notes.txt"),
            (Some("[Pasted text #1 +3 lines]"), "/q/pasted-text-1.txt")
        ]
    );
    assert_eq!(files[0].name(), "shot.png");
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
        let drawn = draw_attachment_thread(&msg, look);
        let dump = text(&drawn);

        verify_marker_links(&drawn, look, &url, &dump);
        verify_clip_link(&drawn, look, &url, &dump);
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

fn verify_marker_links(drawn: &Drawn, look: Look, url: &str, dump: &str) {
    // the marker of the existing file: a link over its cols
    let row = drawn
        .rows
        .iter()
        .find(|row| row.links.iter().any(|link| link.2 == url) && !row_text(row).contains('📎'))
        .unwrap_or_else(|| panic!("{look:?}\n{dump}"));

    let (start, end, _) = row.links.iter().find(|link| link.2 == url).unwrap();
    let line = row_text(row);
    let at = line.find("[Image #1]").unwrap();

    assert_eq!(
        (*start as usize, *end as usize),
        (line[..at].width(), line[..at].width() + 10),
        "{look:?}\n{dump}"
    );

    // markdown splits the brackets off: the marker's pieces underlined, 0 other text
    let under: String = row
        .line
        .spans
        .iter()
        .filter(|span| span.style.add_modifier.contains(Modifier::UNDERLINED))
        .map(|span| span.content.as_ref())
        .collect();

    assert_eq!(under, "[Image #1]", "{look:?}");

    // the gone file: 0 link, `· missing`
    assert!(
        drawn
            .rows
            .iter()
            .all(|row| !row.links.iter().any(|link| link.2.contains("gone.png"))),
        "{look:?}"
    );
    assert!(dump.contains("· missing"), "{look:?}\n{dump}");
}

fn verify_clip_link(drawn: &Drawn, look: Look, url: &str, dump: &str) {
    // the 📎 row: a link from the clip on
    let clip = drawn
        .rows
        .iter()
        .find(|row| row_text(row).contains("📎") && row_text(row).contains("shot.png"))
        .unwrap();
    let line = row_text(clip);
    let start = line.find('📎').map(|i| line[..i].width()).unwrap();

    assert!(
        clip.links
            .iter()
            .any(|(from, _, link)| *link == url && *from as usize == start),
        "{look:?}\n{dump}"
    );
}

fn row_text(row: &Row) -> String {
    row.line.spans.iter().map(|span| span.content.as_ref()).collect()
}
