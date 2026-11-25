#[test]
fn frontmatter_split() {
    let text = super::parse("x", "---\nname: MECHA\ndescription: d\n---\n\nbody line\n");

    assert_eq!(text.name, "MECHA");
    assert_eq!(text.body, "body line");
}
