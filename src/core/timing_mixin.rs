use crate::core::message::Message;
/// Timing mixin for processors that provides common timing functionality
/// This encapsulates all timing-related state and behavior that every processor needs
use crate::core::timing::{
    TimingConfig, TimingHelpers, WatermarkManager, WatermarkStrategy, epoch_millis,
};
use std::time::SystemTime;
use tokio::time::Duration;

/// Mixin struct that encapsulates all timing-related functionality
/// This eliminates duplication across all processor implementations
#[derive(Debug)]
pub struct TimingMixin {
    /// Internal timing configuration (converted from TOML config)
    timing_config: TimingConfig,
    /// Watermark manager for this processor
    watermark_manager: WatermarkManager,
    /// Sequence counter for message ordering
    sequence_counter: u64,
    /// Optional TOML timing configuration (for reference/debugging)
    source_config: Option<crate::config::TimingConfig>,
}

impl TimingMixin {
    /// Create a new timing mixin from processor configuration
    /// Takes the processed timing config from your processor's ProcessorConfig implementation
    /// This respects the ProcessorConfig::from_stage_config() pattern
    pub fn new(timing_config: Option<&crate::config::TimingConfig>) -> Self {
        let source_config = timing_config.cloned();

        // Convert TOML config to internal config
        let internal_timing_config = timing_config
            .map(|tc| tc.to_internal_config())
            .unwrap_or_default();

        let watermark_manager = WatermarkManager::new(internal_timing_config.clone());

        Self {
            timing_config: internal_timing_config,
            watermark_manager,
            sequence_counter: 0,
            source_config,
        }
    }

    /// Get the next sequence number and increment the counter
    pub fn next_sequence_id(&mut self) -> u64 {
        self.sequence_counter += 1;
        self.sequence_counter
    }

    /// Get current sequence counter value without incrementing
    pub fn current_sequence_id(&self) -> u64 {
        self.sequence_counter
    }

    /// Create a message with timing semantics applied
    pub fn create_message_with_timing(
        &mut self,
        source: &str,
        topic: &str,
        payload: serde_json::Value,
        event_time: SystemTime,
    ) -> Message {
        let sequence_id = self.next_sequence_id();

        let mut message = Message::new_with_event_time(source, topic, payload, event_time);
        message = message.with_sequence_id(sequence_id);

        // Add processing deadline if configured
        if let Some(ref source_config) = self.source_config {
            if let Some(timeout_ms) = source_config.processing_timeout_ms {
                message = TimingHelpers::add_processing_deadline(
                    message,
                    Duration::from_millis(timeout_ms),
                );
            }
        }

        // Update watermark
        let watermark = self.watermark_manager.update_watermark(&message);
        TimingHelpers::update_message_watermark(message, watermark)
    }

    /// Create a message with event time extracted from payload if configured
    pub fn create_message_with_event_time_extraction(
        &mut self,
        source: &str,
        topic: &str,
        payload: serde_json::Value,
        fallback_event_time: SystemTime,
    ) -> Message {
        let event_time = if let Some(ref source_config) = self.source_config {
            if let Some(event_time_field) = &source_config.event_time_field {
                match TimingHelpers::extract_timestamp_field(&payload, event_time_field) {
                    Some(event_time) => event_time,
                    None => {
                        tracing::debug!(
                            field = %event_time_field,
                            "Could not extract configured event time field; using fallback event time"
                        );
                        fallback_event_time
                    }
                }
            } else {
                fallback_event_time
            }
        } else {
            fallback_event_time
        };

        self.create_message_with_timing(source, topic, payload, event_time)
    }

    /// Update watermark for an existing message
    pub fn update_message_watermark(&mut self, message: Message) -> Message {
        let watermark = self.watermark_manager.update_watermark(&message);
        TimingHelpers::update_message_watermark(message, watermark)
    }

    /// Check if a message should be dropped due to timing constraints
    pub fn should_drop_message(&self, message: &Message) -> bool {
        TimingHelpers::should_drop_message(message, &self.timing_config)
    }

    /// Get timing configuration (for TimingProcessor trait)
    pub fn timing_config(&self) -> &TimingConfig {
        &self.timing_config
    }

    /// Get mutable watermark manager (for TimingProcessor trait)
    pub fn watermark_manager(&mut self) -> &mut WatermarkManager {
        &mut self.watermark_manager
    }

    /// Get source timing configuration (for debugging/inspection)
    pub fn source_timing_config(&self) -> Option<&crate::config::TimingConfig> {
        self.source_config.as_ref()
    }
}

/// The watermark of a stage that holds messages over time (such as `window`): how far its input
/// has progressed, so it can tell which time spans are complete.
///
/// It follows the stage's timing configuration:
/// - with a punctuated or heuristic `watermark_strategy`, the watermark that strategy derives from
///   the input messages themselves (a `WatermarkManager` that sees each one);
/// - otherwise (no strategy, or periodic), the wall clock minus the stage's `max_lateness_ms`, or
///   minus `default_lateness_ms` when the stage has no `[timing]` block. It's evaluated whenever
///   asked, so it moves on even when no messages arrive.
///
/// This is separate from the `TimingMixin`'s own watermark manager, which stamps the messages the
/// stage creates.
#[derive(Debug)]
pub enum StageWatermark {
    WallClock { lateness_ms: u64 },
    EventTime(WatermarkManager),
}

impl StageWatermark {
    pub fn for_stage(timing: &TimingMixin, default_lateness_ms: u64) -> Self {
        match &timing.timing_config().watermark_strategy {
            WatermarkStrategy::Punctuated { .. } | WatermarkStrategy::Heuristic { .. } => {
                Self::EventTime(WatermarkManager::new(timing.timing_config().clone()))
            }
            WatermarkStrategy::None | WatermarkStrategy::Periodic { .. } => Self::WallClock {
                lateness_ms: timing
                    .source_timing_config()
                    .map_or(default_lateness_ms, |config| config.max_lateness_ms),
            },
        }
    }

    /// Lets an event-time watermark see an input message.
    pub fn observe(&mut self, message: &Message) {
        if let Self::EventTime(watermarks) = self {
            watermarks.update_watermark(message);
        }
    }

    /// The watermark in epoch milliseconds, given the wall clock `now_ms`. `None` until an
    /// event-time watermark has seen enough messages to produce one. An event-time watermark can
    /// move backwards (it is the strategy's latest value).
    pub fn current_ms(&self, now_ms: u64) -> Option<u64> {
        match self {
            Self::WallClock { lateness_ms } => Some(now_ms.saturating_sub(*lateness_ms)),
            Self::EventTime(watermarks) => watermarks.current_watermark().map(epoch_millis),
        }
    }

    pub fn follows_event_time(&self) -> bool {
        matches!(self, Self::EventTime(_))
    }
}

/// Trait for processors that use the timing mixin
/// This provides a standard interface for timing operations
pub trait WithTimingMixin {
    /// Get the timing mixin
    fn timing_mixin(&self) -> &TimingMixin;

    /// Get the mutable timing mixin
    fn timing_mixin_mut(&mut self) -> &mut TimingMixin;

    /// Convenience method to create a message with timing
    fn create_timed_message(
        &mut self,
        source: &str,
        topic: &str,
        payload: serde_json::Value,
        event_time: SystemTime,
    ) -> Message {
        self.timing_mixin_mut()
            .create_message_with_timing(source, topic, payload, event_time)
    }

    /// Convenience method to create a message with event time extraction
    fn create_message_with_extraction(
        &mut self,
        source: &str,
        topic: &str,
        payload: serde_json::Value,
        fallback_event_time: SystemTime,
    ) -> Message {
        self.timing_mixin_mut()
            .create_message_with_event_time_extraction(source, topic, payload, fallback_event_time)
    }
}

/// Blanket implementation of TimingProcessor for anything that has TimingMixin
impl<T: WithTimingMixin> crate::core::timing::TimingProcessor for T {
    fn timing_config(&self) -> &TimingConfig {
        self.timing_mixin().timing_config()
    }

    fn watermark_manager(&mut self) -> &mut WatermarkManager {
        self.timing_mixin_mut().watermark_manager()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::time::UNIX_EPOCH;

    fn timing(config: serde_json::Value) -> TimingMixin {
        let config: crate::config::TimingConfig =
            serde_json::from_value(config).expect("timing config parses");
        TimingMixin::new(Some(&config))
    }

    #[test]
    fn stage_watermark_trails_the_wall_clock_by_the_lateness() {
        let without_timing = StageWatermark::for_stage(&TimingMixin::new(None), 250);
        assert_eq!(without_timing.current_ms(10_000), Some(9_750));

        let with_timing =
            StageWatermark::for_stage(&timing(json!({ "max_lateness_ms": 1000 })), 250);
        assert_eq!(with_timing.current_ms(10_000), Some(9_000));
        assert!(!with_timing.follows_event_time());
    }

    #[test]
    fn stage_watermark_follows_a_punctuated_strategy() {
        let mut watermark = StageWatermark::for_stage(
            &timing(json!({
                "watermark_strategy": { "type": "punctuated", "field": "wm" },
                "max_lateness_ms": 100
            })),
            250,
        );
        assert!(watermark.follows_event_time());
        assert_eq!(watermark.current_ms(10_000), None);

        let message = Message::new_with_event_time(
            "s",
            "t",
            json!({ "wm": 5_000 }),
            UNIX_EPOCH + Duration::from_millis(5_000),
        );
        watermark.observe(&message);
        assert_eq!(watermark.current_ms(10_000), Some(4_900));
    }
}
