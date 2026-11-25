use super::{Act, FlowBar, KeyCode, KeyEvent, KeyModifiers, Mode, flows, json};

impl FlowBar {
    pub(crate) fn key(&mut self, key_value: KeyEvent) -> Option<Act> {
        key_flow_bar(self, key_value)
    }

    fn key_body_state(&mut self) {
        self.key_body();
    }

    fn key_body(&mut self) {
        self.popup_state.mode = Mode::Steps;
        self.popup_state.cur = self.state.as_ref().map_or(0, |text| text.node);
    }
}

fn key_1(slug: &str, view_state: &mut FlowBar) -> Act {
    let name = view_state
        .lib
        .flow(slug)
        .map(|file| file.name.clone())
        .unwrap_or_default();

    if let Some(st) = view_state.state.as_mut() {
        st.queued = Some(flows::run::Switch {
            flow: slug.to_string(),
            start: 0,
        });
    }

    view_state.popup_state.mode = Mode::Steps;
    view_state.popup_state.open = false;
    let _ = name;

    Act::Host("flow_set", json!({"op": "flow_set", "flow": slug, "mode": "after"}))
}

fn key_keycode_enter(slug: &str, view_state: &mut FlowBar) -> Act {
    let start = view_state.popup_state.cur;

    // the chip shows the switch at once (the host confirms with its log line)
    if let Some(st) = view_state.state.as_mut().filter(|text_value| !text_value.finished) {
        st.pending = Some(flows::run::Switch {
            flow: slug.to_string(),
            start,
        });
    }

    view_state.popup_state.mode = Mode::Steps;
    view_state.popup_state.open = false;
    Act::Host(
        "flow_set",
        json!({"op": "flow_set", "flow": slug, "start": start, "mode": "now"}),
    )
}

fn key_branch(view_state: &mut FlowBar) -> Act {
    if let Some(st) = view_state.state.as_mut() {
        st.pending = None;
        st.queued = None;
    }

    Act::Host("flow_cancel", json!({"op": "flow_cancel"}))
}

fn key_keycode_char_e(view_state: &mut FlowBar) -> Act {
    let slug = view_state
        .state
        .as_ref()
        .map(|text_3| text_3.flow.clone())
        .unwrap_or_default();

    view_state.popup_state.open = false;
    Act::Edit(slug)
}

fn key_branch_state(view_state: &mut FlowBar) -> Act {
    if let Some(st) = view_state.state.as_mut() {
        st.wait = None;
    }

    Act::Host("flow_skip", json!({"op": "flow_skip"}))
}

fn key_slug(query: &str, view_state: &mut FlowBar) -> Result<Act, Option<Act>> {
    let Some(&i) = view_state.picks(query).get(view_state.popup_state.cur) else {
        return Err(Some(Act::None));
    };

    let slug = view_state.lib.flows[i].slug.clone();

    view_state.popup_state.cur = 0;

    view_state.popup_state.mode = if view_state.running() {
        Mode::How(slug)
    } else {
        Mode::Start(slug)
    };

    Ok(Act::None)
}

fn key_flow_bar(receiver_state: &mut FlowBar, key_value: KeyEvent) -> Option<Act> {
    if !receiver_state.popup_state.open {
        return None;
    }

    let armed = std::mem::take(&mut receiver_state.popup_state.del_armed);
    let ctrl = key_value.modifiers.contains(KeyModifiers::CONTROL);
    let count = receiver_state.len();

    match key_value.code {
        KeyCode::Esc => {
            return Some(escape_flow_bar(receiver_state));
        }
        KeyCode::Up => {
            receiver_state.popup_state.cur = receiver_state.popup_state.cur.saturating_sub(1);
            return Some(Act::None);
        }
        KeyCode::Down => {
            receiver_state.popup_state.cur = (receiver_state.popup_state.cur + 1).min(count.saturating_sub(1));
            return Some(Act::None);
        }
        _ => {}
    }

    let act = match receiver_state.popup_state.mode.clone() {
        Mode::Pick(query) => return pick_flow_key(receiver_state, key_value, query, ctrl),
        Mode::How(slug) => replace_flow_key(receiver_state, key_value, slug),
        Mode::Start(slug) => match key_value.code {
            KeyCode::Enter => key_keycode_enter(&slug, receiver_state),
            _ => Act::None,
        },
        Mode::Steps => current_flow_key(receiver_state, key_value, armed),
    };

    Some(act)
}

fn key_flow_bar_keycode_backspace_keycode_char(receiver_state: &mut FlowBar, query: String) -> Act {
    receiver_state.popup_state.cur = 0;
    receiver_state.popup_state.mode = Mode::Pick(query);
    Act::None
}

fn escape_flow_bar(state: &mut FlowBar) -> Act {
    match &state.popup_state.mode {
        Mode::Steps if state.detail => state.detail = false,
        Mode::Pick(_) if state.state.is_some() => state.key_body_state(),
        Mode::How(_) | Mode::Start(_) => state.pick(),
        Mode::Steps | Mode::Pick(_) => state.popup_state.open = false,
    }

    Act::None
}

fn key_flow_bar_keycode_char_j(receiver_state: &mut FlowBar) -> Act {
    Act::Host(
        "flow_jump",
        json!({"op": "flow_jump", "node": receiver_state.popup_state.cur}),
    )
}

fn key_flow_bar_keycode_char_character_if(receiver_state: &mut FlowBar, mut query: String, character: char) -> Act {
    query.push(character);
    receiver_state.popup_state.cur = 0;
    receiver_state.popup_state.mode = Mode::Pick(query);
    Act::None
}

fn pick_flow_key(receiver_state: &mut FlowBar, key_value: KeyEvent, mut query: String, ctrl: bool) -> Option<Act> {
    Some(match key_value.code {
        KeyCode::Enter | KeyCode::Tab => match key_slug(&query, receiver_state) {
            Ok(value) => value,
            Err(value) => return value,
        },
        KeyCode::Backspace | KeyCode::Char('u' | 'w' | 'h')
            if crate::ui::textarea::delete_key(&mut query, &key_value) =>
        {
            key_flow_bar_keycode_backspace_keycode_char(receiver_state, query)
        }
        KeyCode::Char(character) if !ctrl => key_flow_bar_keycode_char_character_if(receiver_state, query, character),
        _ => Act::None,
    })
}

fn replace_flow_key(receiver_state: &mut FlowBar, key_value: KeyEvent, slug: String) -> Act {
    match key_value.code {
        KeyCode::Enter => match receiver_state.popup_state.cur {
            0 => {
                receiver_state.popup_state.mode = Mode::Start(slug);
                receiver_state.popup_state.cur = 0;
                Act::None
            }
            1 => key_1(&slug, receiver_state),
            _ => {
                receiver_state.pick();
                Act::None
            }
        },
        _ => Act::None,
    }
}

fn current_flow_key(receiver_state: &mut FlowBar, key_value: KeyEvent, armed: bool) -> Act {
    match key_value.code {
        KeyCode::Enter => {
            receiver_state.detail = !receiver_state.detail;
            Act::None
        }
        KeyCode::Char('s') if receiver_state.running() => key_branch_state(receiver_state),
        KeyCode::Char('j')
            if receiver_state.state.is_some() && receiver_state.popup_state.cur < receiver_state.nodes.len() =>
        {
            key_flow_bar_keycode_char_j(receiver_state)
        }
        KeyCode::Char('c') => {
            receiver_state.pick();
            Act::None
        }
        KeyCode::Char('u')
            if receiver_state
                .state
                .as_ref()
                .is_some_and(|text_2| text_2.pending.is_some() || text_2.queued.is_some()) =>
        {
            key_branch(receiver_state)
        }
        KeyCode::Char('e') => key_keycode_char_e(receiver_state),
        // d d: the flow goes (the 1st d arms)
        KeyCode::Char('d') if receiver_state.state.is_some() && !armed => {
            receiver_state.popup_state.del_armed = true;
            Act::None
        }
        KeyCode::Char('d') if receiver_state.state.is_some() => {
            receiver_state.state = None;
            receiver_state.popup_state.open = false;
            Act::Host("flow_clear", json!({"op": "flow_clear"}))
        }
        _ => Act::None,
    }
}
