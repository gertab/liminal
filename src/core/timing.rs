use crate::core::message::Message;
///! Timing utilities and helper functions for consistent timing semantics across processors
use std::time::{Duration, SystemTime};

/// Simple utility to get current timestamp in milliseconds since epoch
/// Kept for backwards compatibility with legacy code
pub fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_else(|e| {
            tracing::warn!("SystemTime error: {e}");
            0
        })
}

/// Milliseconds since the epoch for a point in time (0 for times before the epoch), e.g. for
/// comparing a message's `timing.event_time` with millisecond window bounds.
pub fn epoch_millis(time: SystemTime) -> u64 {
    time.duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as u64)
}

/// Configuration for timing behavior in processing stages
#[derive(Debug, Clone)]
pub struct TimingConfig {
    /// Watermark generation strategy
    pub watermark_strategy: WatermarkStrategy,

    /// Maximum allowed lateness for out-of-order events
    pub max_lateness: Duration,

    /// Bounds on acceptable jitter for real-time processing
    pub jitter_bounds: Option<Duration>,

    /// Clock synchronization policy
    pub clock_source: ClockSource,

    /// Whether to enable timing metrics collection
    pub metrics_enabled: bool,
}

impl Default for TimingConfig {
    fn default() -> Self {
        Self {
            watermark_strategy: WatermarkStrategy::None,
            max_lateness: Duration::from_secs(30),
            jitter_bounds: None,
            clock_source: ClockSource::System,
            metrics_enabled: true,
        }
    }
}

/// Strategy for generating watermarks in event streams
#[derive(Debug, Clone)]
pub enum WatermarkStrategy {
    /// No watermarks - accept all events regardless of order
    None,

    /// Generate watermarks periodically based on wall-clock time
    Periodic { interval: Duration },

    /// Generate watermarks based on punctuation in the event stream
    Punctuated { field: String },

    /// Heuristic watermarks based on event distribution
    Heuristic { percentile: f64 },
}

/// Clock source for timing operations
#[derive(Debug, Clone)]
pub enum ClockSource {
    /// Use system clock (wall-clock time)
    System,

    /// Use logical clock (monotonic ordering)
    Logical,

    /// Hybrid logical clock (combines logical and wall-clock)
    Hybrid,
}

/// Manages watermark generation and propagation
#[derive(Debug)]
pub struct WatermarkManager {
    config: TimingConfig,
    last_watermark: Option<SystemTime>,
    last_periodic_update: SystemTime,
    event_timestamps: Vec<SystemTime>, // For heuristic watermarks
}

impl WatermarkManager {
    pub fn new(config: TimingConfig) -> Self {
        Self {
            config,
            last_watermark: None,
            last_periodic_update: SystemTime::now(),
            event_timestamps: Vec::new(),
        }
    }

    /// Update watermark based on incoming message
    pub fn update_watermark(&mut self, message: &Message) -> Option<SystemTime> {
        match &self.config.watermark_strategy {
            WatermarkStrategy::None => None,

            WatermarkStrategy::Periodic { interval } => {
                let now = SystemTime::now();
                if now
                    .duration_since(self.last_periodic_update)
                    .unwrap_or(Duration::ZERO)
                    >= *interval
                {
                    self.last_periodic_update = now;
                    let watermark = now - self.config.max_lateness;
                    self.last_watermark = Some(watermark);
                    Some(watermark)
                } else {
                    None
                }
            }

            WatermarkStrategy::Punctuated { field } => {
                // Check if message contains watermark field
                if let Some(watermark_value) =
                    TimingHelpers::extract_timestamp_field(&message.payload, field)
                {
                    let watermark = watermark_value - self.config.max_lateness;
                    self.last_watermark = Some(watermark);
                    Some(watermark)
                } else {
                    None
                }
            }

            WatermarkStrategy::Heuristic { percentile } => {
                // Maintain sliding window of event timestamps
                self.event_timestamps.push(message.timing.event_time);

                // Keep only recent events (e.g., last 1000)
                if self.event_timestamps.len() > 1000 {
                    self.event_timestamps.remove(0);
                }

                if self.event_timestamps.len() >= 10 {
                    let mut sorted = self.event_timestamps.clone();
                    sorted.sort();
                    let index = ((sorted.len() as f64) * percentile / 100.0) as usize;
                    let watermark = sorted.get(index).copied().unwrap_or(SystemTime::now())
                        - self.config.max_lateness;
                    self.last_watermark = Some(watermark);
                    Some(watermark)
                } else {
                    None
                }
            }
        }
    }

    /// Get current watermark
    pub fn current_watermark(&self) -> Option<SystemTime> {
        self.last_watermark
    }
}

/// Helper functions for consistent timing operations across processors
pub struct TimingHelpers;

impl TimingHelpers {
    /// Extract event time from message payload using a field path
    /// Returns current time if field not found or invalid
    pub fn extract_event_time(payload: &serde_json::Value, field_path: &str) -> SystemTime {
        Self::extract_timestamp_field(payload, field_path).unwrap_or_else(SystemTime::now)
    }

    /// Extract timestamp from a specific field in the payload
    pub fn extract_timestamp_field(
        payload: &serde_json::Value,
        field_path: &str,
    ) -> Option<SystemTime> {
        use crate::processors::common::field_utils::FieldUtils;

        if let Some(field_value) = FieldUtils::extract_field_value(payload, field_path) {
            match field_value {
                serde_json::Value::Number(n) => {
                    if let Some(timestamp_ms) = n.as_u64() {
                        // Assume milliseconds since epoch
                        std::time::UNIX_EPOCH.checked_add(Duration::from_millis(timestamp_ms))
                    } else if let Some(timestamp_f) = n.as_f64() {
                        // Handle floating point timestamps (seconds.fraction)
                        let secs = timestamp_f.floor() as u64;
                        let nanos = ((timestamp_f.fract()) * 1_000_000_000.0) as u32;
                        std::time::UNIX_EPOCH.checked_add(Duration::new(secs, nanos))
                    } else {
                        None
                    }
                }
                serde_json::Value::String(s) => {
                    // Try to parse ISO 8601 timestamp
                    Self::parse_iso_timestamp(s)
                }
                _ => None,
            }
        } else {
            None
        }
    }

    /// Parse ISO 8601 timestamp string
    pub fn parse_iso_timestamp(timestamp_str: &str) -> Option<SystemTime> {
        chrono::DateTime::parse_from_rfc3339(timestamp_str)
            .ok()
            .map(SystemTime::from)
    }

    /// Create a message with timing information propagated from source
    pub fn propagate_timing(
        source_message: &Message,
        new_source: &str,
        new_topic: &str,
        new_payload: serde_json::Value,
    ) -> Message {
        let mut new_message = Message::new(new_source, new_topic, new_payload);

        // Propagate timing information from source
        new_message.timing.event_time = source_message.timing.event_time;
        new_message.timing.watermark = source_message.timing.watermark;
        new_message.timing.sequence_id = source_message.timing.sequence_id;
        new_message.timing.trace_id = source_message.timing.trace_id.clone();

        // Propagate deadline if not exceeded
        if let Some(deadline) = source_message.timing.processing_deadline {
            if SystemTime::now() < deadline {
                new_message.timing.processing_deadline = Some(deadline);
            }
        }

        new_message
    }

    /// Update watermark in a message if a new one is available
    pub fn update_message_watermark(
        mut message: Message,
        watermark: Option<SystemTime>,
    ) -> Message {
        if let Some(wm) = watermark {
            message.timing.watermark = Some(wm);
        }
        message
    }

    /// Check if a message should be dropped due to timing constraints
    pub fn should_drop_message(message: &Message, config: &TimingConfig) -> bool {
        // Check deadline
        if message.timing.is_deadline_exceeded() {
            return true;
        }

        // Check if message is too late relative to watermark
        if message.timing.is_late() {
            return true;
        }

        // Check jitter bounds
        if let Some(jitter_bound) = config.jitter_bounds {
            let latency = message.timing.processing_latency();
            if latency > jitter_bound {
                return true;
            }
        }

        false
    }

    /// Add processing deadline to a message based on timing configuration
    pub fn add_processing_deadline(mut message: Message, processing_timeout: Duration) -> Message {
        let deadline = SystemTime::now() + processing_timeout;
        message.timing.processing_deadline = Some(deadline);
        message
    }

    /// Get timing metrics for a message
    pub fn get_timing_metrics(message: &Message) -> TimingMetrics {
        TimingMetrics {
            processing_latency: message.timing.processing_latency(),
            time_until_deadline: message.timing.time_until_deadline(),
            is_late: message.timing.is_late(),
            event_time: message.timing.event_time,
            ingestion_time: message.timing.ingestion_time,
        }
    }
}

/// Timing metrics for monitoring and debugging
#[derive(Debug, Clone)]
pub struct TimingMetrics {
    pub processing_latency: Duration,
    pub time_until_deadline: Option<Duration>,
    pub is_late: bool,
    pub event_time: SystemTime,
    pub ingestion_time: SystemTime,
}

/// Processor mixin trait for consistent timing behavior
pub trait TimingProcessor {
    /// Get timing configuration for this processor
    fn timing_config(&self) -> &TimingConfig;

    /// Get watermark manager for this processor
    fn watermark_manager(&mut self) -> &mut WatermarkManager;

    /// Process a message with timing semantics applied
    fn process_with_timing(&mut self, message: Message) -> Option<Message> {
        // Check if message should be dropped due to timing constraints
        if TimingHelpers::should_drop_message(&message, self.timing_config()) {
            tracing::debug!("Dropping message due to timing constraints");
            return None;
        }

        // Update watermark
        let watermark = self.watermark_manager().update_watermark(&message);
        let updated_message = TimingHelpers::update_message_watermark(message, watermark);

        Some(updated_message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_timing_info_creation() {
        use crate::core::message::TimingInfo;
        let timing = TimingInfo::now();
        assert!(timing.processing_latency() < Duration::from_millis(10)); // Should be very small
    }

    #[test]
    fn test_message_with_timing() {
        let msg = Message::new("test", "topic", json!({"value": 42}));
        assert!(!msg.timing.is_deadline_exceeded());
        assert!(!msg.timing.is_late());
    }

    #[test]
    fn test_watermark_manager() {
        let config = TimingConfig {
            watermark_strategy: WatermarkStrategy::Periodic {
                interval: Duration::from_millis(100),
            },
            ..Default::default()
        };

        let mut manager = WatermarkManager::new(config);
        let msg = Message::new("test", "topic", json!({"value": 42}));

        // First update should generate watermark due to interval
        std::thread::sleep(Duration::from_millis(110));
        let watermark = manager.update_watermark(&msg);
        assert!(watermark.is_some());
    }

    #[test]
    fn parses_rfc3339_timestamp_with_z() {
        let parsed = TimingHelpers::parse_iso_timestamp("2026-08-30T10:41:07.393324Z")
            .expect("timestamp parses");
        let millis = parsed
            .duration_since(std::time::UNIX_EPOCH)
            .expect("timestamp after epoch")
            .as_millis();

        assert_eq!(millis, 1_788_086_467_393);
    }

    #[test]
    fn parses_rfc3339_timestamp_with_offset() {
        let zulu =
            TimingHelpers::parse_iso_timestamp("2026-08-30T10:41:07Z").expect("z timestamp parses");
        let offset = TimingHelpers::parse_iso_timestamp("2026-08-30T12:41:07+02:00")
            .expect("offset timestamp parses");

        assert_eq!(zulu, offset);
    }

    #[test]
    fn rejects_invalid_iso_timestamp() {
        assert!(TimingHelpers::parse_iso_timestamp("not a timestamp").is_none());
    }

    #[test]
    fn extracts_string_event_time_from_payload() {
        let event_time = TimingHelpers::extract_timestamp_field(
            &json!({ "timestamp": "2026-08-30T10:41:07Z" }),
            "timestamp",
        )
        .expect("timestamp extracts");

        assert_eq!(
            event_time,
            TimingHelpers::parse_iso_timestamp("2026-08-30T10:41:07Z").unwrap()
        );
    }
}
