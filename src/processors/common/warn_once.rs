//! Warnings a stage logs once each, however often their cause recurs (e.g. once per message).

use std::collections::HashSet;

#[derive(Debug)]
pub struct WarnOnce {
    stage: String,
    seen: HashSet<String>,
}

impl WarnOnce {
    pub fn new(stage: &str) -> Self {
        Self {
            stage: stage.to_string(),
            seen: HashSet::new(),
        }
    }

    /// Logs `message` the first time `id` comes up; later calls with the same `id` do nothing.
    pub fn warn(&mut self, id: &str, message: impl FnOnce() -> String) {
        if self.seen.insert(id.to_string()) {
            tracing::warn!("Stage '{}': {}", self.stage, message());
        }
    }

    /// Whether `id` has been warned about.
    pub fn has_warned(&self, id: &str) -> bool {
        self.seen.contains(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remembers_each_id_it_warned_about() {
        let mut warnings = WarnOnce::new("stage");
        for _ in 0..3 {
            warnings.warn("late", || "late message".to_string());
        }
        warnings.warn("other", || "other".to_string());

        assert_eq!(warnings.seen.len(), 2);
        assert!(warnings.has_warned("late") && warnings.has_warned("other"));
        assert!(!warnings.has_warned("missing"));
    }
}
