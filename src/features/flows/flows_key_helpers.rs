use super::{
    Ask, Edit, Flow, FlowsTab, Focus, Gate, KeyCode, KeyEvent, KeyModifiers, Layer, Node, Out, Outcome, PickRow, Step,
    Sub, Target, TextArea, flows, input,
};
use std::ops::ControlFlow;

impl FlowsTab {
    pub(super) fn list_key(&mut self, key: KeyEvent) -> Out {
        list_key_flows_tab(self, key)
    }

    fn list_key_body_state(&mut self) {
        self.sub = if self.sub == Sub::Flows { Sub::Steps } else { Sub::Flows };
        self.at = 0;
        self.filter.clear();
        self.reload();
    }

    pub(super) fn rail_key(&mut self, key: KeyEvent) {
        rail_key_flows_tab(self, key);
    }

    pub(super) fn target_key(&mut self, key: KeyEvent, thread: Target) {
        target_key_flows_tab(self, key, thread);
    }

    pub(super) fn ask_key(&mut self, key_value: KeyEvent) {
        ask_key_flows_tab(self, key_value);
    }

    pub(super) fn picker_key(&mut self, key_value: KeyEvent) {
        picker_key_flows_tab(self, key_value);
    }

    pub(super) fn editor_key(&mut self, key_value: KeyEvent) {
        editor_key_flows_tab(self, key_value);
    }
}

fn editor_key_body(thread: &mut FlowsTab) {
    thread.focus = if thread.sub == Sub::Flows {
        Focus::Rail
    } else {
        Focus::List
    }
}

fn rail_key_keycode_char_g_if(file: &Flow, view_state: &mut FlowsTab) {
    let lib_gate = file.steps[view_state.node]
        .uses
        .as_deref()
        .and_then(|user| view_state.lib.step(user))
        .is_some_and(|text| text.gate);

    if let Some(nd) = view_state
        .flow
        .as_mut()
        .and_then(|fl| fl.steps.get_mut(view_state.node))
    {
        rail_key_keycode_char_g_if_body(lib_gate, nd);
    }

    view_state.save_flow();
}

fn rail_key_keycode_char_g_if_body(lib_gate: bool, nd: &mut Node) {
    let on = match &nd.gate {
        Some(Gate::Flag(bytes)) => *bytes,
        Some(Gate::Edges(_)) => true,
        None => lib_gate,
    };

    nd.gate = if on {
        if lib_gate { Some(Gate::Flag(false)) } else { None }
    } else if lib_gate {
        None
    } else {
        Some(Gate::Flag(true))
    };
}

fn picker_key_keycode_enter_keycode_tab(ta: TextArea, cur: usize, rows: &[PickRow], view_state: &mut FlowsTab) {
    let Some(row) = rows.get(cur).cloned() else {
        view_state.picker = Some((ta, cur));
        return;
    };

    let node = match &row {
        PickRow::Step(text_value) => Node {
            uses: Some(text_value.clone()),
            ..Default::default()
        },
        PickRow::OneOff(thread) => Node {
            text: Some(thread.clone()),
            ..Default::default()
        },
        PickRow::NewStep(name) => {
            let text_2 = Step {
                name: name.clone(),
                text: ta.text().trim().to_string(),
                layer: Layer::User,
                ..Default::default()
            };

            if !view_state.save_step(&text_2) {
                view_state.picker = Some((ta, cur));
                return;
            }

            Node {
                uses: Some(name.clone()),
                ..Default::default()
            }
        }
    };

    let at = match view_state.flow.as_ref() {
        Some(file) if file.steps.is_empty() => 0,
        Some(_) => view_state.node + 1,
        None => return,
    };

    if let Some(file_value) = view_state.flow.as_mut() {
        file_value.steps.insert(at, node);
    }

    view_state.node = at;
    view_state.save_flow();

    if !matches!(row, PickRow::Step(_)) {
        view_state.open_node_editor(at);
    }

    // a new text: straight into its editor
}

fn list_key_keycode_char_d(view_state: &mut FlowsTab) {
    let base = match view_state.sub {
        Sub::Flows => view_state.flow.as_ref().map(|file| format!("{} copy", file.name)),
        Sub::Steps => view_state
            .cur_step()
            .map(|text_value| view_state.lib.free_step_name(&format!("{}-copy", text_value.name))),
    };

    if let Some(bytes) = base {
        view_state.ask = Some((
            if view_state.sub == Sub::Flows {
                Ask::DuplicateFlow
            } else {
                Ask::DuplicateStep
            },
            input(true, "name", &bytes),
        ));
    }
}

fn rail_key_directory(key: KeyEvent, view_state: &mut FlowsTab) {
    let directory: i64 = if key.code == KeyCode::Char('+') { 1 } else { -1 };

    if let Some(line) = view_state
        .flow
        .as_mut()
        .and_then(|fl| fl.steps.get_mut(view_state.node))
        .and_then(|nd| nd.loops.as_mut())
    {
        line.max = (i64::from(line.max) + directory).clamp(1, 99) as u32;
    }

    view_state.save_flow();
}

fn rail_key_branch(count: usize, view_state: &mut FlowsTab) -> ControlFlow<()> {
    if !view_state.armed {
        view_state.armed = true;
        return ControlFlow::Break(());
    }

    view_state.armed = false;

    if let Some(fl) = view_state.flow.as_mut() {
        fl.steps.remove(view_state.node);
    }

    view_state.node = view_state.node.min(count.saturating_sub(2));
    view_state.save_flow();

    ControlFlow::Continue(())
}

fn rail_key_branch_state(file: &Flow, count: usize, view_state: &mut FlowsTab) {
    if file.steps[view_state.node].loops.is_some() {
        if let Some(nd) = view_state
            .flow
            .as_mut()
            .and_then(|fl| fl.steps.get_mut(view_state.node))
        {
            nd.loops = None;
        }

        view_state.save_flow();
    } else if count > 0 {
        view_state.target = Some(Target::Loop(view_state.node));
    }
}

fn list_key_body(view_state: &mut FlowsTab) {
    view_state.ask = Some((
        if view_state.sub == Sub::Flows {
            Ask::NewFlow
        } else {
            Ask::NewStep
        },
        input(true, "name", ""),
    ));
}

fn list_key_flows_tab(state: &mut FlowsTab, key: KeyEvent) -> Out {
    let count = state.list().len();

    match key.code {
        _ if crate::ui::textarea::is_filter_key(&key) => state.filtering = true,
        KeyCode::Up | KeyCode::Char('k') if state.at == 0 => return Out::Up,
        KeyCode::Up | KeyCode::Char('k') => {
            state.at -= 1;
            state.load_flow();
        }
        KeyCode::Down | KeyCode::Char('j') => {
            state.at = (state.at + 1).min(count.saturating_sub(1));
            state.load_flow();
        }
        KeyCode::Left | KeyCode::Right | KeyCode::Char('h' | 'l') => {
            state.list_key_body_state();
        }
        KeyCode::Enter => match state.sub {
            Sub::Flows if state.flow.is_some() => {
                state.focus = Focus::Rail;
                state.node = 0;
            }
            Sub::Steps => {
                list_key_flows_tab_sub_steps(state);
            }
            Sub::Flows => {}
        },
        KeyCode::Char('n') => {
            list_key_body(state);
        }
        KeyCode::Char('D') => {
            list_key_keycode_char_d(state);
        }
        KeyCode::Char('R') => {
            list_key_flows_tab_keycode_char_r(state);
        }
        // ctrl+d ctrl+d: an edited built-in back to its default (as ctrl+d in settings)
        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            if !state.reset_armed {
                state.reset_armed = true;
                state.note = Some("ctrl+d again: back to the built-in (your edits go)".into());
                return state.out();
            }

            state.reset_armed = false;
            state.reset();
        }
        KeyCode::Char('d') | KeyCode::Delete => {
            if let Some(value) = list_key_flows_tab_keycode_char_d_keycode(state) {
                return value;
            }
        }
        // r: read the flow and step files again, as on every screen
        KeyCode::Char('r') => {
            state.reload();
            state.note = Some("flows: read again".into());
        }
        KeyCode::Char('m') if state.sub == Sub::Flows => state.move_layer(),
        _ => {}
    }

    state.out()
}

fn list_key_flows_tab_keycode_char_d_keycode(state: &mut FlowsTab) -> Option<Out> {
    if !state.armed {
        state.armed = true;
        return Some(Out::None);
    }

    state.armed = false;
    state.delete();

    None
}

fn list_key_flows_tab_sub_steps(state: &mut FlowsTab) {
    if let Some(text) = state.cur_step().cloned() {
        state.open_step_editor(&text);
    }
}

fn list_key_flows_tab_keycode_char_r(state: &mut FlowsTab) {
    let cur = match state.sub {
        Sub::Flows => state.flow.as_ref().map(|file_value| file_value.name.clone()),
        Sub::Steps => state.cur_step().map(|text_2| text_2.name.clone()),
    };

    if let Some(character) = cur {
        rename_current_item(state, &character);
    }
}

fn rail_key_flows_tab(state: &mut FlowsTab, key: KeyEvent) {
    let Some(file) = state.flow.clone() else {
        state.focus = Focus::List;
        return;
    };

    let count = file.steps.len();
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    let alt = key.modifiers.contains(KeyModifiers::ALT);

    match key.code {
        KeyCode::Esc | KeyCode::Left => state.focus = Focus::List,
        KeyCode::Up | KeyCode::Char('k') if alt => state.move_node(-1),
        KeyCode::Down | KeyCode::Char('j') if alt => state.move_node(1),
        KeyCode::Char('K') => state.move_node(-1),
        KeyCode::Char('J') => state.move_node(1),
        KeyCode::Up | KeyCode::Char('k') => state.node = state.node.saturating_sub(1),
        KeyCode::Down | KeyCode::Char('j') => state.node = (state.node + 1).min(count.saturating_sub(1)),
        KeyCode::Char('a' | 'n') => {
            state.picker = Some((input(true, "step name, or text of a one-off step", ""), 0));
        }
        KeyCode::Enter | KeyCode::Char('e') if count > 0 => state.open_node_editor(state.node),
        KeyCode::Char('d') | KeyCode::Delete if count > 0 => {
            if let ControlFlow::Break(()) = rail_key_branch(count, state) {}
        }
        KeyCode::Char('g') if count > 0 => {
            rail_key_keycode_char_g_if(&file, state);
        }
        KeyCode::Char('L' | 'l') if count > 0 && !shift || key.code == KeyCode::Char('L') => {
            // a loop there: it goes; else pick its target
            rail_key_branch_state(&file, count, state);
        }
        KeyCode::Char('o') if count > 0 => state.target = Some(Target::Outcome(state.node)),
        KeyCode::Char('O') if count > 0 => {
            if let Some(nd) = state.flow.as_mut().and_then(|fl| fl.steps.get_mut(state.node))
                && matches!(nd.gate, Some(Gate::Edges(_)))
            {
                nd.gate = Some(Gate::Flag(true));
            }

            state.save_flow();
        }
        KeyCode::Char('+' | '-') if count > 0 => {
            rail_key_directory(key, state);
        }
        KeyCode::Char('s') if count > 0 => {
            if file.steps[state.node].uses.is_some() {
                return state.fail("a shared step already");
            }

            let base = state
                .lib
                .free_step_name(&flows::slug(&flows::node_title(&file.steps[state.node])));

            state.ask = Some((Ask::Promote(state.node), input(true, "step name", &base)));
        }
        _ => {}
    }
}

fn target_key_flows_tab(state: &mut FlowsTab, key: KeyEvent, thread: Target) {
    let count = state.flow.as_ref().map_or(0, |file| file.steps.len());

    match key.code {
        KeyCode::Esc => {
            let (Target::Loop(i) | Target::Outcome(i)) = thread;

            state.node = i;
            state.target = None;
        }
        KeyCode::Up | KeyCode::Char('k') => state.node = state.node.saturating_sub(1),
        KeyCode::Down | KeyCode::Char('j') => state.node = (state.node + 1).min(count.saturating_sub(1)),
        KeyCode::Enter => {
            state.target = None;

            match thread {
                Target::Loop(from) => configure_loop_target(state, from),
                Target::Outcome(from) => {
                    let to = state.node;

                    state.node = from;
                    state.ask = Some((
                        Ask::Outcome(from, to),
                        input(true, "outcome label, e.g. changes", "changes"),
                    ));
                }
            }
        }
        _ => {}
    }
}

fn ask_key_flows_tab(state: &mut FlowsTab, key_value: KeyEvent) {
    let Some((what, mut ta)) = state.ask.take() else { return };

    match key_value.code {
        KeyCode::Esc => {
            state.error = None;
        }
        KeyCode::Enter => {
            let value = ta.text().trim().to_string();

            if value.is_empty() && !matches!(what, Ask::LoopUntil(..)) {
                state.ask = Some((what, ta));
                return state.fail("empty");
            }

            if let Err(error_value) = state.apply_ask(&what, &value) {
                state.ask = Some((what, ta));
                return state.fail(error_value);
            }

            state.error = None;
        }
        _ => {
            ta.key(key_value);
            state.ask = Some((what, ta));
        }
    }
}

fn picker_key_flows_tab(state: &mut FlowsTab, key_value: KeyEvent) {
    let Some((mut ta, mut cur)) = state.picker.take() else {
        return;
    };

    let rows = state.pick_rows(&ta.text());

    match key_value.code {
        KeyCode::Esc => return,
        KeyCode::Up => cur = cur.saturating_sub(1),
        KeyCode::Down => cur = (cur + 1).min(rows.len().saturating_sub(1)),
        KeyCode::Enter | KeyCode::Tab => {
            picker_key_keycode_enter_keycode_tab(ta, cur, &rows, state);
            return;
        }
        _ => {
            ta.key(key_value);
            cur = 0;
        }
    }

    state.picker = Some((ta, cur));
}

fn editor_key_flows_tab(state: &mut FlowsTab, key_value: KeyEvent) {
    let ctrl = key_value.modifiers.contains(KeyModifiers::CONTROL);

    let back = |thread: &mut FlowsTab| {
        editor_key_body(thread);
    };

    // the `#` dropdown of the field
    if let Some(ed) = state.editor.as_mut() {
        let ta = &mut ed.fields[ed.field].1;

        if state.shortcuts.key(key_value, ta) {
            return;
        }
    }

    let Some(ed) = state.editor.as_mut() else {
        return back(state);
    };

    let count = ed.fields.len();

    match key_value.code {
        // esc: closes without a save; edits only on a 2nd esc (ctrl+s saves)
        KeyCode::Esc => {
            let dirty = ed.gate != ed.gate0 || ed.fields.iter().any(|(_, thread_value, _)| thread_value.edited());

            if !crate::ui::textarea::esc_drops(&mut state.esc_at, dirty) {
                state.note = Some("esc again: drop the edits · ctrl+s: save".into());
                return;
            }

            state.editor = None;
            state.shortcuts.hide();
            back(state);
        }
        KeyCode::Char('s') if ctrl => {
            if state.store_editor() {
                state.editor = None;
                back(state);
            }
        }
        KeyCode::Char('g') if ctrl => ed.gate = !ed.gate,
        KeyCode::Char('f') if ctrl => {
            if let Edit::Node(i, Some(_)) = ed.what {
                state.editor = None;
                state.fork(i);
            }
        }
        KeyCode::Down if ed.fields[ed.field].1.on_last_row() => ed.field = (ed.field + 1) % count,
        KeyCode::Up if ed.fields[ed.field].1.on_first_row() => ed.field = ed.field.saturating_sub(1),
        _ => edit_field_key(ed, key_value, count),
    }
}

fn rename_current_item(state: &mut FlowsTab, name: &str) {
    let action = match state.sub {
        Sub::Flows => Ask::RenameFlow,
        Sub::Steps => Ask::RenameStep,
    };

    state.ask = Some((action, input(true, "name", name)));
}
fn configure_loop_target(state: &mut FlowsTab, from: usize) {
    if state.node > from {
        state.node = from;
        state.fail("a loop goes back: pick this step or one above it");
        return;
    }

    let to = state.node;

    state.node = from;
    state.ask = Some((
        Ask::LoopUntil(from, to),
        input(true, "until: the condition that ends the loop", ""),
    ));
}
fn edit_field_key(editor: &mut super::Editor, key: KeyEvent, count: usize) {
    let field = &mut editor.fields[editor.field].1;

    if matches!(field.key(key), Outcome::Handled) || key.code != KeyCode::Enter {
        return;
    }

    if field.is_single() {
        editor.field = (editor.field + 1) % count;
    } else {
        field.newline();
    }
}
