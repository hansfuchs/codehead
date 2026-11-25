use super::*;

fn view(id: &str) -> View {
    let (tx, _rx) = crate::infra::bus::channel();

    View::new(id, Path::new("/tmp"), "t", tx)
}

fn key(view: &mut View, code: KeyCode, modifiers: KeyModifiers) {
    view.key(KeyEvent::new(code, modifiers));
}

fn typed(view: &mut View, text: &str) {
    for character in text.chars() {
        key(view, KeyCode::Char(character), KeyModifiers::NONE);
    }
}

/// An ask ID arrives (a permission request, or a question with options a / b).
fn ask(view: &mut View, id: &str, question: bool) {
    let (tool, input) = if question {
        (
            "AskUserQuestion",
            json!({"questions": [{"question": "Which?", "header": "Q", "multiSelect": false, "options": [{"label": "a", "description": ""}, {"label": "b", "description": ""}]}]}),
        )
    } else {
        ("Bash", json!({"command": "rm -rf build"}))
    };

    view.thread.push(&store::Event {
        ts: 1,
        agent: Some("ada".into()),
        kind: store::Kind::Ask {
            id: id.into(),
            tool: tool.into(),
            input,
        },
    });
    view.sync_ask();
}

fn answers(view: &View) -> usize {
    view.ops
        .borrow()
        .iter()
        .filter(|recorded| {
            recorded.get("allow").is_some() || recorded.get("answers").is_some() || recorded.get("label").is_some()
        })
        .count()
}

#[test]
fn a_new_ask_while_the_owner_types_waits_for_tab() {
    let mut view = view("keyfix-ask-0000");

    typed(&mut view, "half");
    ask(&mut view, "p1", false);

    assert!(!view.ask.focus, "text in the prompt: the ask waits");

    // space, 1, enter: the prompt's (enter: a new line), 0 answer (no surprise allow)
    typed(&mut view, " 1");
    key(&mut view, KeyCode::Enter, KeyModifiers::NONE);

    assert_eq!(view.prompt.input.text(), "half 1\n");
    assert_eq!(answers(&view), 0, "{:?}", view.ops.borrow());

    // tab: the ask; enter: allow
    key(&mut view, KeyCode::Tab, KeyModifiers::NONE);

    assert!(view.ask.focus);

    key(&mut view, KeyCode::Enter, KeyModifiers::NONE);

    assert_eq!(answers(&view), 1);
    assert_eq!(view.prompt.input.text(), "half 1\n", "the draft stays");
}

#[test]
fn a_new_ask_takes_the_focus_on_an_empty_idle_prompt_only() {
    let mut view = view("keyfix-ask-0001");

    ask(&mut view, "p1", false);

    assert!(view.ask.focus, "empty prompt, 0 keys: the ask has the keys");

    // a key less than 1 s ago, empty prompt: the next ask waits
    view.thread.asks.clear();
    view.sync_ask();
    key(&mut view, KeyCode::PageUp, KeyModifiers::NONE);
    ask(&mut view, "p2", false);

    assert!(!view.ask.focus);
}

#[test]
fn ctrl_w_in_other_edits_the_other_text_never_the_hidden_prompt() {
    let mut view = view("keyfix-ask-0002");

    ask(&mut view, "q1", true);

    assert!(view.ask.focus);

    typed(&mut view, "ab cd");
    key(&mut view, KeyCode::Char('w'), KeyModifiers::CONTROL);

    assert_eq!(view.prompt.input.text(), "", "the prompt never got a key");

    let ask = view.thread.asks.first().cloned().unwrap();

    assert_eq!(view.ask.rows(&ask)[2].label, "ab ");

    key(&mut view, KeyCode::Char('u'), KeyModifiers::CONTROL);

    assert_eq!(view.ask.rows(&ask)[2].label, "");
}

#[test]
fn tab_completes_an_open_mention_while_an_ask_waits() {
    let mut view = view("keyfix-ask-0003");

    view.agents.roster = vec![crate::domain::agents::Agent {
        name: "ada".into(),
        ..Default::default()
    }];
    typed(&mut view, "hi @a");
    ask(&mut view, "p1", false);

    assert!(!view.ask.focus && view.mention_open());

    key(&mut view, KeyCode::Tab, KeyModifiers::NONE);

    assert!(!view.ask.focus, "tab: the dropdown's, not the ask's");
    assert_eq!(view.prompt.input.text(), "hi @ada ");
}

#[test]
fn ctrl_c_clears_the_prompt_and_ctrl_z_brings_it_back() {
    let mut view = view("keyfix-ctrlc-0000");

    typed(&mut view, "a long draft");
    key(&mut view, KeyCode::Char('c'), KeyModifiers::CONTROL);

    assert_eq!(view.prompt.input.text(), "");
    assert!(view.last_ctrl_c.is_none(), "a clear is 0 step to quit");

    key(&mut view, KeyCode::Char('z'), KeyModifiers::CONTROL);

    assert_eq!(view.prompt.input.text(), "a long draft");
}

#[test]
fn ctrl_y_redoes_in_the_prompt_0_yazi() {
    let mut view = view("keyfix-ctrly-0000");

    typed(&mut view, "ab");
    key(&mut view, KeyCode::Char('z'), KeyModifiers::CONTROL);

    assert_ne!(view.prompt.input.text(), "ab");

    key(&mut view, KeyCode::Char('y'), KeyModifiers::CONTROL);

    assert_eq!(view.prompt.input.text(), "ab");
    assert!(view.panes.yazi.is_none());
}

#[test]
fn views_switch_on_f_keys_ctrl_and_alt_digits() {
    let mut view = view("keyfix-pane-0000");

    typed(&mut view, "draft");
    key(&mut view, KeyCode::F(2), KeyModifiers::NONE);

    assert_eq!(
        view.panes.pane,
        Pane::Changes,
        "F2: changes (macOS without option-as-alt)"
    );

    key(&mut view, KeyCode::Char('1'), KeyModifiers::CONTROL);

    assert_eq!(view.panes.pane, Pane::Chat, "ctrl+1: chat");

    key(&mut view, KeyCode::Char('4'), KeyModifiers::ALT);

    assert_eq!(view.panes.pane, Pane::Files);

    key(&mut view, KeyCode::F(1), KeyModifiers::NONE);

    assert_eq!(view.panes.pane, Pane::Chat);
    assert_eq!(
        view.prompt.input.text(),
        "draft",
        "view keys type 0 chars into the prompt"
    );

    // a plain digit: text, 0 view switch
    key(&mut view, KeyCode::Char('2'), KeyModifiers::NONE);

    assert_eq!(view.panes.pane, Pane::Chat);
}

#[test]
fn a_flow_gate_waits_in_the_picker_behind_the_agents_asks() {
    let mut view = view("keyfix-gate-0000");

    typed(&mut view, "looks good");
    view.flowbars = vec![crate::features::flows::flow_bar::FlowBar::new("ada")];
    view.flowbars[0].state = Some(crate::engine::flows::run::FlowState {
        flow: "chore".into(),
        wait: Some(crate::engine::flows::run::Wait::Gate),
        ..Default::default()
    });
    view.sync_flow_ask();

    assert_eq!(view.thread.asks.len(), 1);
    assert!(!view.ask.focus, "the owner types: the gate waits for tab");

    // an agent's ask comes: it goes first, the gate stays last
    ask(&mut view, "p1", false);
    view.sync_flow_ask();

    assert_eq!(view.thread.asks.first().map(|ask| ask.id.as_str()), Some("p1"));
    assert_eq!(
        view.thread.asks.last().map(|ask| ask.tool.as_str()),
        Some(crate::engine::flows::run::WAIT_TOOL)
    );

    // the agent's ask answered (its event): the gate; tab, 1: approve with the note
    view.thread.asks.remove(0);
    view.sync_ask();
    key(&mut view, KeyCode::Tab, KeyModifiers::NONE);
    key(&mut view, KeyCode::Char('1'), KeyModifiers::NONE);
    let op = view
        .ops
        .borrow()
        .iter()
        .find(|recorded| recorded["op"] == "flow_answer")
        .cloned()
        .expect("flow_answer");

    assert_eq!(
        (op["choice"].as_str(), op["note"].as_str()),
        (Some("approve"), Some("looks good"))
    );
    assert!(view.thread.asks.is_empty());

    key(&mut view, KeyCode::Char('z'), KeyModifiers::CONTROL);

    assert_eq!(view.prompt.input.text(), "looks good", "the note: back with ctrl+z");
}
