use super::{PathBuf, Value, View, json, prompt_path};

impl View {
    /// The unsent prompt: the draft under a history step, else the prompt's text; with its
    /// pasted files.
    pub(super) fn prompt_draft(&self) -> String {
        let text = if self.prompt.hist_at.is_some() {
            self.prompt.draft.clone()
        } else {
            self.prompt.input.text()
        };

        // sent, not in the log yet: a crash before the host has it loses 0 messages
        let unsent: Vec<&str> = self.prompt.pending.iter().map(|(sent, ..)| sent.as_str()).collect();

        if text.trim().is_empty() && unsent.is_empty() {
            return String::new();
        }

        let files: Vec<Value> = self
            .prompt
            .attachments
            .iter()
            .filter(|(marker, _)| text.contains(marker.as_str()))
            .map(|(marker, path)| json!([marker, path]))
            .collect();

        json!({"text": text, "attachments": files, "unsent": unsent}).to_string()
    }

    /// After every key, paste, click: the unsent prompt to disk, if it changed (0 text: 0 file).
    /// A crash, a closed terminal, a kill: the next view of the thread has it back.
    pub(super) fn save_prompt(&mut self) {
        let draft = self.prompt_draft();

        if draft == self.prompt.prompt_saved {
            return;
        }

        let path = prompt_path(&self.id);

        let saved = if draft.is_empty() {
            remove_draft(&path)
        } else {
            crate::engine::write_atomic(&path, draft.as_bytes())
        };

        match saved {
            Ok(()) => self.prompt.prompt_saved = draft,
            Err(error) => self.toast(format!("prompt draft not saved: {error}")),
        }
    }

    /// The view opens: the unsent prompt of its last view back in the prompt.
    pub(super) fn load_prompt(&mut self) {
        let Ok(raw) = std::fs::read_to_string(prompt_path(&self.id)) else {
            return;
        };

        let Ok(value) = serde_json::from_str::<Value>(&raw) else {
            return;
        };

        // messages the log never got (a crash right after the send): back in the prompt
        let sent = self.thread.sent();

        let mut parts: Vec<String> = value["unsent"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .filter(|unsent| {
                !sent
                    .iter()
                    .any(|logged| logged.starts_with(unsent) || unsent.starts_with(logged.as_str()))
            })
            .map(String::from)
            .collect();

        let text = value["text"].as_str().unwrap_or_default();

        if !text.trim().is_empty() {
            parts.push(text.to_string());
        }

        if parts.is_empty() {
            return;
        }

        let text = parts.join("\n\n");

        self.prompt.input.set_text(&text);
        self.prompt.input.move_end();
        self.prompt.attachments = value["attachments"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|pair| Some((pair[0].as_str()?.to_string(), PathBuf::from(pair[1].as_str()?))))
            .collect();

        // new pastes: numbers after the draft's markers (a dropped image: 0 file in the paste dir)
        for (marker, _) in &self.prompt.attachments {
            self.prompt.paster.seen(marker);
        }

        self.prompt.prompt_saved = raw;
    }
}

fn remove_draft(path: &PathBuf) -> Result<(), std::io::Error> {
    std::fs::remove_file(path).or_else(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            Ok(())
        } else {
            Err(error)
        }
    })
}
