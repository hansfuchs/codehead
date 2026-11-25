use super::{Card, Form, FormOut, Path, TextArea, finish_open, open_branch, repo_name, theme};

impl Form {
    pub(super) fn open(id: &str, cards: &[Card], roster: &[String]) -> Option<Form> {
        let thread = crate::engine::store::load(id)?;

        let auto = crate::domain::threads::repo_root(Path::new(&thread.dir)).or_else(|| {
            thread
                .worktree
                .as_deref()
                .and_then(|path| crate::domain::threads::repo_root(Path::new(path)))
        });

        let mut repos: Vec<Option<String>> = vec![None];

        for result in cards
            .iter()
            .filter_map(|character| character.repo.as_ref())
            .map(|path_value| path_value.display().to_string())
            .chain(thread.repo.clone())
        {
            open_branch(auto.as_ref(), &mut repos, result);
        }

        repos[1..].sort_by_key(|result_value| result_value.as_deref().map(|path_2| repo_name(Path::new(path_2))));
        let repo_i = thread
            .repo
            .as_ref()
            .and_then(|result_2| repos.iter().position(|x| x.as_ref() == Some(result_2)))
            .unwrap_or(0);
        let mut title = TextArea::new(true, "title");

        title.style = Some(theme::text());
        title.set_text(thread.title.lines().next().unwrap_or_default());
        let mut agents: Vec<(String, bool)> = thread
            .agents
            .iter()
            .map(|agent_value| (agent_value.clone(), true))
            .collect();

        for agent_2 in roster {
            ensure_roster_agent(&mut agents, agent_2);
        }

        Some(finish_open(&thread, auto, repos, repo_i, title, agents))
    }
    pub(super) fn n_rows(&self) -> usize {
        2 + self.projs.len() + self.links.len() + self.agents.len()
    }
    pub(super) fn save(&mut self) -> FormOut {
        save_form(self)
    }
}

fn ensure_roster_agent(agents: &mut Vec<(String, bool)>, agent_2: &String) {
    if !agents.iter().any(|(count, _)| count == agent_2) {
        agents.push((agent_2.clone(), false));
    }
}

fn save_form(state: &mut Form) -> FormOut {
    let agents: Vec<String> = state
        .agents
        .iter()
        .filter(|agent| agent.1)
        .map(|agent_value| agent_value.0.clone())
        .collect();

    if agents.is_empty() {
        state.error = Some("1 agent at least".into());
        return FormOut::Stay;
    }

    if state.title.text().trim().is_empty() {
        state.error = Some("title required".into());
        state.row = 0;
        return FormOut::Stay;
    }

    let error_value = crate::domain::threads::Edit {
        title: state.title.text(),
        repo: state.repos.get(state.repo_i).cloned().flatten(),
        checkouts: state
            .links
            .iter()
            .filter(|line| line.1)
            .map(|line_value| line_value.0.clone())
            .collect(),
        agents,
        projects: (!state.projs[0].1).then(|| {
            state
                .projs
                .iter()
                .skip(1)
                .filter(|path| path.1)
                .map(|path_value| path_value.0.clone())
                .collect()
        }),
    };

    FormOut::Save(state.id.clone(), error_value)
}
