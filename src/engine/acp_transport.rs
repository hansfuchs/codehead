use super::{Arc, Conn, Kind, Mutex, Out, SETUP_TIMEOUT, State, Value};

impl Conn {
    pub(super) fn read_protocol(&self, stdout: std::process::ChildStdout, stderr: Arc<Mutex<Vec<String>>>) {
        let connection = self.clone();

        std::thread::spawn(move || {
            crate::engine::harness::json_lines(stdout, |value| connection.receive(&value, &stderr));
            connection.stdout_closed(&stderr);
        });
    }

    fn receive(&self, value: &Value, stderr: &Mutex<Vec<String>>) -> bool {
        let Ok(mut state) = self.0.lock() else { return false };

        state.receive(value, stderr)
    }

    fn stdout_closed(&self, stderr: &Mutex<Vec<String>>) {
        if let Ok(mut state) = self.0.lock() {
            state.exit(stderr, "");
        }
    }

    pub(super) fn watch_setup(&self, stderr: Arc<Mutex<Vec<String>>>) {
        let connection = self.clone();

        std::thread::spawn(move || watch_setup(&connection, &stderr));
    }
}

impl State {
    fn receive(&mut self, value: &Value, stderr: &Mutex<Vec<String>>) -> bool {
        match self.incoming(value) {
            Ok(()) => true,
            Err(error) => self.receive_error(stderr, &error),
        }
    }

    fn receive_error(&mut self, stderr: &Mutex<Vec<String>>, error: &str) -> bool {
        let message = format!("acp: {error}");

        if self.session.is_none() {
            self.exit(stderr, &message);
            return false;
        }

        self.out(Out::Event(Kind::Error { message }));
        true
    }
}

fn watch_setup(connection: &Conn, stderr: &Arc<Mutex<Vec<String>>>) {
    let start = std::time::Instant::now();

    while start.elapsed() < SETUP_TIMEOUT {
        std::thread::sleep(std::time::Duration::from_millis(250));

        match connection.0.lock() {
            Ok(state) if state.session.is_some() || state.dead => return,
            Ok(_) => {}
            Err(_) => return,
        }
    }

    if let Ok(mut state) = connection.0.lock()
        && state.session.is_none()
    {
        state.exit(stderr, &format!("acp: 0 session after {}s", SETUP_TIMEOUT.as_secs()));
    }
}
