use super::{AgentsFocus, Color, HashMap, Rect, chat};

pub(super) struct AgentState {
    pub(super) roster: Vec<crate::domain::agents::Agent>,
    pub(super) members: Vec<String>,
    pub(super) colors: chat::Colors,
    pub(super) names: Vec<(String, Color)>,
    /// Agents of the roster: (name, harness), for the hints of the stream.
    pub(super) harness_of: Vec<(String, String)>,
    /// Per roster agent: sprite, effort label of its card (read on roster change, not per frame).
    pub(super) profiles: HashMap<String, (crate::domain::sprites::Sprite, String)>,
    /// Agents box order: members, the last @named first (members[0] stays the lead).
    pub(super) order: Vec<String>,
    /// Agents @named before they joined: they join at the front.
    pub(super) front: Vec<String>,
    /// Spawns under way, by agent.
    pub(super) spawns: HashMap<String, crate::domain::avatar::Spawn>,
    /// The sprite slots of the last draw (agent, its 7x4 rect): who is in view, where a spawn goes.
    pub(super) slots: std::cell::RefCell<Vec<(String, Rect)>>,
    /// The flow lines of the cards of the last draw (agent, its rect): a click opens its flow box.
    pub(super) flow_slots: std::cell::RefCell<Vec<(String, Rect)>>,
    /// tab: the agents box has the keys (None: the prompt or a view).
    pub(super) agents_focus: Option<AgentsFocus>,
    /// The next sent message goes to this agent (a flow wait's reply), not the default.
    pub(super) reply_to: Option<String>,
    /// The agents of the menu's agent rows, as the menu opened (ids stay valid after a join).
    pub(super) menu_agents: Vec<String>,
}

impl AgentState {
    pub(super) fn new() -> Self {
        Self {
            roster: Vec::new(),
            members: Vec::new(),
            colors: HashMap::new(),
            names: Vec::new(),
            harness_of: Vec::new(),
            profiles: HashMap::new(),
            order: Vec::new(),
            front: Vec::new(),
            spawns: HashMap::new(),
            slots: std::cell::RefCell::new(Vec::new()),
            flow_slots: std::cell::RefCell::new(Vec::new()),
            agents_focus: None,
            reply_to: None,
            menu_agents: Vec::new(),
        }
    }
}
