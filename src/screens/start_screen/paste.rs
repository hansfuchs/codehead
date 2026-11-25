use super::{App, Field, Modal, Screen, Tab};

impl App {
    pub(crate) fn paste(&mut self, text: &str) {
        self.paste_inner(text);
        self.backup();
    }

    pub(super) fn paste_inner(&mut self, text: &str) {
        self.settings_typed |= self.compose.screen == Screen::Pick && self.tab == Tab::Settings;

        if let Some(Modal::Agent(form)) = &mut self.overlays.modal
            && let Some(ta) = form.text()
        {
            ta.insert(text);
            return;
        }

        if self.overlays.modal.is_some() {
            return;
        }

        if self.compose.screen == Screen::Pick && self.tab == Tab::Settings {
            paste_inner_settings(self, text);
            return;
        }

        if self.compose.screen == Screen::Pick && self.tab == Tab::Flows {
            self.flows_tab.paste(text);
            return;
        }

        if self.compose.screen == Screen::Pick && self.tab == Tab::Projects {
            self.project_selection.projects_tab.paste(text);
            return;
        }

        match self.compose.screen {
            Screen::Compose if self.compose.field == Field::Title => self.compose.title.insert(text),
            // like the thread view: a paste on any other field goes to the prompt
            Screen::Compose => {
                let pasted = self.compose.paster.paste(text);

                self.paste_prompt(pasted);
            }
            Screen::Pick if self.thread_list.tlist.filtering => {
                self.thread_list.tlist.filter.insert(text);
                self.thread_list.tlist.cursor = 0;
            }
            Screen::Pick if self.agent_list.afilter_editing => {
                self.agent_list.afilter_ta.insert(text);
                self.sync_afilter();
            }
            Screen::Pick if self.note_list.filter_editing => {
                self.note_list.filter_ta.insert(text);
                self.sync_filter();
            }
            Screen::Pick => {}
        }
    }
}

fn paste_inner_settings(app: &mut App, text: &str) {
    if let Some(settings) = &mut app.settings {
        settings.paste(text);
    }
}
