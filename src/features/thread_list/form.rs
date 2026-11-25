use super::{Card, Form, FormOut, Path, TextArea, build_form, push_repo, repo_name, theme};

impl Form {
    pub(super) fn open(id: &str, cards: &[Card], roster: &[String]) -> Option<Form> {
        let thread = crate::engine::store::load(id)?;

        let auto = crate::domain::threads::repo_root(Path::new(&thread.dir)).or_else(|| {
            thread
                .worktree
                .as_deref()
                .and_then(|worktree| crate::domain::threads::repo_root(Path::new(worktree)))
        });

        let mut repos: Vec<Option<String>> = vec![None];

        for repo in cards
            .iter()
            .filter_map(|card| card.repo.as_ref())
            .map(|repo| repo.display().to_string())
            .chain(thread.repo.clone())
        {
            push_repo(auto.as_ref(), &mut repos, repo);
        }

        repos[1..].sort_by_key(|repo| repo.as_deref().map(|path| repo_name(Path::new(path))));
        let repo_i = thread
            .repo
            .as_ref()
            .and_then(|repo| repos.iter().position(|pick| pick.as_ref() == Some(repo)))
            .unwrap_or(0);
        let mut title = TextArea::new(true, "title");

        title.style = Some(theme::text());
        title.set_text(thread.title.lines().next().unwrap_or_default());
        let mut agents: Vec<(String, bool)> = thread.agents.iter().map(|agent| (agent.clone(), true)).collect();

        for agent in roster {
            ensure_roster_agent(&mut agents, agent);
        }

        Some(build_form(&thread, auto, repos, repo_i, title, agents))
    }
    pub(super) fn n_rows(&self) -> usize {
        2 + self.projs.len() + self.links.len() + self.agents.len()
    }
    pub(super) fn save(&mut self) -> FormOut {
        save_form(self)
    }
}

fn ensure_roster_agent(agents: &mut Vec<(String, bool)>, agent: &String) {
    if !agents.iter().any(|(name, _)| name == agent) {
        agents.push((agent.clone(), false));
    }
}

fn save_form(form: &mut Form) -> FormOut {
    let agents: Vec<String> = form
        .agents
        .iter()
        .filter(|agent| agent.1)
        .map(|agent| agent.0.clone())
        .collect();

    if agents.is_empty() {
        form.error = Some("1 agent at least".into());
        return FormOut::Stay;
    }

    if form.title.text().trim().is_empty() {
        form.error = Some("title required".into());
        form.row = 0;
        return FormOut::Stay;
    }

    let edit = crate::domain::threads::Edit {
        title: form.title.text(),
        repo: form.repos.get(form.repo_i).cloned().flatten(),
        checkouts: form
            .links
            .iter()
            .filter(|link| link.1)
            .map(|link| link.0.clone())
            .collect(),
        agents,
        projects: (!form.projs[0].1).then(|| {
            form.projs
                .iter()
                .skip(1)
                .filter(|project| project.1)
                .map(|project| project.0.clone())
                .collect()
        }),
    };

    FormOut::Save(form.id.clone(), edit)
}
