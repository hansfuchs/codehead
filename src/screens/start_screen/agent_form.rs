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

        opts.extend(styles.iter().map(|style| (Some(style.id.clone()), style.name.clone())));
        let (mut color, mut mode) = (4, 0);
        let mut models: Vec<String> = std::iter::once("default".to_string())
            .chain(crate::infra::config::models())
            .collect();
        let mut model = 0;
        let effort = agents::effort()
            .and_then(|level| agents::EFFORTS.iter().position(|x| *x == level))
            .unwrap_or(0);
        let profile = agents::parse_profile(edit.as_ref().and_then(|agent| agent.prompt.as_deref()), default_style);

        if let Some(agent) = &edit {
            apply_existing_agent(
                &mut name,
                &mut harness,
                &mut color,
                &mut mode,
                &mut models,
                &mut model,
                agent,
            );
        } else {
            harness.set_text("claude");
        }

        voice.set_text(&profile.voice);
        prompt.set_text(&profile.prompt);
        let style = opts.iter().position(|(id, _)| *id == profile.style).unwrap_or(0);

        // saved, else what the agent tab shows (stable pick), else (new agent) a random regi
        let sprite = profile.sprite.clone().unwrap_or_else(|| match &edit {
            Some(agent) => sprites::default_for(&agent.name),
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
        let text = self.harness.text();
        let harness = text.trim();

        harness.is_empty() || harness == "claude"
    }

    /// The harness of the harness field: the part before `/` (empty: claude).
    pub(super) fn harness_name(&self) -> String {
        let text = self.harness.text();
        let harness = text.trim().split('/').next().unwrap_or_default().trim();

        if harness.is_empty() {
            "claude".into()
        } else {
            harness.to_string()
        }
    }

    /// The model: claude → the model row's, else the part after `/` (0: "default").
    pub(super) fn model_id(&self) -> String {
        if self.is_claude() {
            return self.models[self.model].clone();
        }

        let text = self.harness.text();

        text.trim()
            .split_once('/')
            .map(|(_, model)| model.trim().to_string())
            .filter(|model| !model.is_empty())
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
        let harness = self.harness_name();

        if harness == "claude" {
            self.set_claude_model(id);
        } else if id == "default" {
            self.harness.load_text(&harness);
        } else {
            self.harness.load_text(&format!("{harness}/{id}"));
        }
    }

    fn set_claude_model(&mut self, id: &str) {
        self.harness.load_text("claude");

        self.model = self.models.iter().position(|model| model == id).unwrap_or_else(|| {
            self.models.push(id.to_string());
            self.models.len() - 1
        });
    }

    /// `claude`, `claude/opus`, or the harness text as typed (other harnesses).
    pub(super) fn harness_model(&self) -> String {
        let text = self.harness.text().trim().to_string();

        if !self.is_claude() {
            return text;
        }

        match self.models[self.model].as_str() {
            "default" => "claude".into(),
            model => format!("claude/{model}"),
        }
    }

    /// None: effortLevel unset.
    pub(super) fn effort_level(&self) -> Option<&'static str> {
        Some(agents::EFFORTS[self.effort]).filter(|level| *level != "default")
    }

    /// r: a random avatar of the category, the speed and eyes kept.
    pub(super) fn random_sprite(&mut self) {
        let current = self.sprite.avatar;
        let random = sprites::random(current.cat).avatar;

        self.sprite = Sprite::of(crate::domain::avatar::Avatar {
            idx: random.idx,
            ..current
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
                .map(|(server, _)| server.clone())
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
            .is_none_or(|harness| harness.is_empty() || harness == "claude");
        let (id, name) = &self.styles[self.style];

        match (id, claude, global) {
            (None, true, Some(global_style)) => {
                format!("claude agents still get {global_style} from ~/.claude/settings.json")
            }
            (None, _, _) => "no base style".into(),
            (Some(_), true, Some(global_style)) if global_style == name => {
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
            .map(|harness| {
                (
                    harness.name.to_string(),
                    harness.label.to_string(),
                    harness.bin.to_string(),
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
        .and_then(|agent| crate::engine::roster::get(&agent.name))
        .map(|entry| entry.mcp)
        .unwrap_or_default();

    form.mcp = crate::engine::servers::all()
        .into_iter()
        .map(|server| (server.name.clone(), own.contains(&server.name)))
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
    agent: &Agent,
) {
    name.set_text(&agent.name);

    if agent.harness == "claude" {
        select_existing_model(harness, models, model, agent);
    } else {
        harness.set_text(&agent.harness_model());
    }

    *color = agent
        .color
        .as_deref()
        .and_then(|name| agents::COLORS.iter().position(|x| *x == name))
        .unwrap_or(3);
    *mode = agents::MODES.iter().position(|name| *name == agent.mode).unwrap_or(0);
}

fn select_existing_model(harness: &mut TextArea, models: &mut Vec<String>, model: &mut usize, agent: &Agent) {
    harness.set_text("claude");

    if let Some(saved) = agent
        .model
        .as_deref()
        .filter(|saved| !saved.is_empty() && *saved != "default")
    {
        *model = models.iter().position(|x| x == saved).unwrap_or_else(|| {
            models.push(saved.to_string());
            models.len() - 1
        });
    }
}
