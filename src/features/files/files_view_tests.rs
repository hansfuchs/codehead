use super::*;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::KeyModifiers;

fn screen(view: &mut View) -> (Vec<String>, ratatui::buffer::Buffer) {
    let mut thread = Terminal::new(TestBackend::new(100, 16)).unwrap();

    thread.draw(|file| view.draw(file, file.area())).unwrap();
    let bytes = thread.backend().buffer().clone();

    (
        (0..16)
            .map(|y| {
                (0..40)
                    .map(|x| bytes[(x, y)].symbol().to_string())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect(),
        bytes,
    )
}

fn press(view: &mut View, keys: &str) {
    for character in keys.chars() {
        view.key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
    }
}

#[test]
fn letters_with_a_modifier_do_nothing_and_the_context_menu_opens_yazi_on_the_dir_without_files() {
    let mut view = View::new("fileskeys-0000", Path::new("/tmp/fileskeys"));

    assert_eq!(
        view.key(KeyEvent::new(
            KeyCode::Char('S'),
            KeyModifiers::ALT | KeyModifiers::SHIFT
        )),
        Act::None
    );

    for character in ['f', '/'] {
        assert_eq!(
            view.key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE)),
            Act::None
        );
        assert!(!view.filter_input.filtering, "{character}: 0 filter (ctrl+f is)");
    }

    assert_eq!(
        view.key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL)),
        Act::None
    );
    assert!(view.filter_input.filtering, "ctrl+f: the filter");

    view.filter_input.filtering = false;

    assert_eq!(
        view.key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE)),
        Act::None,
        "y: 0 key (yes only)"
    );
    assert_eq!(view.ctx_cmd("yazi"), Act::Yazi(PathBuf::from("/tmp/fileskeys")));
    assert_eq!(
        view.key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE)),
        Act::None,
        "0 file: 0 IDE"
    );
}

#[test]
fn attachments_get_their_own_section_last_once_per_file() {
    let _env = crate::engine::ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let directory = std::env::temp_dir().join(format!("codehead-fattach-{}", std::process::id()));
    let paste = directory.join("paste");
    let _ = std::fs::remove_dir_all(&directory);

    std::fs::create_dir_all(directory.join("src")).unwrap();
    std::fs::create_dir_all(&paste).unwrap();
    std::fs::write(directory.join("src/a.rs"), "fn a() {}").unwrap();
    std::fs::write(paste.join("image-1.png"), b"\x89PNG").unwrap();
    std::fs::write(paste.join("pasted-text-1.txt"), "long").unwrap();
    let thread = store::Thread::new("f", "t", &directory, vec!["ada".into()]);

    std::fs::create_dir_all(store::thread_dir(&thread.id)).unwrap();
    thread.save().unwrap();
    let (img, txt, code) = (
        paste.join("image-1.png"),
        paste.join("pasted-text-1.txt"),
        directory.join("src/a.rs"),
    );

    let prompt = |text: String| {
        store::Event::new(
            None,
            Kind::Prompt {
                from: "owner".into(),
                to: vec!["ada".into()],
                text,
            },
        )
    };

    attachments_get_their_own_section_last_once_per_file_verify_state(&directory, &thread, &img, &txt, &code, prompt);
}

fn attachments_get_their_own_section_last_once_per_file_verify_state(
    directory: &PathBuf,
    thread: &store::Thread,
    img: &PathBuf,
    txt: &Path,
    code: &PathBuf,
    prompt: impl Fn(String) -> store::Event,
) {
    store::append(
        &thread.id,
        &prompt(format!(
            "see [Image #1]\n\nAttachments (read these files):\n- [Image #1]: {}\n- {}",
            img.display(),
            code.display()
        )),
    )
    .unwrap();
    store::append(
        &thread.id,
        &prompt(format!(
            "again\n\nAttachments (read these files):\n- [Image #1]: {}\n- [Pasted text #1 +1 lines]: {}",
            img.display(),
            txt.display()
        )),
    )
    .unwrap();
    store::append(
        &thread.id,
        &store::Event::new(
            Some("ada"),
            Kind::Tool {
                id: "1".into(),
                name: "Write".into(),
                input: serde_json::json!({"file_path": code}),
            },
        ),
    )
    .unwrap();

    // the agent's file stays in its section; the 2 attachments once each, after it
    attachments_get_their_own_section_last_once_per_file_verify_files(directory, thread, img, code);
}

fn attachments_get_their_own_section_last_once_per_file_verify_files(
    directory: &PathBuf,
    thread: &store::Thread,
    img: &PathBuf,
    code: &PathBuf,
) {
    let files = load(&thread.id, directory);

    assert_eq!(files.len(), 3, "{files:?}");
    assert_eq!(
        files
            .iter()
            .find(|error_value| error_value.path == *code)
            .map(|error_2| &error_2.source),
        Some(&Source::Agent)
    );
    assert_eq!(
        files
            .iter()
            .find(|error_3| error_3.path == *img)
            .map(|error_4| &error_4.source),
        Some(&Source::Attached(Some("[Image #1]".into())))
    );

    // on screen: the section last, marker + name per row
    let mut view = View::new(&thread.id, directory);
    let (text_value, _) = screen(&mut view);

    let at = |text_2: &[String], x: &str| {
        text_2
            .iter()
            .position(|result| result.contains(x))
            .unwrap_or_else(|| panic!("{x}: {text_2:#?}"))
    };

    let (repo, att) = (at(&text_value, "repo changes"), at(&text_value, "attachments"));

    assert!(repo < att && at(&text_value, "src/a.rs") < att, "{text_value:#?}");
    assert!(
        at(&text_value, "[Image #1] image-1.png") > att && at(&text_value, "pasted-text-1.txt") > att,
        "{text_value:#?}"
    );

    let _ = std::fs::remove_dir_all(store::thread_dir(&thread.id));
    let _ = std::fs::remove_dir_all(directory);
}

/// A thread in a temp dir whose agent wrote NAMES (png: an image of 2000×1000 px).
fn image_thread(tag: &str, names: &[&str]) -> (store::Thread, PathBuf) {
    let directory = std::env::temp_dir().join(format!("codehead-fimage-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);

    std::fs::create_dir_all(&directory).unwrap();
    let thread = store::Thread::new("f", "t", &directory, vec!["ada".into()]);

    std::fs::create_dir_all(store::thread_dir(&thread.id)).unwrap();
    thread.save().unwrap();

    for (i, name) in names.iter().enumerate() {
        let path = directory.join(name);

        if std::path::Path::new(name)
            .extension()
            .is_some_and(|extension| extension == "png")
        {
            image::RgbaImage::from_pixel(2000, 1000, image::Rgba([10, 200, 10, 255]))
                .save(&path)
                .unwrap();
        } else {
            std::fs::write(&path, b"\x89PNG\0binary").unwrap();
        }

        store::append(
            &thread.id,
            &store::Event::new(
                Some("ada"),
                Kind::Tool {
                    id: i.to_string(),
                    name: "Write".into(),
                    input: serde_json::json!({"file_path": path}),
                },
            ),
        )
        .unwrap();
    }

    (thread, directory)
}

fn iterm2() -> crate::ui::graphics::Picker {
    crate::ui::graphics::Picker::new(
        ratatui_image::FontSize::new(10, 20),
        ratatui_image::picker::ProtocolType::Iterm2,
    )
}

/// All 100 cols (the `screen` helper: the list's 40).
fn full(view: &mut View) -> (Vec<String>, ratatui::buffer::Buffer) {
    let (_, bytes) = screen(view);
    let text: Vec<String> = (0..16)
        .map(|y| (0..100).map(|x| bytes[(x, y)].symbol().to_string()).collect::<String>())
        .collect();

    (text, bytes)
}

/// Polls until an encode is in (max 10 s).
fn encoded(view: &mut View) {
    let end = Instant::now() + Duration::from_secs(10);

    while !view.poll() {
        assert!(Instant::now() < end, "0 encode in 10 s");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn an_image_previews_as_an_image_encoded_off_the_ui_thread_once() {
    let _env = crate::engine::ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (thread, directory) = image_thread("once", &["shot.png"]);
    let mut view = View::new(&thread.id, &directory);

    view.image_cache.picker = || Some(iterm2());

    // 1st draw: the encode starts
    let (text, _) = full(&mut view);

    assert!(
        text.iter()
            .any(|result| result.contains("shot.png") && result.contains("reading…")),
        "{text:#?}"
    );

    // the encode in: the size line, 1 image (the iTerm2 sequence in the buffer)
    encoded(&mut view);
    let (text_value, bytes) = full(&mut view);

    assert!(
        text_value
            .iter()
            .any(|result_value| result_value.contains("2000×1000 px") && result_value.contains("ctrl+.: yazi")),
        "{text_value:#?}"
    );
    assert!(
        bytes
            .content()
            .iter()
            .any(|character| character.symbol().contains("\x1b]1337;File=")),
        "an iTerm2 image"
    );

    // a 2nd draw: from the cache, 0 new read
    screen(&mut view);

    assert_eq!(view.image_cache.image_reads.pending(), 0);
    assert_eq!(view.image_cache.images.len(), 1);

    // the image in: 0 wake-up per tick
    for _ in 0..3 {
        assert!(!view.poll());
    }

    // after yazi, before a draw (another pane): 0 wake-up per tick either
    view.forget_images();

    for _ in 0..3 {
        assert!(!view.poll());
    }

    // the next draw: encoded again, at once (0 wait for the 5 s read throttle)
    let (text_2, _) = full(&mut view);

    assert!(
        text_2.iter().any(|result_2| result_2.contains("reading…")),
        "{text_2:#?}"
    );
    assert_eq!(view.image_cache.image_reads.pending(), 1);

    let _ = std::fs::remove_dir_all(store::thread_dir(&thread.id));
    let _ = std::fs::remove_dir_all(&directory);
}

#[test]
fn an_image_without_a_graphics_terminal_keeps_the_text_preview() {
    let _env = crate::engine::ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (thread, directory) = image_thread("text", &["shot.png"]);
    let mut view = View::new(&thread.id, &directory);

    // the tests' default: 0 graphics terminal
    assert!((view.image_cache.picker)().is_none());

    let (text, _) = full(&mut view);

    // the head of the text preview (`N lines`), 0 encode
    assert!(
        text.iter()
            .any(|result| result.contains("shot.png") && result.contains("lines ·")),
        "{text:#?}"
    );
    assert!(
        !text
            .iter()
            .any(|result_value| result_value.contains("reading…") || result_value.contains(" px ·")),
        "{text:#?}"
    );
    assert_eq!(view.image_cache.image_reads.pending(), 0);

    let _ = std::fs::remove_dir_all(store::thread_dir(&thread.id));
    let _ = std::fs::remove_dir_all(&directory);
}

#[test]
fn at_most_4_encodes_stay_and_the_shown_one_is_never_dropped() {
    let _env = crate::engine::ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let names = ["a.png", "b.png", "c.png", "d.png", "e.png", "f.png"];
    let (thread, directory) = image_thread("lru", &names);
    let mut view = View::new(&thread.id, &directory);

    view.image_cache.picker = || Some(iterm2());
    full(&mut view);

    // each file in turn, its encode in
    for _ in names {
        full(&mut view);
        encoded(&mut view);
        full(&mut view);
        view.key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
    }

    // 4 kept; the one on screen among them
    assert_eq!(view.image_cache.images.len(), MAX_IMAGES);
    assert_eq!(view.image_cache.image_order.len(), MAX_IMAGES);

    let shown = view
        .image_cache
        .image_want
        .as_ref()
        .map(|(key_value, _)| key_value.clone())
        .unwrap();

    assert!(view.image_cache.images.contains_key(&shown));

    // a cached one shown again: the newest of the kept (LRU, not FIFO)
    view.key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE));
    full(&mut view);
    let back = view
        .image_cache
        .image_want
        .as_ref()
        .map(|(key_2, _)| key_2.clone())
        .unwrap();

    assert_ne!(back, shown);
    assert_eq!(view.image_cache.image_order.last(), Some(&back));
    assert_eq!(view.image_cache.image_reads.pending(), 0, "from the cache");

    // back to the 1st file: dropped, a new read at once
    view.key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE));

    for _ in names {
        view.key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE));
    }

    full(&mut view);

    assert_eq!(view.image_cache.image_reads.pending(), 1);

    let _ = std::fs::remove_dir_all(store::thread_dir(&thread.id));
    let _ = std::fs::remove_dir_all(&directory);
}

#[test]
fn sections_with_gaps_then_a_ranked_filter() {
    let _env = crate::engine::ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let directory = std::env::temp_dir().join(format!("codehead-fview-{}", std::process::id()));
    let other = std::env::temp_dir().join(format!("codehead-fother-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);

    std::fs::create_dir_all(directory.join("src")).unwrap();
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(directory.join("src/view.rs"), "fn draw() {}").unwrap();
    std::fs::write(directory.join("src/app.rs"), "// opens the view\nfn main() {}").unwrap();
    std::fs::write(other.join("scratch.txt"), "nothing").unwrap();
    let thread = store::Thread::new("f", "t", &directory, vec!["ada".into()]);

    std::fs::create_dir_all(store::thread_dir(&thread.id)).unwrap();
    thread.save().unwrap();

    for path in [
        directory.join("src/view.rs"),
        directory.join("src/app.rs"),
        other.join("scratch.txt"),
    ] {
        store::append(
            &thread.id,
            &store::Event::new(
                Some("ada"),
                Kind::Tool {
                    id: "1".into(),
                    name: "Write".into(),
                    input: serde_json::json!({"file_path": path}),
                },
            ),
        )
        .unwrap();
    }

    let mut view = View::new(&thread.id, &directory);
    let (text, _) = screen(&mut view);

    let at = |text_value: &[String], x: &str| {
        text_value
            .iter()
            .position(|result| result.contains(x))
            .unwrap_or_else(|| panic!("{x}: {text_value:#?}"))
    };

    sections_with_gaps_then_a_ranked_filter_verify_state(&directory, &other, &thread, view, &text, at);
}

fn sections_with_gaps_then_a_ranked_filter_verify_state(
    directory: &PathBuf,
    other: &PathBuf,
    thread: &store::Thread,
    mut view: View,
    text: &[String],
    at: impl Fn(&[String], &str) -> usize,
) {
    let (repo, others) = (at(text, "repo changes"), at(text, "other files"));

    assert!(
        at(text, "src/view.rs") > repo && at(text, "src/app.rs") > repo && at(text, "scratch.txt") > others,
        "{text:#?}"
    );
    assert_eq!(text[others - 1], "", "a blank row between sections: {text:#?}");

    // ctrl+f: the filter; `view`: the name match first, the content match after, 0 headings
    view.key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL));

    assert!(view.typing());

    press(&mut view, "view");
    let (text_2, bytes) = screen(&mut view);
    let (agent, character) = (at(&text_2, "src/view.rs"), at(&text_2, "src/app.rs"));

    assert!(
        agent < character
            && !text_2
                .iter()
                .any(|result_value| result_value.contains("repo changes") || result_value.contains("scratch")),
        "{text_2:#?}"
    );

    // the marked chars: `view` of view.rs
    let x = text_2[agent]
        .find("view.rs")
        .map(|i| text_2[agent][..i].chars().count())
        .unwrap() as u16;

    assert!(
        bytes[(x, agent as u16)]
            .modifier
            .contains(ratatui::style::Modifier::UNDERLINED),
        "marks"
    );

    // esc: the filter off, all files again
    view.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

    assert!(!view.typing());

    let (text_3, _) = screen(&mut view);

    at(&text_3, "scratch.txt");
    let _ = std::fs::remove_dir_all(store::thread_dir(&thread.id));
    let _ = std::fs::remove_dir_all(directory);
    let _ = std::fs::remove_dir_all(other);
}
