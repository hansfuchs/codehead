use super::*;

fn key(path: &mut Picker, character: KeyCode) -> Act {
    path.key(KeyEvent::new(character, KeyModifiers::NONE))
}

#[test]
fn two_panes_move_in_out_filter_and_pick() {
    let base = std::env::temp_dir().join(format!("ch-repopick-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);

    for directory in ["alpha/.git", "alpha/src", "beta", "gamma/.git", ".hidden"] {
        std::fs::create_dir_all(base.join(directory)).unwrap();
    }

    let home = vec![crate::infra::home::Repo {
        name: "zeta".into(),
        path: base.join("zeta"),
        url: Some("git@x:y/zeta.git".into()),
    }];
    let mut path = Picker::new(&base.join("beta"), home);
    let names = |value: &[Entry]| value.iter().map(|error| error.name.clone()).collect::<Vec<_>>();

    // left: the parent's dirs (0 hidden, the home clone too), the cursor on beta; right: beta's
    assert_eq!(names(&path.parent), ["alpha", "beta", "gamma", "zeta"]);
    assert_eq!(
        path.current().map(|error_value| error_value.name.as_str()),
        Some("beta")
    );
    assert_eq!(path.kids, [] as [crate::screens::start_screen::repo_pick::Entry; 0]);

    key(&mut path, KeyCode::Up);

    assert_eq!(names(&path.kids), ["src"]);
    assert!(path.current().unwrap().git);

    // →: into alpha (src current), ←: back up (alpha current)
    key(&mut path, KeyCode::Right);

    assert_eq!(path.current().map(|error_2| error_2.name.as_str()), Some("src"));

    key(&mut path, KeyCode::Left);

    assert_eq!(path.current().map(|error_3| error_3.name.as_str()), Some("alpha"));

    // type: filter, the cursor on the 1st match; esc clears it, the 2nd esc closes
    for character in "gam".chars() {
        key(&mut path, KeyCode::Char(character));
    }

    two_panes_move_in_out_filter_and_pick_verify_thread(base, path);
}

fn two_panes_move_in_out_filter_and_pick_verify_thread(base: PathBuf, mut path: Picker) {
    assert_eq!(path.current().map(|error_4| error_4.name.as_str()), Some("gamma"));
    assert_eq!(key(&mut path, KeyCode::Esc), Act::None);
    assert_eq!(path.current().map(|error_5| error_5.name.as_str()), Some("gamma"));

    // the home repo not on disk: enter picks it with its URL
    key(&mut path, KeyCode::Down);

    let Act::Pick(error_6) = key(&mut path, KeyCode::Enter) else {
        panic!()
    };

    assert_eq!(
        (error_6.name.as_str(), error_6.clone.as_deref()),
        ("zeta", Some("git@x:y/zeta.git"))
    );
    assert_eq!(key(&mut path, KeyCode::Esc), Act::Close);

    // drawn: both panes side by side
    let mut thread = ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 20)).unwrap();

    key(&mut path, KeyCode::Up);

    thread
        .draw(|area| {
            path.draw(
                area,
                Rect {
                    x: 0,
                    y: 0,
                    width: 20,
                    height: 1,
                },
                area.area(),
            );
        })
        .unwrap();

    let rows: Vec<String> = (0..20u16)
        .map(|y| {
            (0..80u16)
                .map(|x| thread.backend().buffer()[(x, y)].symbol().to_string())
                .collect()
        })
        .collect();

    assert!(
        rows.iter().any(|line| line.contains("gamma/") && line.contains('│')),
        "{rows:#?}"
    );
    assert!(
        rows.iter()
            .any(|line_value| line_value.contains("alpha/") && line_value.contains("git")),
        "{rows:#?}"
    );

    let _ = std::fs::remove_dir_all(base);
}
