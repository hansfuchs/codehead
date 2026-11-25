use super::*;

#[test]
fn prefix_is_the_branch_tail_as_a_dns_label() {
    assert_eq!(prefix("task/fix-ui"), "fix-ui");
    assert_eq!(prefix("task/Fix_UI.v2"), "fix-ui-v2");
    assert_eq!(prefix("main"), "main");
}

#[test]
fn urls_of_a_worktree_from_the_route_list() {
    let routes = parse_routes(
        r#"{"routes":[{"hostname":"fix-ui.shop.localhost","url":"https://fix-ui.shop.localhost","port":4123},{"hostname":"shop.localhost","url":"https://shop.localhost","port":4001},{"hostname":"api.fix-ui.shop.localhost","url":"https://api.fix-ui.shop.localhost"}]}"#,
    );

    assert_eq!(routes.len(), 3);
    assert_eq!(urls_of(&routes, "fix-ui"), ["https://fix-ui.shop.localhost"]);
    assert_eq!(urls_of(&routes, ""), [] as [std::string::String; 0]);
    assert!(prompt(Some("task/fix-ui")).contains("fix-ui.<app>.localhost"));
}
