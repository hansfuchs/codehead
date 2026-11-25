use super::{App, Field, KEYS_ITEM, Line, Modal, Outcome, PathBuf, Span, Tab, UnicodeWidthStr, theme, worktree_of};
use crate::screens::quick_menu::Action;

impl App {
    /// A start screen tab, from the menu (here or in the thread view).
    pub(crate) fn go_home(&mut self, home: &crate::screens::quick_menu::Home) {
        use crate::screens::quick_menu::Home;
        self.overlays.modal = None;
        self.draft_selection.drafts_menu = None;

        if *home == Home::New {
            self.new_task();
        }

        self.switch_tab(match home {
            Home::New => Tab::New,
            Home::Settings => Tab::Settings,
            Home::Threads => Tab::Threads,
            Home::Merge(_) => Tab::Merge,
            Home::Flows(_) => Tab::Flows,
        });

        if *home != Home::Settings {
            self.tabs_focus = false;
        }

        if let Home::Merge(Some(pr)) = home {
            self.merge_tab.open(pr);
        }

        if let Home::Flows(Some(slug)) = home {
            self.flows_tab.open(slug);
        }
    }

    /// What the flows tab asked for.
    pub(super) fn flows_out(&mut self, output: crate::features::flows::Out) {
        match output {
            crate::features::flows::Out::None => {}
            crate::features::flows::Out::Toast(text) => self.toast(text, false),
            crate::features::flows::Out::Up => self.tabs_focus = true,
        }
    }

    /// What the projects tab asked for.
    pub(super) fn projects_out(&mut self, output: crate::features::projects::Out) {
        match output {
            crate::features::projects::Out::None => {}
            crate::features::projects::Out::Toast(text) => self.toast(text, false),
            crate::features::projects::Out::Up => self.tabs_focus = true,
        }
    }

    /// The projects tab opens: the start screen's repo first, then the threads' repos, the home
    /// repo's repos on disk; thread counts per project.
    pub(super) fn enter_projects(&mut self) {
        let mut repos: Vec<PathBuf> = self.cfg.repo.iter().cloned().collect();

        for repo in self
            .thread_list
            .tlist
            .cards
            .iter()
            .filter_map(|card| card.repo.clone())
            .chain(
                self.repository
                    .repos
                    .iter()
                    .map(|home_repo| home_repo.path.clone())
                    .filter(|path| path.is_dir()),
            )
        {
            push_new_repo(&mut repos, repo);
        }

        self.project_selection.projects_tab.enter(repos);
        self.project_selection.projects_tab.threads = self
            .thread_list
            .tlist
            .cards
            .iter()
            .filter(|card| !card.archived() && !card.projects.is_empty())
            .filter_map(|card| Some((card.repo.clone()?, card.projects.clone())))
            .collect();
    }

    /// What the merge tab asked for.
    pub(super) fn merge_out(&mut self, output: crate::screens::merge::merge_tab::Out) {
        use crate::screens::merge::merge_tab::Out;

        match output {
            Out::None | Out::Back => {}
            Out::Toast(text) => self.toast(text, false),
            // 0 thread: the new-thread view, the task written; the owner sends it
            Out::Compose(task) => {
                self.compose.prompt.set_text(&task);
                self.compose.prompt.move_end();
                self.switch_tab(Tab::New);
                self.tabs_focus = false;
                self.compose.field = Field::Prompt;
                self.toast("the task is written: pick agents, send", false);
            }
        }
    }

    pub(super) fn menu_action(&mut self, action: crate::screens::quick_menu::Action) {
        if let crate::screens::quick_menu::Action::Command(index) = &action
            && *index != KEYS_ITEM
            && run_menu_command(self, *index)
        {
            return;
        }

        match action {
            Action::None => return,
            // home: the start screen is home
            Action::Home(home) => self.go_home(&home),
            Action::Open(card) => self.outcome = Some(Outcome::Resume(Box::new(worktree_of(&card)))),
            Action::Pr(pull_request) => match pull_request.thread {
                Some(thread) => {
                    crate::screens::thread_view::open_pr(&thread.id, &pull_request.item.pr);
                    let card = crate::ui::thread_card::Card {
                        id: thread.id.clone(),
                        title: thread.title.clone(),
                        dir: Some(thread.dir.clone()),
                        ..Default::default()
                    };

                    self.outcome = Some(Outcome::Resume(Box::new(worktree_of(&card))));
                }
                None => self.go_home(&(crate::screens::quick_menu::Home::Merge(Some(pull_request.item.pr.clone())))),
            },
            Action::Jump(card, ts) => {
                crate::domain::search::jump_to(&card.id, ts);
                self.outcome = Some(Outcome::Resume(Box::new(worktree_of(&card))));
            }
            Action::Command(KEYS_ITEM) => {
                show_menu_keys(self);
            }
            Action::Close | Action::Back | Action::Command(_) => {}
        }

        self.overlays.menu = None;
    }
}

fn push_new_repo(repos: &mut Vec<PathBuf>, repo: PathBuf) {
    if !repos.contains(&repo) {
        repos.push(repo);
    }
}

fn show_menu_keys(app: &mut App) {
    let key_width = app
        .overlays
        .menu_help
        .iter()
        .map(|(key, _)| key.width())
        .max()
        .unwrap_or(0);

    let lines = app
        .overlays
        .menu_help
        .iter()
        .map(|(key, line)| {
            Line::from(vec![
                Span::styled(format!("{key:<key_width$}  "), theme::accent()),
                Span::styled(*line, theme::muted()),
            ])
        })
        .collect();

    app.overlays.modal = Some(Modal::Info {
        title: "keys".into(),
        lines,
        scroll: 0,
    });
}

fn run_menu_command(app: &mut App, index: usize) -> bool {
    let Some(action) = app.overlays.menu_cmds.get(index).map(|command| command.act) else {
        return false;
    };

    app.overlays.menu = None;
    app.run(action);
    true
}
