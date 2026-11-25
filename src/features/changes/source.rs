use super::{Changes, Path, commits, git, order, parse};

/// The pull request of a PR-mode view: its commits (the panel's picker), the picked 1 (None:
/// every commit, against the merge base), its GitHub threads (under their lines).
#[derive(Clone, Debug, Default)]
pub(crate) struct PrSrc {
    pub pr: crate::infra::gh::PrRef,
    pub base: String,
    /// The head commit of the last read.
    pub head: String,
    pub commits: Vec<crate::infra::gh::Commit>,
    pub commit: Option<String>,
    pub threads: Vec<crate::infra::gh::Thread>,
}

/// The lines of GitHub thread T (head: the author row) in W columns: the 1st comment, then
/// its replies, 6 text lines each at most.
pub(super) fn gh_lines(thread: &crate::infra::gh::Thread, width: usize) -> Vec<(bool, String)> {
    let mut out = Vec::new();

    for (key, character) in std::iter::once(&thread.root).chain(&thread.replies).enumerate() {
        let mark = if key == 0 { "◈" } else { "  ↳" };

        let state = if key == 0 && thread.resolved {
            " · resolved"
        } else if key == 0 && thread.outdated {
            " · outdated"
        } else {
            ""
        };

        out.push((
            true,
            format!(
                "{mark} {} · {}{state}",
                character.author,
                crate::infra::gh::stamp(character.created)
            ),
        ));
        let indent = if key == 0 { "   " } else { "     " };
        let body: Vec<&str> = character.body.lines().filter(|line| !line.trim().is_empty()).collect();

        for line_value in body.iter().take(6) {
            out.push((
                false,
                format!(
                    "{indent}{}",
                    crate::ui::thread_card::fit(line_value, width.saturating_sub(indent.len()))
                ),
            ));
        }

        if body.len() > 6 {
            out.push((false, format!("{indent}… {} more lines", body.len() - 6)));
        }
    }

    out
}

/// The diff of pull request SRC (its picked commit, else all) as changes.
pub(super) fn load_pr(src: &PrSrc) -> Result<Changes, String> {
    let text = crate::infra::gh::diff(&src.pr, src.commit.as_deref())?;
    let mut files = parse(&text);

    order(&mut files);

    let base = match &src.commit {
        Some(character) => format!("{}^", &character[..character.len().min(7)]),
        None => src.base.clone(),
    };

    Ok(Changes {
        base,
        files,
        ..Default::default()
    })
}

/// The diff of commit C of DIR (against its 1st parent; a root commit: all new) as changes.
pub(super) fn load_commit(dir: &Path, character: &commits::Commit) -> Result<Changes, String> {
    let text = git(
        dir,
        &[
            "show",
            "--format=",
            "--diff-merges=first-parent",
            "-M",
            "--no-color",
            "--no-ext-diff",
            "-U3",
            &character.oid,
        ],
    )?;
    let mut files = parse(&text);

    order(&mut files);
    Ok(Changes {
        base: format!("{}^", character.short),
        rev: character.oid.clone(),
        files,
        ..Default::default()
    })
}
