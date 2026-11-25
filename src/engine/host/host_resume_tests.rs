use super::*;

#[test]
fn the_resume_time_comes_from_the_reset_in_the_error() {
    let _env = crate::engine::ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    // 0 limits file in the test data dir: the error's `resets 1:50pm`
    let now = crate::engine::now_ms();
    let at = resume_at("You've hit your 5-hour limit · resets 1:50pm (Europe/Berlin)", now);

    assert!(at > now && at <= now + 86_400_000 + 60_000, "{at} {now}");
    assert_eq!(crate::domain::chat::clock(at - 60_000), "13:50");

    // 0 reset in the text: 30 min (+ 1)
    assert_eq!(resume_at("usage limit reached", now), now + 31 * 60_000);
}
