use super::{Duration, Instant, Msg, Path, Value, json};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};

pub(super) struct Unsub(std::sync::Arc<AtomicBool>);
impl Drop for Unsub {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

pub(super) fn subscribe(id: &str, tx: crate::infra::bus::Tx<Msg>) -> Unsub {
    let stop = std::sync::Arc::new(AtomicBool::new(false));
    let done = stop.clone();
    let path = crate::engine::host::socket(id);
    let born = Instant::now();

    std::thread::spawn(move || run_subscription(&tx, &done, &path, born));

    Unsub(stop)
}

fn run_subscription(tx: &crate::infra::bus::Tx<Msg>, done: &AtomicBool, path: &Path, born: Instant) {
    while !done.load(Ordering::Relaxed) && subscription_step(tx, done, path, born) {}
}

fn subscription_step(tx: &crate::infra::bus::Tx<Msg>, done: &AtomicBool, path: &Path, born: Instant) -> bool {
    let Ok(stream) = open_subscription(path) else {
        std::thread::sleep(if born.elapsed() < Duration::from_secs(3) {
            Duration::from_millis(20)
        } else {
            Duration::from_secs(1)
        });
        return true;
    };

    read_subscription(stream, tx, done)
}

fn open_subscription(path: &Path) -> std::io::Result<UnixStream> {
    let mut stream = UnixStream::connect(path)?;

    stream.set_read_timeout(Some(Duration::from_secs(1)))?;
    writeln!(stream, "{}", json!({"op":"subscribe"}))?;
    Ok(stream)
}

fn read_subscription(stream: UnixStream, tx: &crate::infra::bus::Tx<Msg>, done: &AtomicBool) -> bool {
    if tx.send(Msg::Subscribed).is_err() {
        return false;
    }

    let mut reader = BufReader::new(stream);
    let mut line = String::new();

    while !done.load(Ordering::Relaxed) {
        match read_live_line(&mut reader, &mut line, tx) {
            ReadState::Continue => {}
            ReadState::Reconnect => return true,
            ReadState::Stop => return false,
        }
    }

    true
}

enum ReadState {
    Continue,
    Reconnect,
    Stop,
}

fn read_live_line(reader: &mut BufReader<UnixStream>, line: &mut String, tx: &crate::infra::bus::Tx<Msg>) -> ReadState {
    match reader.read_line(line) {
        Ok(0) => ReadState::Reconnect,
        Ok(_) => {
            let message = live_message(line.trim());

            line.clear();

            if forward_live(message, tx) {
                ReadState::Continue
            } else {
                ReadState::Stop
            }
        }
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
            ) =>
        {
            ReadState::Continue
        }
        Err(_) => ReadState::Reconnect,
    }
}

fn forward_live(message: Option<Msg>, tx: &crate::infra::bus::Tx<Msg>) -> bool {
    let Some(message) = message else {
        return true;
    };

    if tx.send(message).is_err() {
        return false;
    }

    crate::infra::bus::wake();
    true
}

fn live_message(line: &str) -> Option<Msg> {
    let value: Value = serde_json::from_str(line).unwrap_or_default();

    match value["op"].as_str() {
        Some("wake") => Some(Msg::Wake),
        Some("delta") => Some(Msg::Delta(
            value["agent"].as_str().unwrap_or_default().into(),
            value["text"].as_str().unwrap_or_default().into(),
        )),
        Some("live") => Some(Msg::Live(
            value["agent"].as_str().unwrap_or_default().into(),
            value["text"].as_str().unwrap_or_default().into(),
        )),
        _ => None,
    }
}
