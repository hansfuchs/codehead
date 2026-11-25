use super::*;

/// `cargo test -- --ignored wizard::look --nocapture`: every screen as text.
#[test]
#[ignore = "prints wizard screens for manual review"]
fn print_screens() {
    let mut width = Wizard::new(true);

    let show = |width_value: &Wizard| {
        let mut thread = ratatui::Terminal::new(ratatui::backend::TestBackend::new(84, 26)).unwrap();

        thread.draw(|file| width_value.draw(file)).unwrap();
        let rows: Vec<String> = thread
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|character| character.symbol().to_string())
            .collect::<Vec<_>>()
            .chunks(84)
            .map(|result| result.concat().trim_end().to_string())
            .collect();

        println!("{}\n{}", rows.join("\n"), "=".repeat(84));
    };

    for i in 0..SCREENS.len() {
        width.go(i);

        // the spawn over: the sprites, not the goo
        width
            .crew_state
            .crew
            .iter_mut()
            .for_each(|message| message.spawn.start = avatar::now() - 1000.0);
        show(&width);
    }
}
