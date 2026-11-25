use super::PrRef;

impl PrRef {
    /// `owner/name/123` (a key for files).
    pub(crate) fn key(&self) -> String {
        format!("{}/{}", self.repo, self.number)
    }
}
