use super::*;

#[test]
fn reads_return_before_completion_and_deduplicate() {
    let mut reads = Reads::default();
    let (release, wait) = mpsc::channel();

    reads.request("rows", move || wait.recv().unwrap());

    // A blocked worker cannot block a tab change or start a duplicate read.
    reads.request("rows", || panic!("duplicate read"));

    assert_eq!(reads.poll(), [] as [(&str, i32); 0]);

    release.send(42).unwrap();
    let (_, _, value) = reads.rx.recv_timeout(Duration::from_secs(2)).unwrap();

    assert_eq!(value, 42);
}

#[test]
fn invalidation_discards_results_from_before_a_write() {
    let mut reads = Reads::default();

    reads.request("rows", || 1);
    let stale = reads.rx.recv_timeout(Duration::from_secs(2)).unwrap();

    reads.invalidate();
    reads.request("rows", || 2);
    let fresh = reads.rx.recv_timeout(Duration::from_secs(2)).unwrap();

    reads.tx.send(stale).unwrap();
    reads.tx.send(fresh).unwrap();

    assert_eq!(reads.poll(), vec![("rows", 2)]);

    reads.request("rows", || panic!("fresh value read again"));

    assert!(reads.pending.is_empty());
}
