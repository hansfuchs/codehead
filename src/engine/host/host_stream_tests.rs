use super::*;

#[test]
pub(crate) fn subscribers_get_live_text_then_chunks_and_wakes() {
    let (agent, bytes) = UnixStream::pair().unwrap();
    let (tx, rx) = mpsc::channel::<Input>();

    std::thread::spawn(move || serve(agent, &tx));
    let mut width = bytes.try_clone().unwrap();

    writeln!(width, "{}", json!({"op": "subscribe"})).unwrap();

    let Ok(Input::Subscribe(text)) = rx.recv() else {
        panic!("0 subscribe")
    };

    let (htx, _hrx) = mpsc::channel();
    let mut path = test_host("t".into(), htx);

    path.live.insert("ada".into(), "Hel".into());
    path.subscribe(text);
    path.harness("ada".into(), Out::Delta("lo".into()));
    path.push(&json!({"op": "wake"}));
    let mut result = BufReader::new(bytes);

    let mut line = || {
        let mut line_value = String::new();

        result.read_line(&mut line_value).unwrap();
        serde_json::from_str::<Value>(&line_value).unwrap()
    };

    assert_eq!(line()["ok"], true);
    assert_eq!(line(), json!({"op": "live", "agent": "ada", "text": "Hel"}));
    assert_eq!(line(), json!({"op": "delta", "agent": "ada", "text": "lo"}));
    assert_eq!(line(), json!({"op": "wake"}));
    assert_eq!(path.live["ada"], "Hello");
}
#[test]
pub(crate) fn mentions_match_names_once_in_order() {
    let names = vec!["ada".to_string(), "bob".to_string(), "r2-d2".to_string()];

    assert_eq!(mentions("@bob look, then @Ada. @bob again", &names), vec!["bob", "ada"]);
    assert_eq!(mentions("mail me@ada.dev", &names), Vec::<String>::new());
    assert_eq!(mentions("ask @r2-d2!", &names), vec!["r2-d2"]);
    assert_eq!(mentions("@carol", &names), [] as [std::string::String; 0]);
}
