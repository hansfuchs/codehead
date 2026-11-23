//! The terminal writer of every screen: 1 frame = 1 write, inside a synchronized update (DEC
//! mode 2026). Stdout alone (a 1 KB line buffer) sends a full frame in 1 KB writes: the terminal
//! paints between them, half the screen old, half new (a screen change, a full repaint). A
//! terminal without mode 2026 ignores it and still gets the frame in 1 write.

use std::io::{self, Stdout, Write};

use ratatui::backend::CrosstermBackend;

const BEGIN: &[u8] = b"\x1b[?2026h";
const END: &[u8] = b"\x1b[?2026l";

/// Collects a frame's bytes; `flush` (ratatui's, at the end of each draw) sends them in 1 write.
pub struct Frames<W: Write = Stdout> {
    buf: Vec<u8>,
    out: W,
}

impl<W: Write> Frames<W> {
    pub fn new(out: W) -> Self {
        let mut buf = Vec::with_capacity(64 << 10);
        buf.extend_from_slice(BEGIN);
        Frames { buf, out }
    }
}

impl<W: Write> Write for Frames<W> {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        self.buf.extend_from_slice(b);
        Ok(b.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.buf.len() > BEGIN.len() {
            self.buf.extend_from_slice(END);
            let sent = self.out.write_all(&self.buf);
            self.buf.truncate(BEGIN.len());
            // a big frame's buffer (an image) goes: 0 MBs kept for the session
            if self.buf.capacity() > 1 << 20 {
                self.buf.shrink_to(64 << 10);
            }
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
pub fn backend() -> CrosstermBackend<Frames> {
    CrosstermBackend::new(Frames::new(io::stdout()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_goes_in_1_write_inside_a_synchronized_update() {
        struct Log(Vec<Vec<u8>>);
        impl Write for &mut Log {
            fn write(&mut self, b: &[u8]) -> io::Result<usize> {
                self.0.push(b.to_vec());
                Ok(b.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let mut log = Log(Vec::new());
        {
            let mut f = Frames::new(&mut log);
            for _ in 0..100 {
                f.write_all(&[b'x'; 1000]).unwrap();
            }
            f.flush().unwrap();
            // 0 bytes since the last frame: 0 write
            f.flush().unwrap();
            f.write_all(b"y").unwrap();
        }
        assert_eq!(log.0.len(), 2, "1 write per frame, the rest at drop");
        assert!(log.0[0].starts_with(BEGIN) && log.0[0].ends_with(END));
        assert_eq!(log.0[0].len(), BEGIN.len() + 100_000 + END.len());
        assert_eq!(log.0[1], [BEGIN, b"y", END].concat());
    }
}
