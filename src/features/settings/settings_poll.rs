use super::Settings;

impl Settings {
    /// Background results of the MCP tab. true: redraw.
    pub(crate) fn poll(&mut self) -> bool {
        self.mcp.poll()
    }
}
