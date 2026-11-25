use super::*;

#[test]
fn slug_cuts_and_trims() {
    assert_eq!(slug("Fix the Build, now!"), "fix-the-build-now");
    assert_eq!(slug("  "), "");
}

#[test]
fn fuzzy_prefers_contiguous() {
    assert!(fuzzy("kub", "use kubectl").unwrap() > fuzzy("kbl", "use kubectl").unwrap());
    assert!(fuzzy("zz", "abc").is_none());
}
