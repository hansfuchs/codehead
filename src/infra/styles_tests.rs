#[test]
fn frontmatter_split() {
    let style = super::parse("x", "---\nname: MECHA\ndescription: d\n---\n\nbody line\n");

    assert_eq!(style.name, "MECHA");
    assert_eq!(style.body, "body line");
}
