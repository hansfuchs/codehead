use super::*;

#[test]
fn a_frame_goes_in_1_write_inside_a_synchronized_update() {
    struct Log(Vec<Vec<u8>>);

    impl Write for &mut Log {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.push(bytes.to_vec());
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    let mut log = Log(Vec::new());

    {
        let mut frame = Frames::new(&mut log);

        for _ in 0..100 {
            frame.write_all(&[b'x'; 1000]).unwrap();
        }

        frame.flush().unwrap();

        // 0 bytes since the last frame: 0 write
        frame.flush().unwrap();
        frame.write_all(b"y").unwrap();
    }

    assert_eq!(log.0.len(), 2, "1 write per frame, the rest at drop");
    assert!(log.0[0].starts_with(BEGIN) && log.0[0].ends_with(END));
    assert_eq!(log.0[0].len(), BEGIN.len() + 100_000 + END.len());
    assert_eq!(log.0[1], [BEGIN, b"y", END].concat());
}
