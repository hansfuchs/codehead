use super::{App, Choices, KeyEvent, Path, PathBuf, Target, Where, quick_choices, slug, threads, tilde, where_choices};

impl App {
    pub(super) fn target(&self) -> Result<Target, String> {
        if let Some(target) = &self.compose.target {
            return Ok(target.clone());
        }

        if self.cfg.repo.is_none() {
            return Ok(Target::InPlace);
        }

        if let Some(branch) = &self.cfg.branch {
            return Ok(Target::Branch(branch.clone()));
        }

        let choice = self
            .repository
            .wheres
            .get(self.repository.where_i)
            .cloned()
            .unwrap_or(Where::NewWorktree);

        if choice == Where::Current {
            return Ok(Target::Worktree {
                path: self.repository.here_dir.clone(),
                branch: self.repository.here_branch.clone(),
            });
        }

        if let Where::Branch(branch) = choice {
            let (switch, _) = self.checkout_box();
            let (stash, _) = self.stash_box();

            return Ok(Target::Checkout { branch, switch, stash });
        }

        let name = slug(&self.task_title());

        if name.is_empty() {
            return Err("title needs letters or digits (branch name)".into());
        }

        let branch = format!("{}/{name}", self.cfg.prefix);

        Ok(if choice == Where::NewBranch {
            Target::NewBranch(branch)
        } else {
            Target::Branch(branch)
        })
    }

    pub(super) fn repo_text(&self) -> String {
        self.cfg
            .repo
            .as_ref()
            .map(|repo| tilde(&repo.display().to_string()))
            .unwrap_or_default()
    }

    /// Short label of the where choice (the chip of the where row).
    pub(super) fn where_chip(&self) -> String {
        match (
            &self.compose.target,
            self.repository.wheres.get(self.repository.where_i),
        ) {
            (Some(Target::Worktree { branch, .. }), _) => format!("worktree {branch}"),
            (Some(Target::Dir(_)), _) => "session dir".into(),
            (Some(_), _) | (None, None) => "here".into(),
            (None, Some(Where::Current)) if self.repository.here_branch.is_empty() => {
                "current branch (detached)".into()
            }
            (None, Some(Where::Current)) => format!("current branch · {}", self.repository.here_branch),
            (None, Some(Where::Branch(branch))) => branch.clone(),
            (None, Some(Where::NewBranch)) => "+ new branch".into(),
            (None, Some(Where::NewWorktree)) => "+ new branch + worktree".into(),
        }
    }

    /// Enter / click on the repo row: the repo picker on the repo (else the dir codehead runs in).
    pub(super) fn open_repo_pick(&mut self) {
        let at = self
            .repository
            .repo
            .as_ref()
            .map(|repo| repo.path.clone())
            .filter(|path| path.exists())
            .unwrap_or_else(|| self.cfg.cwd.clone());

        self.repository.repo_pick = Some(crate::screens::start_screen::repo_pick::Picker::new(
            &at,
            self.repository.repos.clone(),
        ));
    }

    /// A pick of the repo picker: E's repo and its branch choices. The start dir (or a dir above
    /// it in its repo): the dir codehead started in, current branch first; another repo: its dir,
    /// a new branch + worktree first; a home repo without a clone: a new worktree after the clone.
    /// 0 git repo: a toast, the repo stays.
    pub(super) fn set_repo(&mut self, entry: crate::screens::start_screen::repo_pick::Entry) {
        let root = if entry.clone.is_some() {
            Some(entry.path.clone())
        } else {
            threads::repo_root(&entry.path)
        };

        let Some(root) = root else {
            return self.toast(
                format!("{}: 0 git repo", tilde(&entry.path.display().to_string())),
                true,
            );
        };

        self.repository.repo_pick = None;
        self.compose.target = None;
        let start = self.repository.start_cwd.starts_with(&entry.path)
            && threads::repo_root(&self.repository.start_cwd).as_ref() == Some(&root);

        let cwd = if start {
            self.repository.start_cwd.clone()
        } else {
            entry.path.clone()
        };

        self.cfg.repo = Some(root.clone());
        self.cfg.cwd.clone_from(&cwd);

        apply_repository_choices(self, &entry, &root, start, &cwd);

        let name = root
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();

        self.repository.repo = Some(crate::infra::home::Repo {
            name,
            path: root,
            url: entry.clone,
        });
        self.flow_selection.flow_default = None;
        self.load_task_flows();
    }

    /// The setting `repo` (DIR): the where row starts on DIR's repo, as if picked. DIR in the
    /// repo codehead started in: 0 change.
    pub(crate) fn use_default_repo(&mut self, dir: &Path) {
        let Some(root) = threads::repo_root(dir) else { return };

        if self.cfg.repo.as_ref() == Some(&root) {
            return;
        }

        let name = root
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();

        self.set_repo(crate::screens::start_screen::repo_pick::Entry {
            name,
            path: root,
            git: true,
            clone: None,
        });
    }

    /// Keys of the open repo picker.
    pub(super) fn repo_pick_key(&mut self, key: KeyEvent) {
        let Some(picker) = self.repository.repo_pick.as_mut() else {
            return;
        };

        match picker.key(key) {
            crate::screens::start_screen::repo_pick::Act::None => {}
            crate::screens::start_screen::repo_pick::Act::Close => self.repository.repo_pick = None,
            crate::screens::start_screen::repo_pick::Act::Pick(entry) => self.set_repo(entry),
        }
    }

    /// The where row's repo, when it needs a clone first: (url, path).
    pub(super) fn clone_first(&self) -> Option<(String, PathBuf)> {
        let repo = self.repository.repo.as_ref()?;

        (!repo.path.join(".git").exists())
            .then(|| repo.url.clone().map(|url| (url, repo.path.clone())))
            .flatten()
    }
}

fn apply_repository_choices(
    app: &mut App,
    entry: &super::repo_pick::Entry,
    root: &std::path::Path,
    start: bool,
    cwd: &std::path::Path,
) {
    let choices = if entry.clone.is_some() {
        Choices {
            wheres: vec![Where::NewWorktree],
            here_dir: root.display().to_string(),
            ..Default::default()
        }
    } else if cfg!(test) {
        where_choices(root, cwd)
    } else {
        // the branches: from the background read (`tick`)
        let (repo_dir, cwd_dir) = (root.to_path_buf(), cwd.to_path_buf());

        app.repository
            .where_reads
            .request(root.to_path_buf(), move || where_choices(&repo_dir, &cwd_dir));
        quick_choices(root, cwd)
    };

    app.repository.wheres = choices.wheres;
    app.repository.checkouts = choices.checkouts;
    app.repository.repo_branch = choices.repo_branch;
    app.repository.here_branch = choices.here_branch;
    app.repository.here_dir = choices.here_dir;

    app.repository.where_i = if start {
        0
    } else {
        app.repository
            .wheres
            .iter()
            .position(|choice| *choice == Where::NewWorktree)
            .unwrap_or(0)
    };
}
