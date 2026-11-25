use super::{Agent, json};

impl Agent {
    pub(super) fn read_file(&mut self, name: &str) {
        let path = self.cwd.join(name);
        let count = name.to_string();

        self.tool(
            &format!("Read {name}"),
            "read",
            &(json!({"file_path": path.display().to_string()})),
            move |directory| std::fs::read_to_string(directory.join(count)).unwrap_or_default(),
        );
    }
}
