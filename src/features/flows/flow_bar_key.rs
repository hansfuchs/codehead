use super::{Act, FlowBar, KeyCode, KeyEvent, KeyModifiers, Mode, flows, json};

impl FlowBar {
    pub(crate) fn key(&mut self, key: KeyEvent) -> Option<Act> {
        key_flow_bar(self, key)
    }

    fn back_to_steps(&mut self) {
        self.popup_state.mode = Mode::Steps;
        self.popup_state.cur = self.state.as_ref().map_or(0, |run| run.node);
    }
}

fn queue_switch(slug: &str, bar: &mut FlowBar) -> Act {
    let name = bar.lib.flow(slug).map(|flow| flow.name.clone()).unwrap_or_default();

    if let Some(run) = bar.state.as_mut() {
        run.queued = Some(flows::run::Switch {
            flow: slug.to_string(),
            start: 0,
        });
    }

    bar.popup_state.mode = Mode::Steps;
    bar.popup_state.open = false;
    let _ = name;

    Act::Host("flow_set", json!({"op": "flow_set", "flow": slug, "mode": "after"}))
}

fn switch_now(slug: &str, bar: &mut FlowBar) -> Act {
    let start = bar.popup_state.cur;

    // the chip shows the switch at once (the host confirms with its log line)
    if let Some(run) = bar.state.as_mut().filter(|run| !run.finished) {
        run.pending = Some(flows::run::Switch {
            flow: slug.to_string(),
            start,
        });
    }

    bar.popup_state.mode = Mode::Steps;
    bar.popup_state.open = false;
    Act::Host(
        "flow_set",
        json!({"op": "flow_set", "flow": slug, "start": start, "mode": "now"}),
    )
}

fn undo_switch(bar: &mut FlowBar) -> Act {
    if let Some(run) = bar.state.as_mut() {
        run.pending = None;
        run.queued = None;
    }

    Act::Host("flow_cancel", json!({"op": "flow_cancel"}))
}

fn edit_flow(bar: &mut FlowBar) -> Act {
    let slug = bar.state.as_ref().map(|run| run.flow.clone()).unwrap_or_default();

    bar.popup_state.open = false;
    Act::Edit(slug)
}

fn skip_step(bar: &mut FlowBar) -> Act {
    if let Some(run) = bar.state.as_mut() {
        run.wait = None;
    }

    Act::Host("flow_skip", json!({"op": "flow_skip"}))
}

fn choose_flow(query: &str, bar: &mut FlowBar) -> Result<Act, Option<Act>> {
    let Some(&i) = bar.picks(query).get(bar.popup_state.cur) else {
        return Err(Some(Act::None));
    };

    let slug = bar.lib.flows[i].slug.clone();

    bar.popup_state.cur = 0;

    bar.popup_state.mode = if bar.running() {
        Mode::How(slug)
    } else {
        Mode::Start(slug)
    };

    Ok(Act::None)
}

fn key_flow_bar(bar: &mut FlowBar, key: KeyEvent) -> Option<Act> {
    if !bar.popup_state.open {
        return None;
    }

    let armed = std::mem::take(&mut bar.popup_state.del_armed);
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let len = bar.len();

    match key.code {
        KeyCode::Esc => {
            return Some(escape_flow_bar(bar));
        }
        KeyCode::Up => {
            bar.popup_state.cur = bar.popup_state.cur.saturating_sub(1);
            return Some(Act::None);
        }
        KeyCode::Down => {
            bar.popup_state.cur = (bar.popup_state.cur + 1).min(len.saturating_sub(1));
            return Some(Act::None);
        }
        _ => {}
    }

    let act = match bar.popup_state.mode.clone() {
        Mode::Pick(query) => return pick_flow_key(bar, key, query, ctrl),
        Mode::How(slug) => replace_flow_key(bar, key, slug),
        Mode::Start(slug) => match key.code {
            KeyCode::Enter => switch_now(&slug, bar),
            _ => Act::None,
        },
        Mode::Steps => current_flow_key(bar, key, armed),
    };

    Some(act)
}

fn set_query(bar: &mut FlowBar, query: String) -> Act {
    bar.popup_state.cur = 0;
    bar.popup_state.mode = Mode::Pick(query);
    Act::None
}

fn escape_flow_bar(state: &mut FlowBar) -> Act {
    match &state.popup_state.mode {
        Mode::Steps if state.detail => state.detail = false,
        Mode::Pick(_) if state.state.is_some() => state.back_to_steps(),
        Mode::How(_) | Mode::Start(_) => state.pick(),
        Mode::Steps | Mode::Pick(_) => state.popup_state.open = false,
    }

    Act::None
}

fn jump_to_step(bar: &mut FlowBar) -> Act {
    Act::Host("flow_jump", json!({"op": "flow_jump", "node": bar.popup_state.cur}))
}

fn push_query_char(bar: &mut FlowBar, mut query: String, character: char) -> Act {
    query.push(character);
    bar.popup_state.cur = 0;
    bar.popup_state.mode = Mode::Pick(query);
    Act::None
}

fn pick_flow_key(bar: &mut FlowBar, key: KeyEvent, mut query: String, ctrl: bool) -> Option<Act> {
    Some(match key.code {
        KeyCode::Enter | KeyCode::Tab => match choose_flow(&query, bar) {
            Ok(value) => value,
            Err(value) => return value,
        },
        KeyCode::Backspace | KeyCode::Char('u' | 'w' | 'h') if crate::ui::textarea::delete_key(&mut query, &key) => {
            set_query(bar, query)
        }
        KeyCode::Char(character) if !ctrl => push_query_char(bar, query, character),
        _ => Act::None,
    })
}

fn replace_flow_key(bar: &mut FlowBar, key: KeyEvent, slug: String) -> Act {
    match key.code {
        KeyCode::Enter => match bar.popup_state.cur {
            0 => {
                bar.popup_state.mode = Mode::Start(slug);
                bar.popup_state.cur = 0;
                Act::None
            }
            1 => queue_switch(&slug, bar),
            _ => {
                bar.pick();
                Act::None
            }
        },
        _ => Act::None,
    }
}

fn current_flow_key(bar: &mut FlowBar, key: KeyEvent, armed: bool) -> Act {
    match key.code {
        KeyCode::Enter => {
            bar.detail = !bar.detail;
            Act::None
        }
        KeyCode::Char('s') if bar.running() => skip_step(bar),
        KeyCode::Char('j') if bar.state.is_some() && bar.popup_state.cur < bar.nodes.len() => jump_to_step(bar),
        KeyCode::Char('c') => {
            bar.pick();
            Act::None
        }
        KeyCode::Char('u')
            if bar
                .state
                .as_ref()
                .is_some_and(|run| run.pending.is_some() || run.queued.is_some()) =>
        {
            undo_switch(bar)
        }
        KeyCode::Char('e') => edit_flow(bar),
        // d d: the flow goes (the 1st d arms)
        KeyCode::Char('d') if bar.state.is_some() && !armed => {
            bar.popup_state.del_armed = true;
            Act::None
        }
        KeyCode::Char('d') if bar.state.is_some() => {
            bar.state = None;
            bar.popup_state.open = false;
            Act::Host("flow_clear", json!({"op": "flow_clear"}))
        }
        _ => Act::None,
    }
}
