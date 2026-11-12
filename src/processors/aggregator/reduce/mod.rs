//! The `reduce` stage: keeps a small state per key, updates it with expressions on every message,
//! and outputs it with every message.
//!
//! - `initial`: the state's values at the start, and after a reset.
//! - `update`: `field = expression`, optionally only `when` a condition holds. A message's updates
//!   all read the state as it was before the message, so `a = state.b` and `b = state.a` swap.
//! - `reset_when`: a condition that puts the state back to `initial` before the updates. The
//!   message that resets is the first of the new history.
//! - `group_by`: one state per key.
//!
//! Conditions and expressions read the message's fields as they are (`value`, `device.id`), plus:
//! - `state.<name>`: the state before this message;
//! - `prev.<field>`: the key's previous message, or this message for the first one (so a delta such
//!   as `x - prev.x` is 0, since evalexpr's `if` can't guard a missing `prev`);
//! - `msg.<field>`: this message, for a field one of these names hides;
//! - `count`: the key's messages since the start or the reset, this one included (an integer);
//! - `dt`: the seconds of event time since `prev` (0 for the first message).
//!
//! The output keeps the input message's timing, as `rule` does. Its fields are the `group_by`
//! fields and the state, laid over the message's own fields with `include_message`.

mod config;
#[cfg(test)]
mod tests;

use config::{ReduceConfig, Update};

use crate::config::{ProcessorConfig, StageConfig};
use crate::core::context::ProcessingContext;
use crate::core::input_poll::{DEFAULT_MAX_DRAIN_PER_INPUT, drain_bounded_each, idle_sleep};
use crate::core::message::Message;
use crate::core::timing_mixin::{TimingMixin, WithTimingMixin};
use crate::processors::Processor;
use crate::processors::common::expression::{ExpressionValue, Variables, to_json};
use crate::processors::common::group_key::{GroupBy, GroupKey};
use crate::processors::common::warn_once::WarnOnce;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::time::SystemTime;

/// The names conditions and expressions read besides the message's fields. A message field with
/// one of these names is hidden, and read as `msg.<name>`.
const STATE: &str = "state";
const PREV: &str = "prev";
const MSG: &str = "msg";
const COUNT: &str = "count";
const DT: &str = "dt";
const RESERVED: [&str; 5] = [STATE, PREV, MSG, COUNT, DT];

/// A key's state, and its history since the start or the last reset.
struct KeyState {
    state: Map<String, Value>,
    /// The messages so far, the current one included.
    count: i64,
    /// The previous message's payload and event time.
    prev: Option<(Value, SystemTime)>,
}

impl KeyState {
    fn new(initial: &Map<String, Value>) -> Self {
        Self {
            state: initial.clone(),
            count: 0,
            prev: None,
        }
    }

    /// Counts a message at `event_time`, and returns the seconds since the previous one: 0 for the
    /// first, and for one older than the previous (warned once).
    fn advance(&mut self, event_time: SystemTime, warnings: &mut WarnOnce) -> f64 {
        self.count += 1;
        match &self.prev {
            None => 0.0,
            Some((_, prev_time)) => match event_time.duration_since(*prev_time) {
                Ok(elapsed) => elapsed.as_secs_f64(),
                Err(_) => {
                    warnings.warn("out-of-order", || {
                        "a message is older than the one before it (same key); its dt is 0"
                            .to_string()
                    });
                    0.0
                }
            },
        }
    }

    /// Back to `initial`, with the current message as the first.
    fn restart(&mut self, initial: &Map<String, Value>) {
        *self = Self::new(initial);
        self.count = 1;
    }
}

/// What a message's conditions and expressions read.
struct View {
    /// The message's fields, then `state`, `prev`, `msg`, `count` and `dt`: for conditions.
    fields: Value,
    /// The same, flattened: for expressions.
    variables: Variables,
}

impl View {
    fn new(payload: &Value, key_state: &KeyState, dt: f64, warnings: &mut WarnOnce) -> Self {
        let mut fields = Map::new();
        let mut variables = Variables::new();
        if let Value::Object(message_fields) = payload {
            for (name, value) in message_fields {
                if RESERVED.contains(&name.as_str()) {
                    warnings.warn(&format!("hidden:{name}"), || {
                        format!("the message field '{name}' is hidden by reduce's own '{name}'; read it as msg.{name}")
                    });
                    continue;
                }
                variables.add_payload(name, value);
                fields.insert(name.clone(), value.clone());
            }
        }

        let state = Value::Object(key_state.state.clone());
        variables.add_typed(STATE, &state);
        fields.insert(STATE.to_string(), state);
        let prev = key_state.prev.as_ref().map_or(payload, |(prev, _)| prev);
        variables.add_payload(PREV, prev);
        fields.insert(PREV.to_string(), prev.clone());
        variables.add_payload(MSG, payload);
        fields.insert(MSG.to_string(), payload.clone());
        variables.set(COUNT, ExpressionValue::Int(key_state.count));
        fields.insert(COUNT.to_string(), Value::from(key_state.count));
        variables.set(DT, ExpressionValue::Float(dt));
        fields.insert(DT.to_string(), Value::from(dt));

        Self {
            fields: Value::Object(fields),
            variables,
        }
    }
}

pub struct ReduceStage {
    name: String,
    config: ReduceConfig,
    updates: Vec<Update>,
    group_by: GroupBy,
    timing: TimingMixin,
    keys: HashMap<String, KeyState>,
    warnings: WarnOnce,
}

impl ReduceStage {
    pub fn new(name: &str, config: StageConfig) -> Result<Box<dyn Processor>> {
        Ok(Box::new(Self::from_config(name, &config)?))
    }

    fn from_config(name: &str, config: &StageConfig) -> Result<Self> {
        let config = ReduceConfig::from_stage_config(config)?;
        Ok(Self {
            name: name.to_string(),
            updates: config.updates()?,
            group_by: GroupBy::new(config.group_by.clone())?,
            timing: TimingMixin::new(config.timing.as_ref()),
            config,
            keys: HashMap::new(),
            warnings: WarnOnce::new(name),
        })
    }

    /// Reduces a message into its key's state. Returns the output (with the input's topic), or
    /// `None` for a message dropped by timing or without a key.
    fn reduce(&mut self, message: Message) -> Option<Message> {
        if self.timing.should_drop_message(&message) {
            tracing::debug!(
                "Reduce '{}': message dropped by timing constraints",
                self.name
            );
            return None;
        }
        let key = self
            .group_by
            .key_or_warn(&message.payload, &mut self.warnings)?;
        if !self.keys.contains_key(&key.id) {
            self.group_by
                .check_key_count(self.keys.len(), &mut self.warnings);
        }
        let initial = &self.config.initial;
        let key_state = self
            .keys
            .entry(key.id.clone())
            .or_insert_with(|| KeyState::new(initial));

        let event_time = message.timing.event_time;
        let dt = key_state.advance(event_time, &mut self.warnings);
        let mut view = View::new(&message.payload, key_state, dt, &mut self.warnings);
        if let Some(reset_when) = &self.config.reset_when
            && reset_when.matches(&view.fields)
        {
            key_state.restart(initial);
            view = View::new(&message.payload, key_state, 0.0, &mut self.warnings);
        }

        // Every update reads the state from before this message; the results apply together.
        let mut changes = Vec::new();
        for (index, update) in self.updates.iter().enumerate() {
            if let Some(when) = &update.when
                && !when.matches(&view.fields)
            {
                continue;
            }
            match update.expression.eval(&view.variables).and_then(to_json) {
                Ok(value) => changes.push((update.field.clone(), value)),
                Err(error) => self.warnings.warn(&format!("update:{index}"), || {
                    format!(
                        "the update of '{}' failed, so it keeps its value: {}",
                        update.field, error
                    )
                }),
            }
        }
        key_state.state.extend(changes);
        key_state.prev = Some((message.payload.clone(), event_time));

        let payload = output_payload(
            &self.group_by,
            &key,
            &message.payload,
            &key_state.state,
            self.config.include_message,
            &mut self.warnings,
        );
        Some(Message {
            source: self.name.clone(),
            payload,
            ..message
        })
    }
}

/// The output: the key's fields and the state, or the state laid over the message's fields with
/// `include_message`.
fn output_payload(
    group_by: &GroupBy,
    key: &GroupKey,
    message: &Value,
    state: &Map<String, Value>,
    include_message: bool,
    warnings: &mut WarnOnce,
) -> Value {
    let mut payload = match message {
        Value::Object(_) if include_message => message.clone(),
        _ => group_by.key_payload(key, warnings),
    };
    if let Value::Object(fields) = &mut payload {
        fields.extend(
            state
                .iter()
                .map(|(name, value)| (name.clone(), value.clone())),
        );
    }
    payload
}

impl WithTimingMixin for ReduceStage {
    fn timing_mixin(&self) -> &TimingMixin {
        &self.timing
    }

    fn timing_mixin_mut(&mut self) -> &mut TimingMixin {
        &mut self.timing
    }
}

#[async_trait]
impl Processor for ReduceStage {
    async fn init(&mut self) -> Result<()> {
        tracing::info!(
            "Reduce processor '{}' initialised: {} state value(s), {} update(s)",
            self.name,
            self.config.initial.len(),
            self.updates.len()
        );
        Ok(())
    }

    async fn process(&mut self, context: &mut ProcessingContext) -> Result<()> {
        let input_messages =
            drain_bounded_each(&mut context.inputs, DEFAULT_MAX_DRAIN_PER_INPUT).await;
        let received = !input_messages.is_empty();

        for input_message in input_messages {
            let Some(output) = self.reduce(input_message.message) else {
                continue;
            };
            if let Some(output_info) = &context.output {
                let message = self.timing.update_message_watermark(Message {
                    topic: output_info.name.clone(),
                    ..output
                });
                if let Err(error) = output_info.channel.publish(message).await {
                    tracing::warn!("Failed to publish reduce result: {:?}", error);
                }
            }
        }

        if !received {
            idle_sleep().await;
        }
        Ok(())
    }
}
