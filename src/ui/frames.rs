//! The terminal writer of every screen: 1 frame = 1 write, inside a synchronized update (DEC
//! mode 2026). Stdout alone (a 1 KB line buffer) sends a full frame in 1 KB writes: the terminal
//! paints between them, half the screen old, half new (a screen change, a full repaint). A
//! terminal without mode 2026 ignores it and still gets the frame in 1 write.

use std::io::{self, Stdout, Write};

use ratatui::backend::CrosstermBackend;

const BEGIN: &[u8] = b"\x1b[?2026h";
const END: &[u8] = b"\x1b[?2026l";

/// Collects a frame's bytes; `flush` (ratatui's, at the end of each draw) sends them in 1 write.
pub(crate) struct Frames<W: Write = Stdout> {
    buf: Vec<u8>,
    out: W,
}

impl<W: Write> Frames<W> {
    pub(crate) fn new(out: W) -> Self {
        let mut buf = Vec::with_capacity(64 << 10);

        buf.extend_from_slice(BEGIN);
        Frames { buf, out }
    }
}

impl<W: Write> Write for Frames<W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.buf.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.buf.len() > BEGIN.len() {
            self.buf.extend_from_slice(END);
            let sent = self.out.write_all(&self.buf);

            release_buffer(&mut self.buf);

            sent?;
        }

        self.out.flush()
    }
}

impl<W: Write> Drop for Frames<W> {
    fn drop(&mut self) {
        let _ = self.flush();
    }
}

/// The backend of a screen on stdout.
pub(crate) fn backend() -> CrosstermBackend<Frames> {
    CrosstermBackend::new(Frames::new(io::stdout()))
}

#[cfg(test)]
#[path = "frames_tests.rs"]
mod tests;

fn release_buffer(buffer: &mut Vec<u8>) {
    buffer.truncate(BEGIN.len());

    // a big frame's buffer (an image) goes: 0 MBs kept for the session
    if buffer.capacity() > 1 << 20 {
        buffer.shrink_to(64 << 10);
    }
}
