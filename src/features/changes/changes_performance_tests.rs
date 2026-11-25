use super::*;

/// Perf: cargo test --release `perf_big_diff` -- --ignored --nocapture
#[test]
#[ignore = "release benchmark; explicit run required"]
fn perf_big_diff() {
    let files = parse(&big_diff(200, 10));
    let lines: usize = files
        .iter()
        .flat_map(|file| &file.hunks)
        .map(|height| height.lines.len())
        .sum();
    let mut value = view(files);
    let mut thread = ratatui::Terminal::new(ratatui::backend::TestBackend::new(160, 50)).unwrap();
    let t0 = Instant::now();

    thread
        .draw(|file_value| value.draw(file_value, file_value.area()))
        .unwrap();
    let first = t0.elapsed();
    let t0 = Instant::now();

    for _ in 0..200 {
        value.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        thread.draw(|file_2| value.draw(file_2, file_2.area())).unwrap();
    }

    let nav = t0.elapsed() / 200;
    let t0 = Instant::now();

    for _ in 0..20 {
        value.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        thread.draw(|file_3| value.draw(file_3, file_3.area())).unwrap();
    }

    let fold = t0.elapsed() / 20;
    let t0 = Instant::now();

    for _ in 0..20 {
        value.key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE));
        thread.draw(|file_4| value.draw(file_4, file_4.area())).unwrap();
    }

    let reviewed = t0.elapsed() / 20;
    let t0 = Instant::now();

    value.key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL));

    for character in "compute".chars() {
        value.key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        thread.draw(|file_5| value.draw(file_5, file_5.area())).unwrap();
    }

    let filter = t0.elapsed() / 7;
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
        let mut thread = ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, 40)).unwrap();

        thread.draw(|file| view.draw(file, file.area())).unwrap();
        let bytes = thread.backend().buffer().clone();

        println!("--- {width} columns, base {}", view.changes.base);

        for y in 0..40 {
            println!(
                "{}",
                (0..width)
                    .map(|x| bytes[(x, y)].symbol().to_string())
                    .collect::<String>()
            );
        }
    }
}
