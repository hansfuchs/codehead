use super::*;

#[test]
fn name_and_restore_by_id() {
    let thread = "stash-test-0000";

    write(
        thread,
        &[Entry {
            id: "a1".into(),
            path: "x.rs".into(),
            lines: "3".into(),
            patch: "p".into(),
            ..Default::default()
        }],
    );

    assert_eq!(read(thread)[0].title(), "x.rs:3");

    set_name(thread, "a1", "\"retry loop\"\nmore").unwrap();

    assert_eq!(read(thread)[0].title(), "retry loop");
    assert!(set_name(thread, "zz", "x").is_err());

    restored(thread, "a1").unwrap();

    assert_eq!(read(thread), [] as [crate::domain::changes::stash::Entry; 0]);

    let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir(thread));
}
