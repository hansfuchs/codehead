use super::{Agent, json};

impl Agent {
    pub(super) fn read_file(&mut self, name: &str) {
        let path = self.cwd.join(name);
        let name = name.to_string();

        self.tool(
            &format!("Read {name}"),
            "read",
            &(json!({"file_path": path.display().to_string()})),
            move |cwd| std::fs::read_to_string(cwd.join(name)).unwrap_or_default(),
        );
    }
}
