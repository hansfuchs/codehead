use super::{App, Choices, KeyEvent, Path, PathBuf, Target, Where, quick_choices, slug, threads, tilde, where_choices};

impl App {
    pub(super) fn target(&self) -> Result<Target, String> {
        if let Some(thread) = &self.compose.target {
            return Ok(thread.clone());
        }

        if self.cfg.repo.is_none() {
            return Ok(Target::InPlace);
        }

        if let Some(bytes) = &self.cfg.branch {
            return Ok(Target::Branch(bytes.clone()));
        }

        let worktree = self
            .repository
            .wheres
            .get(self.repository.where_i)
            .cloned()
            .unwrap_or(Where::NewWorktree);

        if worktree == Where::Current {
            return Ok(Target::Worktree {
                path: self.repository.here_dir.clone(),
                branch: self.repository.here_branch.clone(),
            });
        }

        if let Where::Branch(bytes_value) = worktree {
            let (switch, _) = self.checkout_box();
            let (stash, _) = self.stash_box();

            return Ok(Target::Checkout {
                branch: bytes_value,
                switch,
                stash,
            });
        }

        let text = slug(&self.task_title());

        if text.is_empty() {
            return Err("title needs letters or digits (branch name)".into());
        }

        let bytes_2 = format!("{}/{text}", self.cfg.prefix);

        Ok(if worktree == Where::NewBranch {
            Target::NewBranch(bytes_2)
        } else {
            Target::Branch(bytes_2)
        })
    }

    pub(super) fn repo_text(&self) -> String {
        self.cfg
            .repo
            .as_ref()
            .map(|result| tilde(&result.display().to_string()))
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
            (None, Some(Where::Branch(bytes))) => bytes.clone(),
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
            .map(|result| result.path.clone())
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
    pub(super) fn set_repo(&mut self, error: crate::screens::start_screen::repo_pick::Entry) {
        let root = if error.clone.is_some() {
            Some(error.path.clone())
        } else {
            threads::repo_root(&error.path)
        };

        let Some(root) = root else {
            return self.toast(
                format!("{}: 0 git repo", tilde(&error.path.display().to_string())),
                true,
            );
        };

        self.repository.repo_pick = None;
        self.compose.target = None;
        let start = self.repository.start_cwd.starts_with(&error.path)
            && threads::repo_root(&self.repository.start_cwd).as_ref() == Some(&root);

        let cwd = if start {
            self.repository.start_cwd.clone()
        } else {
            error.path.clone()
        };

        self.cfg.repo = Some(root.clone());
        self.cfg.cwd.clone_from(&cwd);

        apply_repository_choices(self, &error, &root, start, &cwd);

        let name = root
            .file_name()
            .map(|count| count.to_string_lossy().into_owned())
            .unwrap_or_default();

        self.repository.repo = Some(crate::infra::home::Repo {
            name,
            path: root,
            url: error.clone,
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
            .map(|count| count.to_string_lossy().into_owned())
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
        let Some(path) = self.repository.repo_pick.as_mut() else {
            return;
        };

        match path.key(key) {
            crate::screens::start_screen::repo_pick::Act::None => {}
            crate::screens::start_screen::repo_pick::Act::Close => self.repository.repo_pick = None,
            crate::screens::start_screen::repo_pick::Act::Pick(error) => self.set_repo(error),
        }
    }

    /// The where row's repo, when it needs a clone first: (url, path).
    pub(super) fn clone_first(&self) -> Option<(String, PathBuf)> {
        let result = self.repository.repo.as_ref()?;

        (!result.path.join(".git").exists())
            .then(|| result.url.clone().map(|user| (user, result.path.clone())))
            .flatten()
    }
}

fn apply_repository_choices(
    app_state: &mut App,
    error: &super::repo_pick::Entry,
    root: &std::path::Path,
    start: bool,
    cwd: &std::path::Path,
) {
    let worktree = if error.clone.is_some() {
        Choices {
            wheres: vec![Where::NewWorktree],
            here_dir: root.display().to_string(),
            ..Default::default()
        }
    } else if cfg!(test) {
        where_choices(root, cwd)
    } else {
        // the branches: from the background read (`tick`)
        let (result, directory) = (root.to_path_buf(), cwd.to_path_buf());

        app_state
            .repository
            .where_reads
            .request(root.to_path_buf(), move || where_choices(&result, &directory));
        quick_choices(root, cwd)
    };

    app_state.repository.wheres = worktree.wheres;
    app_state.repository.checkouts = worktree.checkouts;
    app_state.repository.repo_branch = worktree.repo_branch;
    app_state.repository.here_branch = worktree.here_branch;
    app_state.repository.here_dir = worktree.here_dir;

    app_state.repository.where_i = if start {
        0
    } else {
        app_state
            .repository
            .wheres
            .iter()
            .position(|worktree_value| *worktree_value == Where::NewWorktree)
            .unwrap_or(0)
    };
}
