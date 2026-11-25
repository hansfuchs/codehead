use super::*;

fn view(id: &str) -> View {
    let (tx, _rx) = crate::infra::bus::channel();

    View::new(id, Path::new("/tmp"), "t", tx)
}

fn key(view: &mut View, character: KeyCode, message: KeyModifiers) {
    view.key(KeyEvent::new(character, message));
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
        .filter(|output| {
            output.get("allow").is_some() || output.get("answers").is_some() || output.get("label").is_some()
        })
        .count()
}

#[test]
fn a_new_ask_while_the_owner_types_waits_for_tab() {
    let mut value = view("keyfix-ask-0000");

    typed(&mut value, "half");
    ask(&mut value, "p1", false);

    assert!(!value.ask.focus, "text in the prompt: the ask waits");

    // space, 1, enter: the prompt's (enter: a new line), 0 answer (no surprise allow)
    typed(&mut value, " 1");
    key(&mut value, KeyCode::Enter, KeyModifiers::NONE);

    assert_eq!(value.prompt.input.text(), "half 1\n");
    assert_eq!(answers(&value), 0, "{:?}", value.ops.borrow());

    // tab: the ask; enter: allow
    key(&mut value, KeyCode::Tab, KeyModifiers::NONE);

    assert!(value.ask.focus);

    key(&mut value, KeyCode::Enter, KeyModifiers::NONE);

    assert_eq!(answers(&value), 1);
    assert_eq!(value.prompt.input.text(), "half 1\n", "the draft stays");
}

#[test]
fn a_new_ask_takes_the_focus_on_an_empty_idle_prompt_only() {
    let mut value = view("keyfix-ask-0001");

    ask(&mut value, "p1", false);

    assert!(value.ask.focus, "empty prompt, 0 keys: the ask has the keys");

    // a key less than 1 s ago, empty prompt: the next ask waits
    value.thread.asks.clear();
    value.sync_ask();
    key(&mut value, KeyCode::PageUp, KeyModifiers::NONE);
    ask(&mut value, "p2", false);

    assert!(!value.ask.focus);
}

#[test]
fn ctrl_w_in_other_edits_the_other_text_never_the_hidden_prompt() {
    let mut value = view("keyfix-ask-0002");

    ask(&mut value, "q1", true);

    assert!(value.ask.focus);

    typed(&mut value, "ab cd");
    key(&mut value, KeyCode::Char('w'), KeyModifiers::CONTROL);

    assert_eq!(value.prompt.input.text(), "", "the prompt never got a key");

    let agent = value.thread.asks.first().cloned().unwrap();

    assert_eq!(value.ask.rows(&agent)[2].label, "ab ");

    key(&mut value, KeyCode::Char('u'), KeyModifiers::CONTROL);

    assert_eq!(value.ask.rows(&agent)[2].label, "");
}

#[test]
fn tab_completes_an_open_mention_while_an_ask_waits() {
    let mut value = view("keyfix-ask-0003");

    value.agents.roster = vec![crate::domain::agents::Agent {
        name: "ada".into(),
        ..Default::default()
    }];
    typed(&mut value, "hi @a");
    ask(&mut value, "p1", false);

    assert!(!value.ask.focus && value.mention_open());

    key(&mut value, KeyCode::Tab, KeyModifiers::NONE);

    assert!(!value.ask.focus, "tab: the dropdown's, not the ask's");
    assert_eq!(value.prompt.input.text(), "hi @ada ");
}

#[test]
fn ctrl_c_clears_the_prompt_and_ctrl_z_brings_it_back() {
    let mut value = view("keyfix-ctrlc-0000");

    typed(&mut value, "a long draft");
    key(&mut value, KeyCode::Char('c'), KeyModifiers::CONTROL);

    assert_eq!(value.prompt.input.text(), "");
    assert!(value.last_ctrl_c.is_none(), "a clear is 0 step to quit");

    key(&mut value, KeyCode::Char('z'), KeyModifiers::CONTROL);

    assert_eq!(value.prompt.input.text(), "a long draft");
}

#[test]
fn ctrl_y_redoes_in_the_prompt_0_yazi() {
    let mut value = view("keyfix-ctrly-0000");

    typed(&mut value, "ab");
    key(&mut value, KeyCode::Char('z'), KeyModifiers::CONTROL);

    assert_ne!(value.prompt.input.text(), "ab");

    key(&mut value, KeyCode::Char('y'), KeyModifiers::CONTROL);

    assert_eq!(value.prompt.input.text(), "ab");
    assert!(value.panes.yazi.is_none());
}

#[test]
fn views_switch_on_f_keys_ctrl_and_alt_digits() {
    let mut value = view("keyfix-pane-0000");

    typed(&mut value, "draft");
    key(&mut value, KeyCode::F(2), KeyModifiers::NONE);

    assert_eq!(
        value.panes.pane,
        Pane::Changes,
        "F2: changes (macOS without option-as-alt)"
    );

    key(&mut value, KeyCode::Char('1'), KeyModifiers::CONTROL);

    assert_eq!(value.panes.pane, Pane::Chat, "ctrl+1: chat");

    key(&mut value, KeyCode::Char('4'), KeyModifiers::ALT);

    assert_eq!(value.panes.pane, Pane::Files);

    key(&mut value, KeyCode::F(1), KeyModifiers::NONE);

    assert_eq!(value.panes.pane, Pane::Chat);
    assert_eq!(
        value.prompt.input.text(),
        "draft",
        "view keys type 0 chars into the prompt"
    );

    // a plain digit: text, 0 view switch
    key(&mut value, KeyCode::Char('2'), KeyModifiers::NONE);

    assert_eq!(value.panes.pane, Pane::Chat);
}

#[test]
fn a_flow_gate_waits_in_the_picker_behind_the_agents_asks() {
    let mut value = view("keyfix-gate-0000");

    typed(&mut value, "looks good");
    value.flowbars = vec![crate::features::flows::flow_bar::FlowBar::new("ada")];
    value.flowbars[0].state = Some(crate::engine::flows::run::FlowState {
        flow: "chore".into(),
        wait: Some(crate::engine::flows::run::Wait::Gate),
        ..Default::default()
    });
    value.sync_flow_ask();

    assert_eq!(value.thread.asks.len(), 1);
    assert!(!value.ask.focus, "the owner types: the gate waits for tab");

    // an agent's ask comes: it goes first, the gate stays last
    ask(&mut value, "p1", false);
    value.sync_flow_ask();

    assert_eq!(value.thread.asks.first().map(|agent| agent.id.as_str()), Some("p1"));
    assert_eq!(
        value.thread.asks.last().map(|agent| agent.tool.as_str()),
        Some(crate::engine::flows::run::WAIT_TOOL)
    );

    // the agent's ask answered (its event): the gate; tab, 1: approve with the note
    value.thread.asks.remove(0);
    value.sync_ask();
    key(&mut value, KeyCode::Tab, KeyModifiers::NONE);
    key(&mut value, KeyCode::Char('1'), KeyModifiers::NONE);
    let op = value
        .ops
        .borrow()
        .iter()
        .find(|output| output["op"] == "flow_answer")
        .cloned()
        .expect("flow_answer");

    assert_eq!(
        (op["choice"].as_str(), op["note"].as_str()),
        (Some("approve"), Some("looks good"))
    );
    assert!(value.thread.asks.is_empty());

    key(&mut value, KeyCode::Char('z'), KeyModifiers::CONTROL);

    assert_eq!(value.prompt.input.text(), "looks good", "the note: back with ctrl+z");
}
