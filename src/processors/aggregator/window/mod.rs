//! The `window` stage: collects the messages of a time span, or the last N messages, per key, and
//! outputs aggregates over them.
//!
//! - `tumbling`: back-to-back windows of `size_ms`, aligned to the epoch (e.g. :00, :10).
//! - `sliding`: the last `size_ms`, output every `slide_ms` (or with every message).
//! - `last_n`: the last `size_n` messages.
//!
//! Time windows use Liminal's event time, `message.timing.event_time`: when the input received the
//! message, or the payload field named by the input's `timing.event_time_field`. A window is output
//! once the stage's watermark ([`StageWatermark`]) passes its end: by default the wall clock minus a
//! lateness (the stage's `timing.max_lateness_ms`, or [`DEFAULT_LATENESS_MS`]); with a punctuated or
//! heuristic `timing.watermark_strategy`, the watermark that strategy derives from the messages.
//!
//! A message too late for every window still to be output is dropped, as is a message that
//! `TimingMixin::should_drop_message` rejects (as in `rule`).

mod aggregate;
mod config;
#[cfg(test)]
mod tests;

use aggregate::{Aggregator, Item};
use config::{WindowConfig, Windows};

use crate::config::{ProcessorConfig, StageConfig};
use crate::core::context::ProcessingContext;
use crate::core::input_poll::{DEFAULT_MAX_DRAIN_PER_INPUT, drain_bounded_each, idle_sleep};
use crate::core::message::Message;
use crate::core::timing::{epoch_millis, now_millis};
use crate::core::timing_mixin::{StageWatermark, TimingMixin, WithTimingMixin};
use crate::processors::Processor;
use crate::processors::common::group_key::{GroupBy, GroupKey};
use crate::processors::common::warn_once::WarnOnce;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, UNIX_EPOCH};

/// How long a window waits past its end for messages still on their way, when the stage has no
/// `[timing]` block (with one, its `max_lateness_ms` is used; Liminal's default of 30 s is too long
/// a wait for windows).
pub const DEFAULT_LATENESS_MS: u64 = 250;

/// With `emit_empty`, a watermark jump over more windows than this (e.g. after the computer slept)
/// skips the empty ones instead of outputting each.
const MAX_EMPTY_WINDOWS_PER_JUMP: u64 = 100;

/// A key's messages: in event-time order for time windows, in arrival order for `last_n`.
struct KeyState {
    key: GroupKey,
    items: VecDeque<Item>,
}

impl KeyState {
    fn new(key: GroupKey) -> Self {
        Self {
            key,
            items: VecDeque::new(),
        }
    }
}

/// A window result waiting to be published.
struct Pending {
    payload: Value,
    end_ms: u64,
}

pub struct WindowStage {
    name: String,
    config: WindowConfig,
    windows: Windows,
    group_by: GroupBy,
    aggregator: Aggregator,
    timing: TimingMixin,
    watermark: StageWatermark,
    keys: HashMap<String, KeyState>,
    /// `Windows::Closing`: the end of the next window to output.
    next_end_ms: Option<u64>,
    warnings: WarnOnce,
}

impl WindowStage {
    pub fn new(name: &str, config: StageConfig) -> Result<Box<dyn Processor>> {
        Ok(Box::new(Self::from_config(name, &config)?))
    }

    fn from_config(name: &str, config: &StageConfig) -> Result<Self> {
        let config = WindowConfig::from_stage_config(config)?;
        let windows = config.windows()?;
        let group_by = GroupBy::new(config.group_by.clone())?;
        let aggregator = Aggregator::new(&config.aggregates);
        let timing = TimingMixin::new(config.timing.as_ref());
        let watermark = StageWatermark::for_stage(&timing, DEFAULT_LATENESS_MS);

        let mut keys = HashMap::new();
        // Without group_by, the one key exists from the start, so empty windows are output before
        // the first message too.
        if config.emit_empty
            && let Some(key) = group_by.key_of(&Value::Null)
        {
            keys.insert(key.id.clone(), KeyState::new(key));
        }

        Ok(Self {
            name: name.to_string(),
            config,
            windows,
            group_by,
            aggregator,
            timing,
            watermark,
            keys,
            next_end_ms: None,
            warnings: WarnOnce::new(name),
        })
    }

    /// One round at `now_ms` (epoch ms): adds `messages`, then outputs the windows the watermark has
    /// passed. Returns the results, in order.
    fn step(&mut self, now_ms: u64, messages: Vec<Message>) -> Vec<Pending> {
        let mut pending = Vec::new();
        for message in messages {
            if self.timing.should_drop_message(&message) {
                tracing::debug!(
                    "Window '{}': message dropped by timing constraints",
                    self.name
                );
                continue;
            }
            self.watermark.observe(&message);
            let watermark = self.watermark.current_ms(now_ms);
            self.add(&message, watermark, &mut pending);
        }

        let watermark = self.watermark.current_ms(now_ms);
        if let Windows::Closing { step_ms, .. } = self.windows
            && self.next_end_ms.is_none()
        {
            self.start_windows(now_ms, watermark, step_ms);
        }
        if let Some(watermark) = watermark {
            self.advance(watermark, &mut pending);
        }
        pending
    }

    /// Sets the end of the first closing window: the first boundary after the earliest time known,
    /// which is now (wall-clock watermark), the first event-time watermark, or an earlier message.
    fn start_windows(&mut self, now_ms: u64, watermark: Option<u64>, step_ms: u64) {
        let start = if self.watermark.follows_event_time() {
            // Wait for the first watermark; messages seen before it are kept.
            match watermark {
                Some(watermark) => watermark,
                None => return,
            }
        } else {
            now_ms
        };
        let origin = self
            .keys
            .values()
            .filter_map(|state| state.items.front())
            .map(|item| item.at_ms)
            .fold(start, u64::min);
        self.next_end_ms = Some((origin / step_ms + 1) * step_ms);
    }

    /// The earliest event time a message can have and still count in a window that is yet to be
    /// output. Anything earlier is late.
    fn floor_ms(&self, watermark: Option<u64>) -> Option<u64> {
        match self.windows {
            // The start of the next window to output.
            Windows::Closing { size_ms, .. } => {
                self.next_end_ms.map(|end| end.saturating_sub(size_ms))
            }
            // Windows before the watermark's are over.
            Windows::TumblingRunning { size_ms } => watermark.map(|w| w / size_ms * size_ms),
            Windows::SlidingRunning { size_ms } => watermark.map(|w| w.saturating_sub(size_ms)),
            Windows::LastN { .. } => None,
        }
    }

    /// Adds a message to its key's window, and outputs a result for the running windows (and for a
    /// full `last_n` batch).
    fn add(&mut self, message: &Message, watermark: Option<u64>, pending: &mut Vec<Pending>) {
        let at_ms = epoch_millis(message.timing.event_time);
        if self.floor_ms(watermark).is_some_and(|floor| at_ms < floor) {
            self.warnings.warn("late", || {
                "messages too late for any window still open are dropped (a larger timing.max_lateness_ms waits longer)"
                    .to_string()
            });
            return;
        }
        let Some(key) = self
            .group_by
            .key_or_warn(&message.payload, &mut self.warnings)
        else {
            return;
        };
        if !self.keys.contains_key(&key.id) {
            self.group_by
                .check_key_count(self.keys.len(), &mut self.warnings);
        }

        let item = self.aggregator.item(at_ms, &message.payload);
        let max_items = self.config.max_items;
        let state = self
            .keys
            .entry(key.id.clone())
            .or_insert_with(|| KeyState::new(key));

        // Keep the message, and find the bounds of the window to output now (if any).
        let (start_ms, end_ms) = match self.windows {
            Windows::Closing { .. } => {
                insert_by_time(&mut state.items, item, max_items, &mut self.warnings);
                return;
            }
            Windows::TumblingRunning { size_ms } => {
                insert_by_time(&mut state.items, item, max_items, &mut self.warnings);
                let start = at_ms / size_ms * size_ms;
                (start, start + size_ms)
            }
            Windows::SlidingRunning { size_ms } => {
                insert_by_time(&mut state.items, item, max_items, &mut self.warnings);
                (at_ms.saturating_sub(size_ms), at_ms)
            }
            Windows::LastN { size_n, batches } => {
                state.items.push_back(item);
                while state.items.len() > size_n {
                    state.items.pop_front();
                }
                if batches && state.items.len() < size_n {
                    return;
                }
                time_span(&state.items).unwrap_or((at_ms, at_ms))
            }
        };

        let window: Vec<&Item> = match self.windows {
            // The last size_ms up to and including this message: (start, at].
            Windows::SlidingRunning { .. } => items_between(&state.items, start_ms + 1, end_ms + 1),
            Windows::LastN { .. } => state.items.iter().collect(),
            _ => items_between(&state.items, start_ms, end_ms),
        };
        let payload = self.aggregator.result(
            &self.group_by,
            &state.key,
            &window,
            start_ms,
            end_ms,
            &mut self.warnings,
        );
        pending.push(Pending { payload, end_ms });

        if let Windows::LastN { batches: true, .. } = self.windows {
            state.items.clear();
        }
    }

    /// Moves the windows on to `watermark`: outputs the closing windows it has passed, or drops what
    /// the running windows can no longer use.
    fn advance(&mut self, watermark: u64, pending: &mut Vec<Pending>) {
        match self.windows {
            Windows::Closing { size_ms, step_ms } => {
                self.close_windows(watermark, size_ms, step_ms, pending)
            }
            Windows::TumblingRunning { .. } | Windows::SlidingRunning { .. } => {
                if let Some(floor) = self.floor_ms(Some(watermark)) {
                    for state in self.keys.values_mut() {
                        evict_before(&mut state.items, floor);
                    }
                    self.keys.retain(|_, state| !state.items.is_empty());
                }
            }
            Windows::LastN { .. } => {}
        }
    }

    /// Outputs each window `[end - size_ms, end)` whose end the watermark has passed.
    fn close_windows(
        &mut self,
        watermark: u64,
        size_ms: u64,
        step_ms: u64,
        pending: &mut Vec<Pending>,
    ) {
        let Some(mut end) = self.next_end_ms else {
            return;
        };
        let emit_empty = self.config.emit_empty;
        let due = watermark.saturating_sub(end) / step_ms + 1;
        let fill_empty = emit_empty && due <= MAX_EMPTY_WINDOWS_PER_JUMP;
        if emit_empty && !fill_empty {
            self.warnings.warn("watermark-jump", || {
                format!(
                    "the watermark jumped over {due} windows (did the computer sleep?); empty windows in the gap aren't output"
                )
            });
        }

        while watermark >= end {
            let start = end.saturating_sub(size_ms);
            for state in self.keys.values_mut() {
                let window = items_between(&state.items, start, end);
                if !window.is_empty() || fill_empty {
                    let payload = self.aggregator.result(
                        &self.group_by,
                        &state.key,
                        &window,
                        start,
                        end,
                        &mut self.warnings,
                    );
                    pending.push(Pending {
                        payload,
                        end_ms: end,
                    });
                }
                // The next window starts at (end + step) - size.
                evict_before(&mut state.items, (end + step_ms).saturating_sub(size_ms));
            }
            end += step_ms;
            if !emit_empty {
                self.keys.retain(|_, state| !state.items.is_empty());
            }
            // Nothing left to output: skip past the empty windows.
            if !fill_empty && self.keys.values().all(|state| state.items.is_empty()) {
                end = end.max((watermark / step_ms + 1) * step_ms);
            }
        }
        self.next_end_ms = Some(end);
    }
}

impl WithTimingMixin for WindowStage {
    fn timing_mixin(&self) -> &TimingMixin {
        &self.timing
    }

    fn timing_mixin_mut(&mut self) -> &mut TimingMixin {
        &mut self.timing
    }
}

#[async_trait]
impl Processor for WindowStage {
    async fn init(&mut self) -> Result<()> {
        tracing::info!(
            "Window processor '{}' initialised: {} window, {} aggregate(s)",
            self.name,
            self.config.kind.as_str(),
            self.config.aggregates.len()
        );
        Ok(())
    }

    async fn process(&mut self, context: &mut ProcessingContext) -> Result<()> {
        let input_messages =
            drain_bounded_each(&mut context.inputs, DEFAULT_MAX_DRAIN_PER_INPUT).await;
        let received = !input_messages.is_empty();
        let messages = input_messages
            .into_iter()
            .map(|input_message| input_message.message)
            .collect();

        let results = self.step(now_millis(), messages);
        if let Some(output_info) = &context.output {
            for Pending { payload, end_ms } in results {
                let message = self.timing.create_message_with_timing(
                    &self.name,
                    &output_info.name,
                    payload,
                    UNIX_EPOCH + Duration::from_millis(end_ms),
                );
                if let Err(error) = output_info.channel.publish(message).await {
                    tracing::warn!("Failed to publish window result: {:?}", error);
                }
            }
        }

        if !received {
            idle_sleep().await;
        }
        Ok(())
    }
}

/// Inserts an item in event-time order (normally at the back), keeping at most `max_items`.
fn insert_by_time(
    items: &mut VecDeque<Item>,
    item: Item,
    max_items: usize,
    warnings: &mut WarnOnce,
) {
    if items.len() >= max_items {
        items.pop_front();
        warnings.warn("max-items", || {
            format!("more than max_items ({max_items}) messages for one key; dropping the oldest")
        });
    }
    let index = items.partition_point(|existing| existing.at_ms <= item.at_ms);
    items.insert(index, item);
}

/// The items (in event-time order) with `start_ms <= at_ms < end_ms`.
fn items_between(items: &VecDeque<Item>, start_ms: u64, end_ms: u64) -> Vec<&Item> {
    items
        .iter()
        .skip_while(|item| item.at_ms < start_ms)
        .take_while(|item| item.at_ms < end_ms)
        .collect()
}

/// Drops the items (in event-time order) before `cutoff_ms`.
fn evict_before(items: &mut VecDeque<Item>, cutoff_ms: u64) {
    while items.front().is_some_and(|item| item.at_ms < cutoff_ms) {
        items.pop_front();
    }
}

/// The earliest and latest event time among the items (in any order).
fn time_span(items: &VecDeque<Item>) -> Option<(u64, u64)> {
    let start = items.iter().map(|item| item.at_ms).min()?;
    let end = items.iter().map(|item| item.at_ms).max()?;
    Some((start, end))
}
