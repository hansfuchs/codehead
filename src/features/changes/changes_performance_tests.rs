use super::*;

/// Perf: cargo test --release `perf_big_diff` -- --ignored --nocapture
#[test]
#[ignore = "release benchmark; explicit run required"]
fn perf_big_diff() {
    let files = parse(&big_diff(200, 10));
    let lines: usize = files
        .iter()
        .flat_map(|file| &file.hunks)
        .map(|hunk| hunk.lines.len())
        .sum();
    let mut view = view(files);
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(160, 50)).unwrap();
    let started = Instant::now();

    terminal.draw(|frame| view.draw(frame, frame.area())).unwrap();
    let first = started.elapsed();
    let started = Instant::now();

    for _ in 0..200 {
        view.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        terminal.draw(|frame| view.draw(frame, frame.area())).unwrap();
    }

    let nav = started.elapsed() / 200;
    let started = Instant::now();

    for _ in 0..20 {
        view.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        terminal.draw(|frame| view.draw(frame, frame.area())).unwrap();
    }

    let fold = started.elapsed() / 20;
    let started = Instant::now();

    for _ in 0..20 {
        view.key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE));
        terminal.draw(|frame| view.draw(frame, frame.area())).unwrap();
    }

    let reviewed = started.elapsed() / 20;
    let started = Instant::now();

    view.key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL));

    for typed in "compute".chars() {
        view.key(KeyEvent::new(KeyCode::Char(typed), KeyModifiers::NONE));
        terminal.draw(|frame| view.draw(frame, frame.area())).unwrap();
    }

    let filter = started.elapsed() / 7;
    let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir("changes-test-0000"));

    println!(
        "perf: 200 files, {lines} diff lines · first draw {first:?} · 1 row down + draw {nav:?} · fold toggle + draw {fold:?} · r (reviewed) + draw {reviewed:?} · filter key + draw {filter:?}"
    );
}

/// Live look: `CODEHEAD_CHANGES_DIR`=<repo> cargo test `changes_live` -- --ignored --nocapture
#[test]
#[ignore = "live interaction; explicit run required"]
fn changes_live() {
    let dir = PathBuf::from(std::env::var("CODEHEAD_CHANGES_DIR").unwrap());

    for width in [120u16, 180] {
        let mut view = View::new("changes-live-0000", &dir);

        view.changes = load(&dir).unwrap();
        view.loaded = Some(Instant::now());
        let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, 40)).unwrap();

        terminal.draw(|frame| view.draw(frame, frame.area())).unwrap();
        let buffer = terminal.backend().buffer().clone();

        println!("--- {width} columns, base {}", view.changes.base);

        for y in 0..40 {
            println!(
                "{}",
                (0..width)
                    .map(|x| buffer[(x, y)].symbol().to_string())
                    .collect::<String>()
            );
        }
    }
}
