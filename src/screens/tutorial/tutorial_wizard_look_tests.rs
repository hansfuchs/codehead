use super::*;

/// `cargo test -- --ignored wizard::look --nocapture`: every screen as text.
#[test]
#[ignore = "prints wizard screens for manual review"]
fn print_screens() {
    let mut wizard = Wizard::new(true);

    let show = |wizard: &Wizard| {
        let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(84, 26)).unwrap();

        terminal.draw(|frame| wizard.draw(frame)).unwrap();
        let rows: Vec<String> = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol().to_string())
            .collect::<Vec<_>>()
            .chunks(84)
            .map(|row| row.concat().trim_end().to_string())
            .collect();

        println!("{}\n{}", rows.join("\n"), "=".repeat(84));
    };

    for i in 0..SCREENS.len() {
        wizard.go(i);

        // the spawn over: the sprites, not the goo
        wizard
            .crew_state
            .crew
            .iter_mut()
            .for_each(|member| member.spawn.start = avatar::now() - 1000.0);
        show(&wizard);
    }
}
