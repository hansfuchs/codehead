use super::{McpTab, Msg, servers};

impl McpTab {
    /// Background results in. true: something changed (redraw).
    pub(crate) fn poll(&mut self) -> bool {
        let mut any = false;

        poll_jobs(&mut any, self);

        any
    }
}

fn finish_server_save(result: Result<(servers::Server, Vec<servers::Server>), String>, mcp_tab: &mut McpTab) {
    mcp_tab.jobs.busy.pop();

    if let Some(edit_form) = mcp_tab.saving.take() {
        let modal = super::key_helpers::edit_saved(edit_form, result, mcp_tab);

        // another modal opened meanwhile: it stays
        if mcp_tab.modal.is_none() {
            mcp_tab.modal = modal;
        }
    }
}

fn poll_jobs(any: &mut bool, mcp_tab: &mut McpTab) {
    while let Ok(message) = mcp_tab.jobs.rx.try_recv() {
        *any = true;

        match message {
            Msg::Refreshed(found) => {
                mcp_tab.jobs.refreshing = false;
                mcp_tab.natives = found.natives;
                mcp_tab.jobs.cli_errors = found.errors;
            }
            Msg::Probed => mcp_tab.jobs.probing = false,
            Msg::Saved(result) => {
                finish_server_save(result, mcp_tab);
            }
            Msg::Imported { harness, name, result } => {
                mcp_tab.jobs.busy.pop();
                super::imported(&harness, &name, result, mcp_tab);
            }
            Msg::Done(text, error) => {
                mcp_tab.jobs.busy.pop();
                super::say(text, error, mcp_tab);
            }
        }

        mcp_tab.statuses = servers::statuses();
    }
}
