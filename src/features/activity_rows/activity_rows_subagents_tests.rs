use super::*;

fn sub(id: &str, master: &str, parent: Option<&str>) -> Sub {
    Sub {
        id: id.into(),
        master: master.into(),
        parent: parent.map(String::from),
        description: format!("job {id}"),
        start: crate::engine::now_ms(),
        ..Default::default()
    }
}

#[test]
fn order_groups_by_master_with_nested_ones_under_their_parent() {
    let mut done = sub("x", "ada", None);

    done.state = Some("done".into());
    let subs = [
        sub("a", "ada", None),
        sub("b", "bob", None),
        sub("c", "ada", Some("a")),
        done,
        sub("d", "ada", None),
        sub("e", "ada", Some("gone")),
    ];
    let output: Vec<(&str, usize)> = order(&subs)
        .into_iter()
        .map(|(sub, depth)| (sub.id.as_str(), depth))
        .collect();

    assert_eq!(output, [("a", 0), ("c", 1), ("d", 0), ("e", 0), ("b", 0)]);
}
