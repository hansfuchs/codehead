use super::*;
use std::ops::ControlFlow;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;

/// A fake host at DIR/ID.sock: it answers `stop`; QUITS: it then ends (its socket goes).
fn fake_host(dir: &Path, id: &str, quits: bool) -> std::thread::JoinHandle<()> {
    let path = dir.join(format!("{id}.sock"));
    let listener = UnixListener::bind(&path).unwrap();

    std::thread::spawn(move || serve_requests(quits, &path, &listener))
}

fn serve_requests(quits: bool, path: &PathBuf, listener: &UnixListener) {
    for stream in listener.incoming().flatten() {
        let mut line = String::new();

        if let ControlFlow::Break(()) = read_request(&stream, &mut line) {
            continue;
        }

        let _ = writeln!(&stream, "{}", serde_json::json!({"ok": true}));

        if let ControlFlow::Break(()) = quit_on_stop(quits, path, &line) {
            return;
        }
    }
}

fn quit_on_stop(quits: bool, path: &PathBuf, line: &str) -> ControlFlow<()> {
    if quits && line.contains("stop") {
        let _ = std::fs::remove_file(path);

        return ControlFlow::Break(());
    }

    ControlFlow::Continue(())
}

fn read_request(stream: &UnixStream, line: &mut String) -> ControlFlow<()> {
    if BufReader::new(stream).read_line(line).unwrap_or(0) == 0 {
        return ControlFlow::Break(());
    }

    ControlFlow::Continue(())
}

/// Only hosts that serve get the stop; one that ends: stopped, one that stays: its error.
#[test]
fn kill_hosts_stops_every_serving_host_and_names_the_ones_that_stay() {
    let dir = PathBuf::from(format!("/tmp/chk-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    std::fs::create_dir_all(&dir).unwrap();

    // a socket file of a host that is gone
    std::fs::write(dir.join("gone.sock"), "").unwrap();
    let quitting = fake_host(&dir, "aaa", true);
    let staying = fake_host(&dir, "bbb", false);

    assert_eq!(hosts_in(&dir), vec!["aaa".to_string(), "bbb".to_string()]);

    let out = stop_all_in(&dir, Duration::from_millis(500));

    assert_eq!(out.len(), 2, "{out:?}");
    assert_eq!(out[0], ("aaa".to_string(), None));
    assert!(
        out[1].0 == "bbb" && out[1].1.as_deref().is_some_and(|error| error.starts_with("still runs")),
        "{out:?}"
    );

    quitting.join().unwrap();

    assert_eq!(hosts_in(&dir), vec!["bbb".to_string()]);

    drop(staying);

    let _ = std::fs::remove_dir_all(&dir);
}
