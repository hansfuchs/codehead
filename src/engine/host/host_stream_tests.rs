use super::*;

#[test]
pub(crate) fn subscribers_get_live_text_then_chunks_and_wakes() {
    let (server, client) = UnixStream::pair().unwrap();
    let (tx, rx) = mpsc::channel::<Input>();

    std::thread::spawn(move || serve(server, &tx));
    let mut writer = client.try_clone().unwrap();

    writeln!(writer, "{}", json!({"op": "subscribe"})).unwrap();

    let Ok(Input::Subscribe(subscriber)) = rx.recv() else {
        panic!("0 subscribe")
    };

    let (htx, harness_rx) = mpsc::channel();
    let mut host = test_host("t".into(), htx);

    host.live.insert("ada".into(), "Hel".into());
    host.subscribe(subscriber);
    host.harness("ada".into(), Out::Delta("lo".into()));
    host.push(&json!({"op": "wake"}));
    let mut reader = BufReader::new(client);

    let mut next_line = || {
        let mut line = String::new();

        reader.read_line(&mut line).unwrap();
        serde_json::from_str::<Value>(&line).unwrap()
    };

    assert_eq!(next_line()["ok"], true);
    assert_eq!(next_line(), json!({"op": "live", "agent": "ada", "text": "Hel"}));
    assert_eq!(next_line(), json!({"op": "delta", "agent": "ada", "text": "lo"}));
    assert_eq!(next_line(), json!({"op": "wake"}));
    assert_eq!(host.live["ada"], "Hello");

    drop(harness_rx);
}

#[test]
pub(crate) fn mentions_match_names_once_in_order() {
    let names = vec!["ada".to_string(), "bob".to_string(), "r2-d2".to_string()];

    assert_eq!(mentions("@bob look, then @Ada. @bob again", &names), vec!["bob", "ada"]);
    assert_eq!(mentions("mail me@ada.dev", &names), Vec::<String>::new());
    assert_eq!(mentions("ask @r2-d2!", &names), vec!["r2-d2"]);
    assert_eq!(mentions("@carol", &names), [] as [std::string::String; 0]);
}
