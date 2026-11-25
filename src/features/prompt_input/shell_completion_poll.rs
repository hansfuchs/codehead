use super::{Res, ShellComp};

impl ShellComp {
    /// Results in. True: a redraw is due.
    pub(crate) fn poll(&mut self) -> bool {
        let mut any = false;

        apply_completion_results(&mut any, self);

        any
    }
}

fn cache_fish_results(line: String, rows: Vec<(String, String)>, shell: &mut ShellComp) {
    if shell.fish_rows.len() > 32 {
        shell.fish_rows.clear();
    }

    shell.fish_rows.insert(line, rows);
}

fn apply_completion_results(any: &mut bool, shell: &mut ShellComp) {
    while let Ok(response) = shell.rx.try_recv() {
        match response {
            Res::Fish(line, rows) => {
                cache_fish_results(line, rows, shell);
            }
            Res::History(history) => shell.catalog.history = Some(history),
            Res::Commands(commands) => shell.catalog.commands = Some(commands),
        }

        *any = true;
    }
}
