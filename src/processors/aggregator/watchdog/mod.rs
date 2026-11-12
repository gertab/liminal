//! The `watchdog` stage: outputs an alert when a key (e.g. one sensor) has been silent for longer
//! than its timeout, and, with `emit_recovered`, a message when it reports again.
//!
//! - Keys are the values of `group_by` (e.g. `sensor`), or, without it, the stage's input channels.
//! - `timeout_ms` applies to every key; `timeouts` gives a key its own.
//! - `expected_keys` (and, without `group_by`, every input) are watched from the start, so a key that
//!   never reports is caught too.
//!
//! Silence is measured on the stage's watermark ([`StageWatermark`]): by default the wall clock, from
//! when each message arrived; with a punctuated or heuristic `timing.watermark_strategy`, in event
//! time, so a recording or a fast replay can be checked. An event-time watermark only moves with the
//! messages, so it catches a key that goes quiet while others report, not a stream that stops.
//!
//! A key gets one alert per silence. A message older than the key's last one (possible in event
//! time) changes nothing.

mod config;
#[cfg(test)]
mod tests;

use config::{INPUT, LAST_SEEN, SILENT_MS, STATUS, TIMEOUT_MS, WatchdogConfig};

use crate::config::{ProcessorConfig, StageConfig};
use crate::core::context::ProcessingContext;
use crate::core::input_poll::{
    DEFAULT_MAX_DRAIN_PER_INPUT, InputMessage, drain_bounded_each, idle_sleep,
};
use crate::core::timing::{epoch_millis, now_millis};
use crate::core::timing_mixin::{StageWatermark, TimingMixin, WithTimingMixin};
use crate::processors::Processor;
use crate::processors::common::group_key::{GroupBy, GroupKey};
use crate::processors::common::warn_once::WarnOnce;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::time::{Duration, UNIX_EPOCH};

/// A watched key.
struct KeyState {
    /// The key's fields in an alert: its `group_by` values, or `{"input": <channel>}`.
    fields: Value,
    timeout_ms: u64,
    /// When its silence is measured from: its last message, or the start for a key not seen yet.
    since_ms: u64,
    last_seen_ms: Option<u64>,
    silent: bool,
}

impl KeyState {
    fn new(fields: Value, timeout_ms: u64, since_ms: u64) -> Self {
        Self {
            fields,
            timeout_ms,
            since_ms,
            last_seen_ms: None,
            silent: false,
        }
    }

    fn deadline_ms(&self) -> u64 {
        self.since_ms.saturating_add(self.timeout_ms)
    }

    /// An alert for this key: its fields, then `status`, `timeout_ms`, `silent_ms` and `last_seen`
    /// (left out for a key never seen).
    fn alert(&self, status: &str, silent_ms: u64) -> Value {
        let mut payload = self.fields.clone();
        if let Value::Object(fields) = &mut payload {
            fields.insert(STATUS.to_string(), Value::from(status));
            fields.insert(TIMEOUT_MS.to_string(), Value::from(self.timeout_ms));
            fields.insert(SILENT_MS.to_string(), Value::from(silent_ms));
            if let Some(last_seen) = self.last_seen_ms {
                fields.insert(LAST_SEEN.to_string(), Value::from(last_seen));
            }
        }
        payload
    }
}

/// An alert waiting to be published, with its event time (epoch ms).
struct Pending {
    payload: Value,
    at_ms: u64,
}

pub struct WatchdogStage {
    name: String,
    config: WatchdogConfig,
    group_by: GroupBy,
    timing: TimingMixin,
    watermark: StageWatermark,
    /// By key name: the input, the `group_by` value as `timeouts` names it, or the key's id with
    /// several `group_by` fields.
    keys: HashMap<String, KeyState>,
    started: bool,
    /// No key not yet silent is due before this.
    next_due_ms: Option<u64>,
    warnings: WarnOnce,
}

impl WatchdogStage {
    pub fn new(name: &str, config: StageConfig) -> Result<Box<dyn Processor>> {
        Ok(Box::new(Self::from_config(name, &config)?))
    }

    fn from_config(name: &str, config: &StageConfig) -> Result<Self> {
        let config = WatchdogConfig::from_stage_config(config)?;
        let timing = TimingMixin::new(config.timing.as_ref());
        Ok(Self {
            name: name.to_string(),
            group_by: GroupBy::new(config.group_by.clone())?,
            // No lateness by default: live silence is measured from when messages arrive.
            watermark: StageWatermark::for_stage(&timing, 0),
            timing,
            config,
            keys: HashMap::new(),
            started: false,
            next_due_ms: None,
            warnings: WarnOnce::new(name),
        })
    }

    /// One round at `now_ms` (epoch ms): hears `messages`, alerting for the keys the watermark shows
    /// silent as it goes. Returns the alerts, in order.
    fn step(&mut self, now_ms: u64, messages: Vec<InputMessage>) -> Vec<Pending> {
        let mut pending = Vec::new();
        for InputMessage {
            input_name,
            message,
        } in messages
        {
            if self.timing.should_drop_message(&message) {
                tracing::debug!(
                    "Watchdog '{}': message dropped by timing constraints",
                    self.name
                );
                continue;
            }
            self.watermark.observe(&message);
            // Silences come first, so a gap this message ends is reported before its recovery, even
            // when a replay delivers a whole gap in one batch.
            self.advance(now_ms, &mut pending);
            let seen_ms = if self.watermark.follows_event_time() {
                epoch_millis(message.timing.event_time)
            } else {
                now_ms
            };
            self.hear(&input_name, &message.payload, seen_ms, &mut pending);
        }
        self.advance(now_ms, &mut pending);
        pending
    }

    /// Moves on to the current watermark (if there's one yet): starts watching, and alerts for the
    /// keys it shows silent.
    fn advance(&mut self, now_ms: u64, pending: &mut Vec<Pending>) {
        if let Some(watermark) = self.watermark.current_ms(now_ms) {
            if !self.started {
                self.start(watermark);
            }
            self.check(watermark, pending);
        }
    }

    /// Watches the keys expected from the start (those not heard yet), from `start_ms`.
    fn start(&mut self, start_ms: u64) {
        self.started = true;
        for name in self.config.watched_from_start().to_vec() {
            if self.keys.contains_key(&name) {
                continue;
            }
            let fields = if self.group_by.paths().is_empty() {
                json!({ INPUT: name })
            } else {
                let key = GroupKey {
                    id: name.clone(),
                    values: vec![Value::String(name.clone())],
                };
                self.group_by.key_payload(&key, &mut self.warnings)
            };
            let state = KeyState::new(fields, self.config.timeout_for(&name), start_ms);
            self.lower_next_due(state.deadline_ms());
            self.keys.insert(name, state);
        }
    }

    /// A message from `input_name` at `seen_ms`: its key isn't silent, or is back.
    fn hear(
        &mut self,
        input_name: &str,
        payload: &Value,
        seen_ms: u64,
        pending: &mut Vec<Pending>,
    ) {
        let (name, key) = if self.group_by.paths().is_empty() {
            (input_name.to_string(), None)
        } else {
            let Some(key) = self.group_by.key_or_warn(payload, &mut self.warnings) else {
                return;
            };
            let name = match key.values.as_slice() {
                [value] => key_name(value),
                _ => key.id.clone(),
            };
            (name, Some(key))
        };

        // The key's fields, from this message, the first time it's seen (an expected key's are
        // built from its name, as text).
        let first_sighting = self
            .keys
            .get(&name)
            .is_none_or(|state| state.last_seen_ms.is_none());
        let fields = first_sighting.then(|| match &key {
            Some(key) => self.group_by.key_payload(key, &mut self.warnings),
            None => json!({ INPUT: input_name }),
        });
        if !self.keys.contains_key(&name) {
            self.group_by
                .check_key_count(self.keys.len(), &mut self.warnings);
        }
        let config = &self.config;
        let state = self.keys.entry(name).or_insert_with_key(|name| {
            // Its fields are set below.
            KeyState::new(Value::Null, config.timeout_for(name), seen_ms)
        });
        if seen_ms < state.since_ms {
            return;
        }
        if let Some(fields) = fields {
            state.fields = fields;
        }
        let gap_ms = seen_ms - state.since_ms;
        state.since_ms = seen_ms;
        state.last_seen_ms = Some(seen_ms);
        if state.silent {
            state.silent = false;
            if self.config.emit_recovered {
                pending.push(Pending {
                    payload: state.alert("recovered", gap_ms),
                    at_ms: seen_ms,
                });
            }
        }
        let deadline = state.deadline_ms();
        self.lower_next_due(deadline);
    }

    /// Alerts, in deadline order, for the keys whose silence reached their timeout at `watermark`.
    fn check(&mut self, watermark: u64, pending: &mut Vec<Pending>) {
        if self.next_due_ms.is_none_or(|due| watermark < due) {
            return;
        }
        let mut alerts = Vec::new();
        let mut next_due: Option<u64> = None;
        for (name, state) in &mut self.keys {
            if state.silent {
                continue;
            }
            let deadline = state.deadline_ms();
            if watermark >= deadline {
                state.silent = true;
                let payload = state.alert("silent", watermark - state.since_ms);
                alerts.push((deadline, name.clone(), payload));
            } else {
                next_due = Some(next_due.map_or(deadline, |due| due.min(deadline)));
            }
        }
        self.next_due_ms = next_due;

        alerts.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
        pending.extend(
            alerts
                .into_iter()
                .map(|(at_ms, _, payload)| Pending { payload, at_ms }),
        );
    }

    fn lower_next_due(&mut self, deadline: u64) {
        self.next_due_ms = Some(self.next_due_ms.map_or(deadline, |due| due.min(deadline)));
    }
}

/// A key value as `timeouts` and `expected_keys` name it: text as it is, anything else as written
/// (`3`, `true`).
fn key_name(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

impl WithTimingMixin for WatchdogStage {
    fn timing_mixin(&self) -> &TimingMixin {
        &self.timing
    }

    fn timing_mixin_mut(&mut self) -> &mut TimingMixin {
        &mut self.timing
    }
}

#[async_trait]
impl Processor for WatchdogStage {
    async fn init(&mut self) -> Result<()> {
        tracing::info!(
            "Watchdog processor '{}' initialised: timeout {} ms, {} key(s) watched from the start",
            self.name,
            self.config.timeout_ms,
            self.config.watched_from_start().len()
        );
        Ok(())
    }

    async fn process(&mut self, context: &mut ProcessingContext) -> Result<()> {
        let input_messages =
            drain_bounded_each(&mut context.inputs, DEFAULT_MAX_DRAIN_PER_INPUT).await;
        let received = !input_messages.is_empty();

        let alerts = self.step(now_millis(), input_messages);
        if let Some(output_info) = &context.output {
            for Pending { payload, at_ms } in alerts {
                let message = self.timing.create_message_with_timing(
                    &self.name,
                    &output_info.name,
                    payload,
                    UNIX_EPOCH + Duration::from_millis(at_ms),
                );
                if let Err(error) = output_info.channel.publish(message).await {
                    tracing::warn!("Failed to publish watchdog alert: {:?}", error);
                }
            }
        }

        if !received {
            idle_sleep().await;
        }
        Ok(())
    }
}
