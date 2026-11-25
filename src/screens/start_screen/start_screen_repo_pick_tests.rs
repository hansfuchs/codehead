use super::*;

fn key(picker: &mut Picker, code: KeyCode) -> Act {
    picker.key(KeyEvent::new(code, KeyModifiers::NONE))
}

#[test]
fn two_panes_move_in_out_filter_and_pick() {
    let base = std::env::temp_dir().join(format!("ch-repopick-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);

    for dir in ["alpha/.git", "alpha/src", "beta", "gamma/.git", ".hidden"] {
        std::fs::create_dir_all(base.join(dir)).unwrap();
    }

    let home = vec![crate::infra::home::Repo {
        name: "zeta".into(),
        path: base.join("zeta"),
        url: Some("git@x:y/zeta.git".into()),
    }];
    let mut picker = Picker::new(&base.join("beta"), home);
    let names = |entries: &[Entry]| entries.iter().map(|entry| entry.name.clone()).collect::<Vec<_>>();

    // left: the parent's dirs (0 hidden, the home clone too), the cursor on beta; right: beta's
    assert_eq!(names(&picker.parent), ["alpha", "beta", "gamma", "zeta"]);
    assert_eq!(picker.current().map(|entry| entry.name.as_str()), Some("beta"));
    assert_eq!(picker.kids, [] as [crate::screens::start_screen::repo_pick::Entry; 0]);

    key(&mut picker, KeyCode::Up);

    assert_eq!(names(&picker.kids), ["src"]);
    assert!(picker.current().unwrap().git);

    // →: into alpha (src current), ←: back up (alpha current)
    key(&mut picker, KeyCode::Right);

    assert_eq!(picker.current().map(|entry| entry.name.as_str()), Some("src"));

    key(&mut picker, KeyCode::Left);

    assert_eq!(picker.current().map(|entry| entry.name.as_str()), Some("alpha"));

    // type: filter, the cursor on the 1st match; esc clears it, the 2nd esc closes
    for letter in "gam".chars() {
        key(&mut picker, KeyCode::Char(letter));
    }

    two_panes_move_in_out_filter_and_pick_verify_drawn(base, picker);
}

fn two_panes_move_in_out_filter_and_pick_verify_drawn(base: PathBuf, mut picker: Picker) {
    assert_eq!(picker.current().map(|entry| entry.name.as_str()), Some("gamma"));
    assert_eq!(key(&mut picker, KeyCode::Esc), Act::None);
    assert_eq!(picker.current().map(|entry| entry.name.as_str()), Some("gamma"));

    // the home repo not on disk: enter picks it with its URL
    key(&mut picker, KeyCode::Down);

    let Act::Pick(home_entry) = key(&mut picker, KeyCode::Enter) else {
        panic!()
    };

    assert_eq!(
        (home_entry.name.as_str(), home_entry.clone.as_deref()),
        ("zeta", Some("git@x:y/zeta.git"))
    );
    assert_eq!(key(&mut picker, KeyCode::Esc), Act::Close);

    // drawn: both panes side by side
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 20)).unwrap();

    key(&mut picker, KeyCode::Up);

    terminal
        .draw(|frame| {
            picker.draw(
                frame,
                Rect {
                    x: 0,
                    y: 0,
                    width: 20,
                    height: 1,
                },
                frame.area(),
            );
        })
        .unwrap();

    let rows: Vec<String> = (0..20u16)
        .map(|y| {
            (0..80u16)
                .map(|x| terminal.backend().buffer()[(x, y)].symbol().to_string())
                .collect()
        })
        .collect();

    assert!(
        rows.iter().any(|line| line.contains("gamma/") && line.contains('│')),
        "{rows:#?}"
    );
    assert!(
        rows.iter().any(|line| line.contains("alpha/") && line.contains("git")),
        "{rows:#?}"
    );

    let _ = std::fs::remove_dir_all(base);
}
