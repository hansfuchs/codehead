use super::*;

#[test]
fn compares_numerically() {
    assert!(newer("0.2.10", "0.2.9"));
    assert!(newer("0.3.1", "0.2.40"));
    assert!(!newer("0.2.9", "0.2.9"));
    assert!(!newer("0.2.8", "0.2.9"));
    assert!(!newer("garbage", "0.2.9"));
}
