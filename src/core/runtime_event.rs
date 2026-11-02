use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static NEXT_RUNTIME_EVENT_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RuntimeEvent {
    pub id: u64,
    pub timestamp_ms: u64,
    pub kind: RuntimeEventKind,
    pub stage_id: Option<String>,
    pub processor_type: Option<String>,
    pub channel_name: Option<String>,
    pub text: Option<String>,
    /// Structured payload, e.g. a processor's view for the GUI. Omitted from JSON when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeEventKind {
    PipelineStarting,
    PipelineStarted,
    PipelineStopped,
    StageStarting,
    StageRunning,
    StageStopped,
    MessageReceived,
    MessageEmitted,
    ProcessorError,
    /// A processor published a snapshot of its state for the GUI (in `data`).
    ProcessorView,
    /// A stage rejected a command sent to it (reason in `text`); the stage keeps running.
    CommandRejected,
}

impl RuntimeEvent {
    pub fn new(kind: RuntimeEventKind) -> Self {
        Self {
            id: NEXT_RUNTIME_EVENT_ID.fetch_add(1, Ordering::Relaxed),
            timestamp_ms: timestamp_ms(),
            kind,
            stage_id: None,
            processor_type: None,
            channel_name: None,
            text: None,
            data: None,
        }
    }

    pub fn stage(mut self, stage_id: impl Into<String>) -> Self {
        self.stage_id = Some(stage_id.into());
        self
    }

    pub fn processor_type(mut self, processor_type: impl Into<String>) -> Self {
        self.processor_type = Some(processor_type.into());
        self
    }

    pub fn channel(mut self, channel_name: impl Into<String>) -> Self {
        self.channel_name = Some(channel_name.into());
        self
    }

    pub fn text(mut self, text: impl Into<String>) -> Self {
        self.text = Some(text.into());
        self
    }

    pub fn data(mut self, data: serde_json::Value) -> Self {
        self.data = Some(data);
        self
    }
}

fn timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().try_into().unwrap_or(u64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_event_serializes_with_snake_case_kind() {
        let event = RuntimeEvent {
            id: 1,
            timestamp_ms: 42,
            kind: RuntimeEventKind::StageRunning,
            stage_id: Some("sensor".to_string()),
            processor_type: Some("mqtt_sub".to_string()),
            channel_name: Some("raw".to_string()),
            text: None,
            data: None,
        };

        let json = serde_json::to_string(&event).expect("event serializes");

        assert!(json.contains("\"kind\":\"stage_running\""));
        assert!(json.contains("\"stage_id\":\"sensor\""));
        assert!(json.contains("\"channel_name\":\"raw\""));
        assert!(!json.contains("data"), "no data field unless set: {json}");
    }

    #[test]
    fn processor_views_carry_data() {
        let event = RuntimeEvent::new(RuntimeEventKind::ProcessorView)
            .stage("floor")
            .data(serde_json::json!({"state": {"sim_ms": 20}}));
        let json = serde_json::to_string(&event).expect("event serializes");
        assert!(json.contains("\"kind\":\"processor_view\""));
        assert!(json.contains("\"data\":{\"state\":{\"sim_ms\":20}}"));
    }
}
