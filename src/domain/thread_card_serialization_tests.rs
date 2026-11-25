use super::*;

#[test]
fn activity_groups_keep_the_legacy_json_fields() {
    let mut card = Card::default();

    card.agents.live = 2;
    card.agents.busy = true;
    card.agents.blocked = true;
    card.completion.done = true;
    let mut json = serde_json::to_value(&card).unwrap();

    assert_eq!(json["live"], 2);
    assert_eq!(json["busy"], true);
    assert_eq!(json["blocked"], true);
    assert_eq!(json["done"], true);
    assert!(json.get("agents").is_none());
    assert!(json.get("completion").is_none());
    assert_eq!(serde_json::from_value::<Card>(json.clone()).unwrap(), card);

    json.as_object_mut().unwrap().remove("done");
    json.as_object_mut().unwrap().remove("review");
    json.as_object_mut().unwrap().remove("monitoring");
    let restored: Card = serde_json::from_value(json).unwrap();

    assert!(!restored.completion.done);
    assert!(!restored.completion.review);
    assert!(!restored.agents.monitoring);
}
