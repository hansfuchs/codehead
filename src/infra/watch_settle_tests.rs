use super::*;

/// The times (ms after START) at which the topic of 1 work tree goes out, for events at EVENTS
/// ms; the loop wakes at each deadline, as the watcher thread does.
fn published(events: &[u64]) -> Vec<u64> {
    let start = Instant::now();
    let at = |ms: u64| start + Duration::from_millis(ms);
    let mut settle = Settle::default();
    let mut out = Vec::new();
    let mut pending = events.iter().copied().peekable();

    loop {
        let wake = settle.next();
        let event = pending.peek().copied().map(at);

        let now = match (wake, event) {
            (None, None) => return out,
            (Some(wake), Some(event)) if wake < event => wake,
            (Some(wake), None) => wake,
            (_, Some(event)) => {
                pending.next();
                settle.hold("/wt".into(), event);
                event
            }
        };

        let due = settle.due(now);

        if !due.is_empty() {
            out.push(now.duration_since(start).as_millis() as u64);
        }
    }
}

/// 1 event: 1 topic after the quiet time.
#[test]
fn one_event_goes_out_after_the_quiet_time() {
    let out = published(&[0]);

    assert_eq!(out, vec![200]);
}

/// A burst (20 events 25 ms apart): 1 topic, the quiet time after its last event.
#[test]
fn a_burst_goes_out_once() {
    let burst: Vec<u64> = (0..20).map(|i| i * 25).collect();

    let out = published(&burst);

    assert_eq!(out, vec![475 + 200]);
}

/// A constant stream (1 event per 100 ms for 3 s): a topic every second, 1 after its end.
#[test]
fn a_constant_stream_goes_out_every_max_wait() {
    let stream: Vec<u64> = (0..30).map(|i| i * 100).collect();

    let out = published(&stream);

    assert_eq!(out, vec![1000, 2100, 3100]);
}

/// 2 work trees: each its own deadline.
#[test]
fn work_trees_settle_apart() {
    let start = Instant::now();
    let mut settle = Settle::default();

    settle.hold("/a".into(), start);
    settle.hold("/b".into(), start + Duration::from_millis(150));
    let first = settle.due(start + QUIET);
    let second = settle.due(start + Duration::from_millis(350));

    assert_eq!((first, second), (vec![PathBuf::from("/a")], vec![PathBuf::from("/b")]));
    assert_eq!(settle.next(), None);
}
