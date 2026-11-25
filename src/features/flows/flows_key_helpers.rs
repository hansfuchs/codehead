use super::{
    Ask, Edit, Flow, FlowsTab, Focus, Gate, KeyCode, KeyEvent, KeyModifiers, Layer, Node, Out, Outcome, PickRow, Step,
    Sub, Target, TextArea, flows, input,
};

impl FlowsTab {
    pub(super) fn list_key(&mut self, key: KeyEvent) -> Out {
        list_key_flows_tab(self, key)
    }

    fn toggle_sub(&mut self) {
        self.sub = if self.sub == Sub::Flows { Sub::Steps } else { Sub::Flows };
        self.at = 0;
        self.filter.clear();
        self.reload();
    }

    pub(super) fn rail_key(&mut self, key: KeyEvent) {
        rail_key_flows_tab(self, key);
    }

    pub(super) fn target_key(&mut self, key: KeyEvent, target: Target) {
        target_key_flows_tab(self, key, target);
    }

    pub(super) fn ask_key(&mut self, key: KeyEvent) {
        ask_key_flows_tab(self, key);
    }

    pub(super) fn picker_key(&mut self, key: KeyEvent) {
        picker_key_flows_tab(self, key);
    }

    pub(super) fn editor_key(&mut self, key: KeyEvent) {
        editor_key_flows_tab(self, key);
    }
}

fn leave_editor(tab: &mut FlowsTab) {
    tab.focus = if tab.sub == Sub::Flows {
        Focus::Rail
    } else {
        Focus::List
    }
}

fn toggle_gate(flow: &Flow, tab: &mut FlowsTab) {
    let lib_gate = flow.steps[tab.node]
        .uses
        .as_deref()
        .and_then(|step_name| tab.lib.step(step_name))
        .is_some_and(|step| step.gate);

    if let Some(node) = tab.flow.as_mut().and_then(|flow| flow.steps.get_mut(tab.node)) {
        flip_gate(lib_gate, node);
    }

    tab.save_flow();
}

fn flip_gate(lib_gate: bool, node: &mut Node) {
    let on = match &node.gate {
        Some(Gate::Flag(flag)) => *flag,
        Some(Gate::Edges(_)) => true,
        None => lib_gate,
    };

    node.gate = if on {
        if lib_gate { Some(Gate::Flag(false)) } else { None }
    } else if lib_gate {
        None
    } else {
        Some(Gate::Flag(true))
    };
}

fn add_picked_step(query: TextArea, cur: usize, rows: &[PickRow], tab: &mut FlowsTab) {
    let Some(row) = rows.get(cur).cloned() else {
        tab.picker = Some((query, cur));
        return;
    };

    let node = match &row {
        PickRow::Step(name) => Node {
            uses: Some(name.clone()),
            ..Default::default()
        },
        PickRow::OneOff(text) => Node {
            text: Some(text.clone()),
            ..Default::default()
        },
        PickRow::NewStep(name) => {
            let step = Step {
                name: name.clone(),
                text: query.text().trim().to_string(),
                layer: Layer::User,
                ..Default::default()
            };

            if !tab.save_step(&step) {
                tab.picker = Some((query, cur));
                return;
            }

            Node {
                uses: Some(name.clone()),
                ..Default::default()
            }
        }
    };

    let at = match tab.flow.as_ref() {
        Some(flow) if flow.steps.is_empty() => 0,
        Some(_) => tab.node + 1,
        None => return,
    };

    if let Some(flow) = tab.flow.as_mut() {
        flow.steps.insert(at, node);
    }

    tab.node = at;
    tab.save_flow();

    // a new text: straight into its editor
    if !matches!(row, PickRow::Step(_)) {
        tab.open_node_editor(at);
    }
}

fn ask_duplicate(tab: &mut FlowsTab) {
    let base = match tab.sub {
        Sub::Flows => tab.flow.as_ref().map(|flow| format!("{} copy", flow.name)),
        Sub::Steps => tab
            .cur_step()
            .map(|step| tab.lib.free_step_name(&format!("{}-copy", step.name))),
    };

    if let Some(name) = base {
        tab.ask = Some((
            if tab.sub == Sub::Flows {
                Ask::DuplicateFlow
            } else {
                Ask::DuplicateStep
            },
            input(true, "name", &name),
        ));
    }
}

fn change_loop_max(key: KeyEvent, tab: &mut FlowsTab) {
    let delta: i64 = if key.code == KeyCode::Char('+') { 1 } else { -1 };

    if let Some(loops) = tab
        .flow
        .as_mut()
        .and_then(|flow| flow.steps.get_mut(tab.node))
        .and_then(|node| node.loops.as_mut())
    {
        loops.max = (i64::from(loops.max) + delta).clamp(1, 99) as u32;
    }

    tab.save_flow();
}

fn delete_node(count: usize, tab: &mut FlowsTab) {
    if !tab.armed {
        tab.armed = true;
        return;
    }

    tab.armed = false;

    if let Some(flow) = tab.flow.as_mut() {
        flow.steps.remove(tab.node);
    }

    tab.node = tab.node.min(count.saturating_sub(2));
    tab.save_flow();
}

fn toggle_loop(flow: &Flow, node_count: usize, tab: &mut FlowsTab) {
    if flow.steps[tab.node].loops.is_some() {
        if let Some(node) = tab.flow.as_mut().and_then(|flow| flow.steps.get_mut(tab.node)) {
            node.loops = None;
        }

        tab.save_flow();
    } else if node_count > 0 {
        tab.target = Some(Target::Loop(tab.node));
    }
}

fn ask_new(tab: &mut FlowsTab) {
    tab.ask = Some((
        if tab.sub == Sub::Flows {
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
            state.toggle_sub();
        }
        KeyCode::Enter => match state.sub {
            Sub::Flows if state.flow.is_some() => {
                state.focus = Focus::Rail;
                state.node = 0;
            }
            Sub::Steps => {
                edit_current_step(state);
            }
            Sub::Flows => {}
        },
        KeyCode::Char('n') => {
            ask_new(state);
        }
        KeyCode::Char('D') => {
            ask_duplicate(state);
        }
        KeyCode::Char('R') => {
            ask_rename(state);
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
            if let Some(value) = arm_or_delete(state) {
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

fn arm_or_delete(state: &mut FlowsTab) -> Option<Out> {
    if !state.armed {
        state.armed = true;
        return Some(Out::None);
    }

    state.armed = false;
    state.delete();

    None
}

fn edit_current_step(state: &mut FlowsTab) {
    if let Some(step) = state.cur_step().cloned() {
        state.open_step_editor(&step);
    }
}

fn ask_rename(state: &mut FlowsTab) {
    let name = match state.sub {
        Sub::Flows => state.flow.as_ref().map(|flow| flow.name.clone()),
        Sub::Steps => state.cur_step().map(|step| step.name.clone()),
    };

    if let Some(name) = name {
        rename_current_item(state, &name);
    }
}

fn rail_key_flows_tab(state: &mut FlowsTab, key: KeyEvent) {
    let Some(flow) = state.flow.clone() else {
        state.focus = Focus::List;
        return;
    };

    let count = flow.steps.len();
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
            delete_node(count, state);
        }
        KeyCode::Char('g') if count > 0 => {
            toggle_gate(&flow, state);
        }
        KeyCode::Char('L' | 'l') if count > 0 && !shift || key.code == KeyCode::Char('L') => {
            // a loop there: it goes; else pick its target
            toggle_loop(&flow, count, state);
        }
        KeyCode::Char('o') if count > 0 => state.target = Some(Target::Outcome(state.node)),
        KeyCode::Char('O') if count > 0 => {
            if let Some(node) = state.flow.as_mut().and_then(|flow| flow.steps.get_mut(state.node))
                && matches!(node.gate, Some(Gate::Edges(_)))
            {
                node.gate = Some(Gate::Flag(true));
            }

            state.save_flow();
        }
        KeyCode::Char('+' | '-') if count > 0 => {
            change_loop_max(key, state);
        }
        KeyCode::Char('s') if count > 0 => {
            if flow.steps[state.node].uses.is_some() {
                return state.fail("a shared step already");
            }

            let base = state
                .lib
                .free_step_name(&flows::slug(&flows::node_title(&flow.steps[state.node])));

            state.ask = Some((Ask::Promote(state.node), input(true, "step name", &base)));
        }
        _ => {}
    }
}

fn target_key_flows_tab(state: &mut FlowsTab, key: KeyEvent, target: Target) {
    let count = state.flow.as_ref().map_or(0, |flow| flow.steps.len());

    match key.code {
        KeyCode::Esc => {
            let (Target::Loop(i) | Target::Outcome(i)) = target;

            state.node = i;
            state.target = None;
        }
        KeyCode::Up | KeyCode::Char('k') => state.node = state.node.saturating_sub(1),
        KeyCode::Down | KeyCode::Char('j') => state.node = (state.node + 1).min(count.saturating_sub(1)),
        KeyCode::Enter => {
            state.target = None;

            match target {
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

fn ask_key_flows_tab(state: &mut FlowsTab, key: KeyEvent) {
    let Some((what, mut answer)) = state.ask.take() else {
        return;
    };

    match key.code {
        KeyCode::Esc => {
            state.error = None;
        }
        KeyCode::Enter => {
            let value = answer.text().trim().to_string();

            if value.is_empty() && !matches!(what, Ask::LoopUntil(..)) {
                state.ask = Some((what, answer));
                return state.fail("empty");
            }

            if let Err(error) = state.apply_ask(&what, &value) {
                state.ask = Some((what, answer));
                return state.fail(error);
            }

            state.error = None;
        }
        _ => {
            answer.key(key);
            state.ask = Some((what, answer));
        }
    }
}

fn picker_key_flows_tab(state: &mut FlowsTab, key: KeyEvent) {
    let Some((mut query, mut cur)) = state.picker.take() else {
        return;
    };

    let rows = state.pick_rows(&query.text());

    match key.code {
        KeyCode::Esc => return,
        KeyCode::Up => cur = cur.saturating_sub(1),
        KeyCode::Down => cur = (cur + 1).min(rows.len().saturating_sub(1)),
        KeyCode::Enter | KeyCode::Tab => {
            add_picked_step(query, cur, &rows, state);
            return;
        }
        _ => {
            query.key(key);
            cur = 0;
        }
    }

    state.picker = Some((query, cur));
}

fn editor_key_flows_tab(state: &mut FlowsTab, key: KeyEvent) {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

    let back = |tab: &mut FlowsTab| {
        leave_editor(tab);
    };

    // the `#` dropdown of the field
    if let Some(editor) = state.editor.as_mut() {
        let field = &mut editor.fields[editor.field].1;

        if state.shortcuts.key(key, field) {
            return;
        }
    }

    let Some(editor) = state.editor.as_mut() else {
        return back(state);
    };

    let count = editor.fields.len();

    match key.code {
        // esc: closes without a save; edits only on a 2nd esc (ctrl+s saves)
        KeyCode::Esc => {
            let dirty = editor.gate != editor.gate0 || editor.fields.iter().any(|(_, field, _)| field.edited());

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
        KeyCode::Char('g') if ctrl => editor.gate = !editor.gate,
        KeyCode::Char('f') if ctrl => {
            if let Edit::Node(i, Some(_)) = editor.what {
                state.editor = None;
                state.fork(i);
            }
        }
        KeyCode::Down if editor.fields[editor.field].1.on_last_row() => editor.field = (editor.field + 1) % count,
        KeyCode::Up if editor.fields[editor.field].1.on_first_row() => editor.field = editor.field.saturating_sub(1),
        _ => edit_field_key(editor, key, count),
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
