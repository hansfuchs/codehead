use super::*;

#[test]
fn paste_rules() {
    let dir = std::env::temp_dir().join(format!("codehead-paste-{}", std::process::id()));

    std::fs::create_dir_all(&dir).unwrap();
    let img = dir.join("a b.png");

    std::fs::write(&img, b"\x89PNG").unwrap();
    let text = img.display().to_string();

    assert_eq!(dropped_file(&text), Some(img.clone()));
    assert_eq!(dropped_file(&format!("'{text}'")), Some(img.clone()));
    assert_eq!(dropped_file(&text.replace(' ', "\\ ")), Some(img.clone()));
    assert_eq!(dropped_file(&format!("file://{text}")), Some(img.clone()));
    assert_eq!(dropped_file("hello world"), None);
    assert!(is_image(&img));
    assert_eq!(text_marker(2, "a\nb\nc"), "[Pasted text #2 +3 lines]");

    let mut paster = Paster::new(dir.join("out"));

    assert_eq!(
        paster.paste(&text),
        Pasted::File {
            path: img.clone(),
            marker: "[Image #1]".into()
        }
    );
    assert_eq!(paster.paste("short"), Pasted::Text("short".into()));

    let long = "x\r".repeat(600);
    let code = "let a = 1;\n".repeat(200);

    assert_eq!(paster.paste(&code), Pasted::Text(code.clone()));

    match paster.paste(&long) {
        Pasted::File { path, marker } => {
            assert_eq!(marker, "[Pasted text #1 +600 lines]");
            assert!(std::fs::read_to_string(path).unwrap().starts_with("x\nx\n"));
        }
        other => panic!("{other:?}"),
    }

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_new_paster_numbers_after_the_files_and_markers_in_use() {
    let dir = std::env::temp_dir().join(format!("codehead-paste-again-{}", std::process::id()));

    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("image-1.png"), b"first").unwrap();
    std::fs::write(dir.join("image-3.png"), b"third").unwrap();
    std::fs::write(dir.join("pasted-text-2.txt"), b"x").unwrap();
    std::fs::write(dir.join("image-x.png"), b"x").unwrap();

    let mut paster = Paster::new(dir.clone());

    assert_eq!((paster.images, paster.texts), (3, 2));

    paster.seen("[Image #5]");
    paster.seen("[Pasted text #4 +9 lines]");
    paster.seen("[Image #2]");

    assert_eq!((paster.images, paster.texts), (5, 4));

    let long = "y\n".repeat(600);

    assert!(matches!(paster.paste(&long), Pasted::File { marker, .. } if marker == "[Pasted text #5 +600 lines]"));
    assert_eq!(std::fs::read(dir.join("image-1.png")).unwrap(), b"first");

    let _ = std::fs::remove_dir_all(&dir);
}
