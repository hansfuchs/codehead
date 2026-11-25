use super::{FlowsTab, Sub};

impl FlowsTab {
    pub(super) fn load_flow(&mut self) {
        self.flow = self
            .list()
            .get(self.at)
            .and_then(|&i| self.lib.flows.get(i))
            .cloned()
            .filter(|_| self.sub == Sub::Flows);
        self.node = 0;
        self.rail_off = 0;
    }
}
