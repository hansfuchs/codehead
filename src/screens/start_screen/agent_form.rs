use super::{
    Agent, F_HARNESS, F_NAME, F_PROMPT, F_VOICE, FORM_FIELDS, OutStyle, Rect, Sprite, TextArea, agents, sprites,
};

/// New / edit agent form. Fields: name, harness, model, effort, color, mode, style, sprite set, sprite, voice, system prompt, buttons.
pub(super) struct AgentForm {
    pub(super) edit: Option<Agent>,
    pub(super) name: TextArea,
    /// Harness alone (`claude`), or harness/model for other harnesses (`codex/gpt-5.5`).
    pub(super) harness: TextArea,
    /// Claude models: "default" first, then `config::models()` (+ the agent's own, if not listed).
    pub(super) models: Vec<String>,
    pub(super) model: usize,
    /// Index into `agents::EFFORTS`. Global: ~/.claude/settings.json `effortLevel`.
    pub(super) effort: usize,
    pub(super) voice: TextArea,
    pub(super) prompt: TextArea,
    pub(super) color: usize,
    pub(super) mode: usize,
    /// Index into `styles`; 0 = none.
    pub(super) style: usize,
    pub(super) styles: Vec<(Option<String>, String)>,
    pub(super) sprite: Sprite,
    /// The avatar picker, open over the form (enter on the avatar rows).
    pub(super) picker: Option<crate::features::agents::avatar_picker::Picker>,
    pub(super) field: usize,
    pub(super) save: bool,
    pub(super) error: Option<String>,
    pub(super) areas: [Rect; FORM_FIELDS - 1],
    /// `fingerprint` at open: Esc asks before it drops edits.
    pub(super) orig: String,
    /// Harnesses of the harness dropdown: the installed ones (name, label, bin).
    pub(super) harnesses: Vec<(String, String, String)>,
    /// Open dropdown of the harness / model field.
    pub(super) pick: Option<FormPick>,
    /// codehead's MCP servers (settings › mcp servers): name, on for this agent; the cursor.
    pub(super) mcp: Vec<(String, bool)>,
    pub(super) mcp_cur: usize,
}

/// The open dropdown of the agent form: harness list, or the model list of HARNESS; cursor,
/// 1st row in view, typed filter.
#[derive(Clone)]
pub(super) struct FormPick {
    pub(super) model: bool,
    pub(super) harness: String,
    pub(super) cursor: usize,
    pub(super) offset: usize,
    pub(super) query: String,
}

impl AgentForm {
    pub(super) fn new(edit: Option<Agent>, styles: &[OutStyle], default_style: Option<&str>) -> AgentForm {
        let mut name = TextArea::new(true, "lowercase name, e.g. nova");
        let mut harness = TextArea::new(
            true,
            "claude · claude/opus · codex/gpt-5.5 · opencode · goose · cursor · pi · amp …",
        );
        let mut voice = TextArea::new(false, "tone + persona, e.g. dry humour, calls the owner boss");
        let mut prompt = TextArea::new(false, "standing instructions: role, focus, rules (system prompt)");
        let mut opts: Vec<(Option<String>, String)> = vec![(None, "none".into())];

        opts.extend(styles.iter().map(|text| (Some(text.id.clone()), text.name.clone())));
        let (mut color, mut mode) = (4, 0);
        let mut models: Vec<String> = std::iter::once("default".to_string())
            .chain(crate::infra::config::models())
            .collect();
        let mut model = 0;
        let effort = agents::effort()
            .and_then(|error| agents::EFFORTS.iter().position(|x| *x == error))
            .unwrap_or(0);
        let profile = agents::parse_profile(edit.as_ref().and_then(|agent| agent.prompt.as_deref()), default_style);

        if let Some(agent_value) = &edit {
            apply_existing_agent(
                &mut name,
                &mut harness,
                &mut color,
                &mut mode,
                &mut models,
                &mut model,
                agent_value,
            );
        } else {
            harness.set_text("claude");
        }

        voice.set_text(&profile.voice);
        prompt.set_text(&profile.prompt);
        let style = opts.iter().position(|(id, _)| *id == profile.style).unwrap_or(0);

        // saved, else what the agent tab shows (stable pick), else (new agent) a random regi
        let sprite = profile.sprite.clone().unwrap_or_else(|| match &edit {
            Some(agent_2) => sprites::default_for(&agent_2.name),
            None => sprites::random(crate::domain::avatar::Cat::Regi),
        });
        let mut form = initial_form(FormValues {
            edit,
            name,
            harness,
            voice,
            prompt,
            opts,
            color,
            mode,
            models,
            model,
            effort,
            style,
            sprite,
        });

        load_form_servers(&mut form);

        form
    }

    /// The form's values in 1 string: Esc asks before it drops a form that differs from `orig`.
    pub(super) fn fingerprint(&self) -> String {
        format!(
            "{}\x1f{}\x1f{}\x1f{}\x1f{}\x1f{}\x1f{}\x1f{}\x1f{}\x1f{}\x1f{:?}",
            self.name.text(),
            self.harness.text(),
            self.model,
            self.effort,
            self.voice.text(),
            self.prompt.text(),
            self.color,
            self.mode,
            self.style,
            self.sprite.id,
            self.mcp
        )
    }

    /// Edits since the form opened.
    pub(super) fn dirty(&self) -> bool {
        self.fingerprint() != self.orig
    }

    pub(super) fn text(&mut self) -> Option<&mut TextArea> {
        match self.field {
            F_NAME => Some(&mut self.name),
            F_HARNESS => Some(&mut self.harness),
            F_VOICE => Some(&mut self.voice),
            F_PROMPT => Some(&mut self.prompt),
            _ => None,
        }
    }

    pub(super) fn multiline(&self) -> bool {
        matches!(self.field, F_VOICE | F_PROMPT)
    }

    /// Harness is claude (or empty = claude): the model row applies.
    pub(super) fn is_claude(&self) -> bool {
        let height = self.harness.text();
        let height_value = height.trim();

        height_value.is_empty() || height_value == "claude"
    }

    /// The harness of the harness field: the part before `/` (empty: claude).
    pub(super) fn harness_name(&self) -> String {
        let thread = self.harness.text();
        let height = thread.trim().split('/').next().unwrap_or_default().trim();

        if height.is_empty() {
            "claude".into()
        } else {
            height.to_string()
        }
    }

    /// The model: claude → the model row's, else the part after `/` (0: "default").
    pub(super) fn model_id(&self) -> String {
        if self.is_claude() {
            return self.models[self.model].clone();
        }

        let thread = self.harness.text();

        thread
            .trim()
            .split_once('/')
            .map(|(_, message)| message.trim().to_string())
            .filter(|message_value| !message_value.is_empty())
            .unwrap_or_else(|| "default".into())
    }

    /// Harness NAME from the dropdown: the field holds it (same harness: its model stays).
    pub(super) fn set_harness(&mut self, name: &str) {
        if name == self.harness_name() {
            return;
        }

        self.harness.load_text(name);

        if name == "claude" {
            self.model = 0;
        }
    }

    /// Model ID of the current harness from the dropdown: claude → the model row, else `harness/id`.
    pub(super) fn set_model(&mut self, id: &str) {
        let height = self.harness_name();

        if height == "claude" {
            self.set_model_body(id);
        } else if id == "default" {
            self.harness.load_text(&height);
        } else {
            self.harness.load_text(&format!("{height}/{id}"));
        }
    }

    fn set_model_body(&mut self, id: &str) {
        self.harness.load_text("claude");

        self.model = self.models.iter().position(|message| message == id).unwrap_or_else(|| {
            self.models.push(id.to_string());
            self.models.len() - 1
        });
    }

    /// `claude`, `claude/opus`, or the harness text as typed (other harnesses).
    pub(super) fn harness_model(&self) -> String {
        let height = self.harness.text().trim().to_string();

        if !self.is_claude() {
            return height;
        }

        match self.models[self.model].as_str() {
            "default" => "claude".into(),
            message => format!("claude/{message}"),
        }
    }

    /// None: effortLevel unset.
    pub(super) fn effort_level(&self) -> Option<&'static str> {
        Some(agents::EFFORTS[self.effort]).filter(|error| *error != "default")
    }

    /// r: a random avatar of the category, the speed and eyes kept.
    pub(super) fn random_sprite(&mut self) {
        let agent = self.sprite.avatar;
        let result = sprites::random(agent.cat).avatar;

        self.sprite = Sprite::of(crate::domain::avatar::Avatar {
            idx: result.idx,
            ..agent
        });
    }

    pub(super) fn profile(&self) -> agents::Profile {
        agents::Profile {
            style: self.styles[self.style].0.clone(),
            voice: self.voice.text().trim().to_string(),
            prompt: self.prompt.text().trim().to_string(),
            sprite: Some(self.sprite.clone()),
        }
    }

    pub(super) fn spec(&self, styles: &[OutStyle], global: Option<&str>) -> agents::Spec {
        let harness = self.harness_model();

        agents::Spec {
            name: self.name.text().trim().to_string(),
            prompt: agents::compose(
                &self.profile(),
                styles,
                if harness.is_empty() { "claude" } else { &harness },
                global,
            ),
            harness,
            color: Some(agents::COLORS[self.color].to_string()),
            mode: agents::MODES[self.mode].to_string(),
            mcp: self
                .mcp
                .iter()
                .filter(|(_, on)| *on)
                .map(|(count, _)| count.clone())
                .collect(),
        }
    }

    /// What the base style does for this harness.
    pub(super) fn style_note(&self, global: Option<&str>) -> String {
        let claude = self
            .harness
            .text()
            .trim()
            .split('/')
            .next()
            .is_none_or(|height| height.is_empty() || height == "claude");
        let (id, name) = &self.styles[self.style];

        match (id, claude, global) {
            (None, true, Some(group)) => format!("claude agents still get {group} from ~/.claude/settings.json"),
            (None, _, _) => "no base style".into(),
            (Some(_), true, Some(group_value)) if group_value == name => {
                "= global outputStyle: claude applies it, 0 copies in the prompt".to_string()
            }
            (Some(_), _, _) => format!("{name} goes into the agent prompt"),
        }
    }
}

struct FormValues {
    edit: Option<Agent>,
    name: TextArea,
    harness: TextArea,
    voice: TextArea,
    prompt: TextArea,
    opts: Vec<(Option<String>, String)>,
    color: usize,
    mode: usize,
    models: Vec<String>,
    model: usize,
    effort: usize,
    style: usize,
    sprite: Sprite,
}

fn initial_form(values: FormValues) -> AgentForm {
    let FormValues {
        edit,
        name,
        harness,
        voice,
        prompt,
        opts,
        color,
        mode,
        models,
        model,
        effort,
        style,
        sprite,
    } = values;

    AgentForm {
        edit,
        name,
        harness,
        models,
        model,
        effort,
        voice,
        prompt,
        color,
        mode,
        style,
        styles: opts,
        picker: None,
        sprite,
        field: 0,
        save: true,
        error: None,
        areas: [Rect::default(); FORM_FIELDS - 1],
        orig: String::new(),
        harnesses: crate::engine::harnesses::installed()
            .into_iter()
            .map(|height| {
                (
                    height.name.to_string(),
                    height.label.to_string(),
                    height.bin.to_string(),
                )
            })
            .collect(),
        pick: None,
        mcp: Vec::new(),
        mcp_cur: 0,
    }
}

fn load_form_servers(form: &mut AgentForm) {
    let own: Vec<String> = form
        .edit
        .as_ref()
        .and_then(|agent_3| crate::engine::roster::get(&agent_3.name))
        .map(|agent_4| agent_4.mcp)
        .unwrap_or_default();

    form.mcp = crate::engine::servers::all()
        .into_iter()
        .map(|text_value| (text_value.name.clone(), own.contains(&text_value.name)))
        .collect();
    form.orig = form.fingerprint();
}

fn apply_existing_agent(
    name: &mut TextArea,
    harness: &mut TextArea,
    color: &mut usize,
    mode: &mut usize,
    models: &mut Vec<String>,
    model: &mut usize,
    agent_value: &Agent,
) {
    name.set_text(&agent_value.name);

    if agent_value.harness == "claude" {
        select_existing_model(harness, models, model, agent_value);
    } else {
        harness.set_text(&agent_value.harness_model());
    }

    *color = agent_value
        .color
        .as_deref()
        .and_then(|character| agents::COLORS.iter().position(|x| *x == character))
        .unwrap_or(3);
    *mode = agents::MODES
        .iter()
        .position(|message_2| *message_2 == agent_value.mode)
        .unwrap_or(0);
}

fn select_existing_model(harness: &mut TextArea, models: &mut Vec<String>, model: &mut usize, agent_value: &Agent) {
    harness.set_text("claude");

    if let Some(message_value) = agent_value
        .model
        .as_deref()
        .filter(|message| !message.is_empty() && *message != "default")
    {
        *model = models.iter().position(|x| x == message_value).unwrap_or_else(|| {
            models.push(message_value.to_string());
            models.len() - 1
        });
    }
}
