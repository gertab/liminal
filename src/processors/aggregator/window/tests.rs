//! The window stage's behaviour, driven through `step` with explicit times, plus one test of
//! `process` for publishing.

use super::*;
use crate::config::types::ChannelType;
use crate::core::channel::{Channel, PubSubChannel, Subscriber};
use serde_json::json;
use std::sync::Arc;
use tokio::sync::mpsc;

/// Epoch ms the tests start at: a multiple of every window size used.
const BASE: u64 = 1_000_000;

fn stage_config(parameters: Value, timing: Option<Value>) -> StageConfig {
    let Value::Object(parameters) = parameters else {
        panic!("parameters must be an object");
    };
    StageConfig {
        r#type: "window".to_string(),
        inputs: Some(vec!["in".to_string()]),
        output: Some("stats".to_string()),
        concurrency: None,
        channel: None,
        timing: timing.map(|timing| serde_json::from_value(timing).expect("timing parses")),
        parameters: Some(parameters.into_iter().collect()),
    }
}

fn window(parameters: Value) -> WindowStage {
    WindowStage::from_config("window", &stage_config(parameters, None)).expect("valid config")
}

fn window_with_timing(parameters: Value, timing: Value) -> WindowStage {
    WindowStage::from_config("window", &stage_config(parameters, Some(timing)))
        .expect("valid config")
}

/// A message whose event time is `ms` after `BASE`.
fn at(ms: u64, payload: Value) -> Message {
    Message::new_with_event_time(
        "source",
        "in",
        payload,
        UNIX_EPOCH + Duration::from_millis(BASE + ms),
    )
}

fn value_at(ms: u64, value: f64) -> Message {
    at(ms, json!({ "value": value }))
}

/// A round at `ms` after `BASE`; returns the payloads output.
fn step(stage: &mut WindowStage, ms: u64, messages: Vec<Message>) -> Vec<Value> {
    stage
        .step(BASE + ms, messages)
        .into_iter()
        .map(|pending| pending.payload)
        .collect()
}

fn all_aggregates() -> Value {
    json!([
        { "function": "count" },
        { "function": "sum", "field": "value" },
        { "function": "avg", "field": "value" },
        { "function": "min", "field": "value" },
        { "function": "max", "field": "value" },
        { "function": "spread", "field": "value" },
        { "function": "std", "field": "value" },
    ])
}

fn count_and_avg() -> Value {
    json!([{ "function": "count" }, { "function": "avg", "field": "value" }])
}

#[test]
fn tumbling_outputs_each_window_once_the_watermark_passes_its_end() {
    let mut stage =
        window(json!({ "kind": "tumbling", "size_ms": 1000, "aggregates": all_aggregates() }));

    assert!(step(&mut stage, 100, vec![value_at(100, 1.0)]).is_empty());
    assert!(step(&mut stage, 400, vec![value_at(400, 2.0)]).is_empty());
    assert!(step(&mut stage, 900, vec![value_at(900, 3.0)]).is_empty());
    // The watermark is the wall clock minus DEFAULT_LATENESS_MS (250 ms).
    assert!(step(&mut stage, 1249, vec![]).is_empty());

    let outputs = step(&mut stage, 1250, vec![]);
    assert_eq!(outputs.len(), 1, "{outputs:?}");
    let window = &outputs[0];
    assert_eq!(window["window_start"], json!(BASE));
    assert_eq!(window["window_end"], json!(BASE + 1000));
    assert_eq!(window["count"], json!(3));
    assert_eq!(window["value_sum"], json!(6.0));
    assert_eq!(window["value_avg"], json!(2.0));
    assert_eq!(window["value_min"], json!(1.0));
    assert_eq!(window["value_max"], json!(3.0));
    assert_eq!(window["value_spread"], json!(2.0));
    let std = window["value_std"].as_f64().unwrap();
    assert!((std - (2.0f64 / 3.0).sqrt()).abs() < 1e-9, "{std}");

    // An empty window outputs nothing; the next message starts a new window.
    assert!(step(&mut stage, 2250, vec![]).is_empty());
    assert!(step(&mut stage, 2500, vec![value_at(2500, 10.0)]).is_empty());
    let outputs = step(&mut stage, 3250, vec![]);
    assert_eq!(outputs[0]["count"], json!(1));
    assert_eq!(outputs[0]["window_start"], json!(BASE + 2000));
}

#[test]
fn counts_out_of_order_messages_and_drops_late_ones() {
    let mut stage =
        window(json!({ "kind": "tumbling", "size_ms": 1000, "aggregates": all_aggregates() }));

    // Out of order but within the lateness: both count.
    step(
        &mut stage,
        950,
        vec![value_at(900, 2.0), value_at(800, 1.0)],
    );
    let outputs = step(&mut stage, 1250, vec![]);
    assert_eq!(outputs[0]["count"], json!(2));
    assert_eq!(outputs[0]["value_min"], json!(1.0));

    // Its window has been output, so a message from 950 is late and dropped.
    step(
        &mut stage,
        1300,
        vec![value_at(950, 99.0), value_at(1100, 5.0)],
    );
    let outputs = step(&mut stage, 2250, vec![]);
    assert_eq!(outputs[0]["count"], json!(1));
    assert_eq!(outputs[0]["value_max"], json!(5.0));
    assert!(stage.warnings.has_warned("late"));
}

#[test]
fn waits_for_the_stage_max_lateness() {
    let mut stage = window_with_timing(
        json!({ "kind": "tumbling", "size_ms": 1000, "aggregates": count_and_avg() }),
        json!({ "max_lateness_ms": 1000 }),
    );
    step(&mut stage, 100, vec![value_at(100, 1.0)]);
    assert!(step(&mut stage, 1999, vec![]).is_empty());
    assert_eq!(step(&mut stage, 2000, vec![])[0]["count"], json!(1));
}

#[test]
fn an_event_time_watermark_closes_windows_by_the_messages_own_times() {
    // A punctuated watermark (the `wm` field) moves time, whatever the wall clock says, as when
    // replaying a recording.
    let mut stage = window_with_timing(
        json!({ "kind": "tumbling", "size_ms": 1000, "aggregates": count_and_avg() }),
        json!({ "watermark_strategy": { "type": "punctuated", "field": "wm" }, "max_lateness_ms": 0 }),
    );
    let replayed = |ms: u64, value: f64| at(ms, json!({ "value": value, "wm": BASE + ms }));
    let much_later = 50_000_000;

    assert!(
        step(
            &mut stage,
            much_later,
            vec![replayed(100, 1.0), replayed(400, 2.0)]
        )
        .is_empty()
    );
    let outputs = step(&mut stage, much_later, vec![replayed(1100, 3.0)]);
    assert_eq!(outputs.len(), 1, "{outputs:?}");
    assert_eq!(outputs[0]["window_end"], json!(BASE + 1000));
    assert_eq!(outputs[0]["count"], json!(2));

    // Behind the watermark: late.
    step(&mut stage, much_later, vec![replayed(500, 9.0)]);
    let outputs = step(&mut stage, much_later, vec![replayed(2000, 4.0)]);
    assert_eq!(outputs[0]["count"], json!(1));
    assert_eq!(outputs[0]["value_avg"], json!(3.0));
}

#[test]
fn drops_messages_rejected_by_their_timing() {
    let mut stage =
        window(json!({ "kind": "tumbling", "size_ms": 1000, "aggregates": count_and_avg() }));
    let mut behind_its_watermark = value_at(100, 1.0);
    behind_its_watermark.timing.watermark =
        Some(behind_its_watermark.timing.event_time + Duration::from_millis(1));

    step(
        &mut stage,
        100,
        vec![behind_its_watermark, value_at(200, 2.0)],
    );
    let outputs = step(&mut stage, 1250, vec![]);
    assert_eq!(outputs[0]["count"], json!(1));
    assert_eq!(outputs[0]["value_avg"], json!(2.0));
}

#[test]
fn emit_empty_outputs_every_window_even_without_messages() {
    let mut stage = window(json!({
        "kind": "tumbling", "size_ms": 1000, "emit_empty": true, "aggregates": count_and_avg(),
    }));

    assert!(step(&mut stage, 0, vec![]).is_empty());
    let outputs = step(&mut stage, 1250, vec![]);
    assert_eq!(outputs.len(), 1, "{outputs:?}");
    assert_eq!(outputs[0]["count"], json!(0));
    assert_eq!(outputs[0]["value_avg"], Value::Null);

    // Two windows passed between rounds: both are output.
    let outputs = step(&mut stage, 3250, vec![]);
    let ends: Vec<_> = outputs
        .iter()
        .map(|window| window["window_end"].clone())
        .collect();
    assert_eq!(ends, vec![json!(BASE + 2000), json!(BASE + 3000)]);
}

#[test]
fn a_big_watermark_jump_skips_the_empty_windows() {
    let mut stage = window(json!({
        "kind": "tumbling", "size_ms": 1000, "emit_empty": true, "aggregates": count_and_avg(),
    }));
    step(&mut stage, 0, vec![]);

    // As if the computer slept for 10 minutes: 600 windows are due.
    assert!(step(&mut stage, 600_250, vec![]).is_empty());
    assert!(stage.warnings.has_warned("watermark-jump"));

    // Then windows are output one by one again.
    let outputs = step(&mut stage, 601_250, vec![]);
    assert_eq!(outputs.len(), 1, "{outputs:?}");
    assert_eq!(outputs[0]["window_end"], json!(BASE + 601_000));
}

#[test]
fn tumbling_every_message_restarts_at_each_window() {
    let mut stage = window(json!({
        "kind": "tumbling", "size_ms": 1000, "emit": "every_message", "aggregates": count_and_avg(),
    }));

    let first = step(&mut stage, 100, vec![value_at(100, 1.0)]);
    assert_eq!(
        (first[0]["count"].clone(), first[0]["value_avg"].clone()),
        (json!(1), json!(1.0))
    );
    let second = step(&mut stage, 500, vec![value_at(500, 3.0)]);
    assert_eq!(
        (second[0]["count"].clone(), second[0]["value_avg"].clone()),
        (json!(2), json!(2.0))
    );
    assert_eq!(second[0]["window_end"], json!(BASE + 1000));

    let next = step(&mut stage, 1200, vec![value_at(1200, 10.0)]);
    assert_eq!(
        (next[0]["count"].clone(), next[0]["value_avg"].clone()),
        (json!(1), json!(10.0))
    );
    assert_eq!(next[0]["window_start"], json!(BASE + 1000));
}

#[test]
fn sliding_every_message_covers_the_last_size_ms() {
    let mut stage = window(json!({
        "kind": "sliding", "size_ms": 1000, "emit": "every_message", "aggregates": count_and_avg(),
    }));

    step(&mut stage, 100, vec![value_at(100, 1.0)]);
    assert_eq!(
        step(&mut stage, 600, vec![value_at(600, 2.0)])[0]["count"],
        json!(2)
    );

    // At 1200 the message from 100 has slid out.
    let outputs = step(&mut stage, 1200, vec![value_at(1200, 3.0)]);
    assert_eq!(outputs[0]["count"], json!(2));
    assert_eq!(outputs[0]["value_avg"], json!(2.5));
    assert_eq!(outputs[0]["window_start"], json!(BASE + 200));
    assert_eq!(outputs[0]["window_end"], json!(BASE + 1200));
}

#[test]
fn sliding_on_close_outputs_every_slide() {
    let mut stage = window(json!({
        "kind": "sliding", "size_ms": 1000, "slide_ms": 500,
        "aggregates": [{ "function": "count" }, { "function": "max", "field": "value" }],
    }));

    step(&mut stage, 100, vec![value_at(100, 1.0)]);
    let first = step(&mut stage, 750, vec![]);
    assert_eq!(first[0]["count"], json!(1));
    assert_eq!(first[0]["window_start"], json!(BASE - 500));
    assert_eq!(first[0]["window_end"], json!(BASE + 500));

    step(&mut stage, 800, vec![value_at(600, 2.0)]);
    let second = step(&mut stage, 1250, vec![]);
    assert_eq!(
        (second[0]["count"].clone(), second[0]["value_max"].clone()),
        (json!(2), json!(2.0))
    );

    let third = step(&mut stage, 1750, vec![]);
    assert_eq!(
        (third[0]["count"].clone(), third[0]["value_max"].clone()),
        (json!(1), json!(2.0))
    );

    assert!(step(&mut stage, 2250, vec![]).is_empty());
}

#[test]
fn last_n_every_message_uses_the_last_n() {
    let mut stage = window(json!({
        "kind": "last_n", "size_n": 3, "emit": "every_message", "aggregates": count_and_avg(),
    }));

    let mut counts = Vec::new();
    let mut last = Value::Null;
    for (index, value) in [1.0, 2.0, 3.0, 4.0].into_iter().enumerate() {
        let ms = 100 * (index as u64 + 1);
        let outputs = step(&mut stage, ms, vec![value_at(ms, value)]);
        counts.push(outputs[0]["count"].clone());
        last = outputs[0].clone();
    }
    assert_eq!(counts, vec![json!(1), json!(2), json!(3), json!(3)]);
    assert_eq!(last["value_avg"], json!(3.0));
    assert_eq!(last["window_start"], json!(BASE + 200));
    assert_eq!(last["window_end"], json!(BASE + 400));
}

#[test]
fn last_n_on_close_outputs_each_batch() {
    let mut stage = window(json!({
        "kind": "last_n", "size_n": 2, "aggregates": [{ "function": "avg", "field": "value" }],
    }));

    assert!(step(&mut stage, 100, vec![value_at(100, 1.0)]).is_empty());
    assert_eq!(
        step(&mut stage, 200, vec![value_at(200, 2.0)])[0]["value_avg"],
        json!(1.5)
    );
    assert!(step(&mut stage, 300, vec![value_at(300, 3.0)]).is_empty());
    assert_eq!(
        step(&mut stage, 400, vec![value_at(400, 4.0)])[0]["value_avg"],
        json!(3.5)
    );
}

#[test]
fn group_by_keeps_a_window_per_key() {
    let mut stage = window(json!({
        "kind": "tumbling", "size_ms": 1000, "group_by": ["sensor", "site.id"], "aggregates": count_and_avg(),
    }));

    step(
        &mut stage,
        100,
        vec![
            at(
                100,
                json!({ "sensor": "a", "site": { "id": 1 }, "value": 1.0 }),
            ),
            at(
                100,
                json!({ "sensor": "b", "site": { "id": 1 }, "value": 10.0 }),
            ),
            at(
                100,
                json!({ "sensor": "a", "site": { "id": 1 }, "value": 3.0 }),
            ),
            at(100, json!({ "value": 99.0 })),
        ],
    );
    assert!(stage.warnings.has_warned("missing-key"));

    let mut outputs = step(&mut stage, 1250, vec![]);
    outputs.sort_by_key(|window| window["sensor"].as_str().unwrap_or_default().to_string());
    assert_eq!(outputs.len(), 2, "{outputs:?}");
    assert_eq!(outputs[0]["sensor"], json!("a"));
    assert_eq!(outputs[0]["site"], json!({ "id": 1 }));
    assert_eq!(
        (outputs[0]["count"].clone(), outputs[0]["value_avg"].clone()),
        (json!(2), json!(2.0))
    );
    assert_eq!(outputs[1]["sensor"], json!("b"));
    assert_eq!(outputs[1]["value_avg"], json!(10.0));
}

#[test]
fn skips_missing_and_non_numeric_values() {
    let mut stage = window(json!({
        "kind": "tumbling", "size_ms": 1000,
        "aggregates": [
            { "function": "count" },
            { "function": "distinct_count", "field": "state", "as": "states" },
            { "function": "avg", "field": "value" },
        ],
    }));

    step(
        &mut stage,
        100,
        vec![
            at(100, json!({ "value": 1.0, "state": "on" })),
            at(100, json!({ "value": "x", "state": "on" })),
            at(100, json!({ "state": "off" })),
            at(100, json!({ "value": 3.0 })),
        ],
    );

    let window = &step(&mut stage, 1250, vec![])[0];
    assert_eq!(window["count"], json!(4));
    assert_eq!(window["states"], json!(2));
    assert_eq!(window["value_avg"], json!(2.0));
    assert!(stage.warnings.has_warned("non-numeric:value"));
}

#[test]
fn keeps_at_most_max_items_per_key() {
    let mut stage = window(json!({
        "kind": "tumbling", "size_ms": 1000, "max_items": 2,
        "aggregates": [{ "function": "count" }, { "function": "min", "field": "value" }],
    }));

    step(
        &mut stage,
        100,
        vec![value_at(100, 1.0), value_at(110, 2.0), value_at(120, 3.0)],
    );
    let window = &step(&mut stage, 1250, vec![])[0];
    assert_eq!(
        (window["count"].clone(), window["value_min"].clone()),
        (json!(2), json!(2.0))
    );
}

#[tokio::test]
async fn publishes_results_as_timed_messages() {
    let mut stage = window(json!({
        "kind": "last_n", "size_n": 1, "aggregates": [{ "function": "count" }],
    }));
    let mut context = ProcessingContext::new("window".to_string());
    let (input, receiver) = mpsc::channel(4);
    context.add_input("in".to_string(), Subscriber::Mpsc(receiver));
    let channel = Arc::new(Channel::new(ChannelType::Broadcast, 4));
    let mut output = channel.subscribe();
    context.attach_output("stats".to_string(), channel);

    let message = value_at(100, 1.0);
    let event_time = message.timing.event_time;
    input.send(message).await.unwrap();
    stage.process(&mut context).await.unwrap();

    let result = output.try_recv().await.expect("a window result");
    assert_eq!(result.source, "window");
    assert_eq!(result.topic, "stats");
    assert_eq!(result.payload["count"], json!(1));
    assert_eq!(result.timing.event_time, event_time);
    assert_eq!(result.timing.sequence_id, Some(1));
}
