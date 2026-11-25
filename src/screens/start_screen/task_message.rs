use super::{App, Field, Outcome, claude, flow_default_set};

impl App {
    /// The thread's title: the title field, else the prompt's 1st line (max. 80 chars).
    pub(super) fn task_title(&self) -> String {
        let thread = self.compose.title.text().trim().to_string();

        if !thread.is_empty() {
            return thread;
        }

        let line = self
            .compose
            .prompt
            .text()
            .lines()
            .map(str::trim)
            .find(|line_value| !line_value.is_empty())
            .unwrap_or_default()
            .to_string();

        if line.chars().count() > 80 {
            line.chars().take(79).collect::<String>() + "…"
        } else {
            line
        }
    }

    /// Thread message: line 1 = the thread title, the notes to read, the Claude Code context, then the
    /// prompt. 0 title: the prompt comes first (its 1st line: the title), 0 line twice.
    pub(crate) fn message(&self) -> String {
        let derived = self.compose.title.text().trim().is_empty();

        // `#name` of a shortcut: its text
        let text = crate::domain::shortcuts::expand(&self.compose.prompt.text());
        let prompt = crate::ui::textarea::without_empty_items(&text);
        let mut parts = Vec::new();

        if derived {
            parts.push(crate::infra::config::expand_aliases(prompt.trim()));
        } else {
            parts.push(self.compose.title.text().trim().to_string());
        }

        if !self.note_list.selected.is_empty() {
            let list: Vec<String> = self
                .note_list
                .selected
                .iter()
                .map(|path| format!("- {}", self.note_list.notes.abs(path).display()))
                .collect();

            parts.push(format!("Read these notes first:\n{}", list.join("\n")));
        }

        parts.extend(self.compose.carry.iter().map(claude::context));

        if !derived && !prompt.trim().is_empty() {
            parts.push(crate::infra::config::expand_aliases(prompt.trim_end()));
        }

        // pasted images, long texts: their markers in the prompt, the files here
        let files: Vec<String> = self
            .compose
            .files
            .iter()
            .filter(|(message, _)| prompt.contains(message.as_str()))
            .map(|(message_value, path_value)| format!("- {message_value}: {}", path_value.display()))
            .collect();

        if !files.is_empty() {
            parts.push(format!("Attachments (read these files):\n{}", files.join("\n")));
        }

        parts.join("\n\n")
    }

    pub(super) fn start(&mut self) {
        // the title: optional (the prompt's 1st line); a prompt or a title: required
        if self.task_title().is_empty() {
            self.compose.field = Field::Prompt;
            self.toast("type the task first", true);
            return;
        }

        match self.target() {
            Ok(_) if self.compose.picks.is_empty() => {
                self.compose.field = Field::Agents;
                self.toast("pick 1+ agents (space)", true);
            }
            Ok(target) => {
                start_agents(self, target);
            }
            Err(error) => {
                self.compose.field = Field::Title;
                self.toast(error, true);
            }
        }
    }
}

fn start_agents(app_state: &mut App, target: super::Target) {
    let agents: Vec<String> = app_state
        .agent_list
        .agents
        .iter()
        .filter(|agent| app_state.compose.picks.contains(&agent.name))
        .map(|agent_value| agent_value.name.clone())
        .collect();

    // each agent's flow (its start step); the lead's: the repo's next default
    let flows: Vec<(String, String, usize)> = agents
        .iter()
        .map(|agent_2| (agent_2.clone(), app_state.flow_of(agent_2)))
        .filter(|(_, (text, _))| !text.is_empty())
        .map(|(agent_3, (text_value, st))| (agent_3, text_value, st))
        .collect();

    if let Some(lead) = agents.first()
        && let Some((_, slug, _)) = flows.iter().find(|(agent_4, _, _)| agent_4 == lead)
    {
        flow_default_set(&app_state.flow_key(), slug);
    }

    let projects = app_state
        .project_selection
        .proj_picks
        .clone()
        .filter(|_| app_state.cfg.repo.is_some());
    let title_set = !app_state.compose.title.text().trim().is_empty();

    app_state.outcome = Some(Outcome::New {
        message: app_state.message(),
        target,
        agents,
        clone: app_state.clone_first(),
        flows,
        projects,
        title_set,
    });
}
