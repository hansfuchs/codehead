use super::*;
use serde_json::json;

#[test]
fn guard_and_modes() {
    assert!(risky(&json!({"command": "rm -rf ~/x"})));
    assert!(risky(&json!({"command": ["git", "reset", "--hard"]})));
    assert!(risky(&json!({"command": "sudo make install"})));
    assert!(!risky(&json!({"command": "rm -rf target"})));
    assert!(!risky(&json!({"command": "cargo test"})));
    assert!(auto_allow("autopilot", "Bash", "execute", &json!({"command": "ls"})));
    assert!(!auto_allow(
        "autopilot",
        "Bash",
        "execute",
        &json!({"command": "sudo ls"})
    ));
    assert!(auto_allow("edits", "Edit", "edit", &json!({})));
    assert!(!auto_allow("edits", "Bash", "execute", &json!({"command": "ls"})));
    assert!(!auto_allow("supervised", "Edit", "edit", &json!({})));
    assert!(auto_allow("supervised", "codehead_send", "other", &json!({})));
}
