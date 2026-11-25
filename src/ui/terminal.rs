use ratatui::crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture, KeyboardEnhancementFlags,
    PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use ratatui::crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::crossterm::{cursor, execute};
use std::io::{self, Stdout, Write};

pub(crate) struct Tty {
    pub(crate) enhanced: bool,
}

impl Tty {
    pub(crate) fn enter(&mut self, out: &mut Stdout) -> io::Result<()> {
        terminal::enable_raw_mode()?;

        // the terminal's palette (agent border colors), before crossterm reads the tty
        crate::ui::theme::query_palette();
        execute!(out, EnterAlternateScreen, EnableMouseCapture, EnableBracketedPaste)?;

        // always on, also when the support query got 0 answer in time: without it shift+enter
        // comes as `ESC[27;2;13~`, which crossterm drops. Terminals without it ignore the request.
        execute!(
            out,
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        )?;
        self.enhanced = true;

        Ok(())
    }

    /// Input modes off, alternate screen kept (the next screen takes it over: no flash).
    pub(crate) fn release(&mut self, out: &mut Stdout) {
        if self.enhanced {
            let _ = execute!(out, PopKeyboardEnhancementFlags);

            self.enhanced = false;
        }

        let _ = execute!(out, DisableMouseCapture, DisableBracketedPaste, cursor::Show);
        let _ = terminal::disable_raw_mode();
    }

    pub(crate) fn leave(&mut self, out: &mut Stdout) {
        self.release(out);
        let _ = execute!(out, LeaveAlternateScreen);

        if let Some(hint) = crate::infra::update::hint() {
            // not eprintln: on a closed terminal it panics
            let _ = writeln!(io::stderr(), "{hint}");
        }
    }
}
