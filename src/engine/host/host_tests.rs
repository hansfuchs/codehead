use super::*;

#[path = "host_flow_tests.rs"]
mod host_flow_tests;

#[path = "host_child_tests.rs"]
mod host_child_tests;

#[path = "host_setup_tests.rs"]
mod host_setup_tests;

#[path = "host_stream_tests.rs"]
mod host_stream_tests;

fn test_host(id: String, harness_tx: Sender<(String, Out)>) -> Host {
    Host {
        id,
        exe: PathBuf::new(),
        sessions: HashMap::new(),
        asks: HashMap::new(),
        harness_tx,
        last: Instant::now(),
        stopping: false,
        subs: std::cell::RefCell::default(),
        live: HashMap::new(),
        turn_use: HashMap::new(),
        setup_running: false,
        queue: Vec::new(),
        later: Vec::new(),
        subagents: crate::engine::subagents::Tracker::default(),
        sub_scan: Instant::now(),
        held: Vec::new(),
        kid: Kid::default(),
        kids_at: Instant::now(),
        resume: HashMap::new(),
    }
}
