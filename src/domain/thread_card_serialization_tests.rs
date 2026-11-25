use super::*;

#[test]
fn activity_groups_keep_the_legacy_json_fields() {
    let mut card = Card::default();

    card.agents.live = 2;
    card.agents.busy = true;
    card.agents.blocked = true;
    card.completion.done = true;
    let mut value = serde_json::to_value(&card).unwrap();

    assert_eq!(value["live"], 2);
    assert_eq!(value["busy"], true);
    assert_eq!(value["blocked"], true);
    assert_eq!(value["done"], true);
    assert!(value.get("agents").is_none());
    assert!(value.get("completion").is_none());
    assert_eq!(serde_json::from_value::<Card>(value.clone()).unwrap(), card);

    value.as_object_mut().unwrap().remove("done");
    value.as_object_mut().unwrap().remove("review");
    value.as_object_mut().unwrap().remove("monitoring");
    let restored: Card = serde_json::from_value(value).unwrap();

    assert!(!restored.completion.done);
    assert!(!restored.completion.review);
    assert!(!restored.agents.monitoring);
}
