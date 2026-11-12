//! The reduce stage's behaviour, driven through `reduce` with explicit event times, plus one test of
//! `process` for publishing.

use super::*;
use crate::config::types::ChannelType;
use crate::core::channel::{Channel, PubSubChannel, Subscriber};
use serde_json::json;
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};
use tokio::sync::mpsc;

/// Epoch ms the tests' event times start at.
const BASE: u64 = 1_000_000;

fn stage(parameters: Value) -> ReduceStage {
    let Value::Object(parameters) = parameters else {
        panic!("parameters must be an object");
    };
    let config = StageConfig {
        r#type: "reduce".to_string(),
        inputs: Some(vec!["in".to_string()]),
        output: Some("state".to_string()),
        concurrency: None,
        channel: None,
        timing: None,
        parameters: Some(parameters.into_iter().collect()),
    };
    ReduceStage::from_config("reduce", &config).expect("valid config")
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

/// Reduces each message; returns the output payloads.
fn run(stage: &mut ReduceStage, messages: Vec<Message>) -> Vec<Value> {
    messages
        .into_iter()
        .filter_map(|message| stage.reduce(message))
        .map(|output| output.payload)
        .collect()
}

fn position(ms: u64, entity: &str, x: f64, y: f64) -> Message {
    at(
        ms,
        json!({ "kind": "position", "entity": entity, "x": x, "y": y }),
    )
}

#[test]
fn counts_messages_with_an_integer_state_and_outputs_it_with_every_message() {
    let mut stage = stage(json!({
        "initial": { "messages": 0, "total": 0.0 },
        "update": [
            { "field": "messages", "expression": "state.messages + 1" },
            { "field": "total", "expression": "state.total + value" },
        ],
    }));

    let outputs = run(
        &mut stage,
        vec![
            at(0, json!({ "value": 1.5 })),
            at(10, json!({ "value": 2 })),
        ],
    );
    assert_eq!(
        outputs,
        vec![
            json!({ "messages": 1, "total": 1.5 }),
            json!({ "messages": 2, "total": 3.5 }),
        ]
    );
}

#[test]
fn an_odometer_per_entity_starts_at_zero_and_keeps_keys_apart() {
    let mut stage = stage(json!({
        "initial": { "distance_m": 0.0 },
        "update": [{
            "field": "distance_m",
            "expression": "state.distance_m + math::hypot(x - prev.x, y - prev.y)",
            "when": { "field_path": "kind", "operation": "==", "value": "position" },
        }],
        "group_by": ["entity"],
    }));

    let outputs = run(
        &mut stage,
        vec![
            // The first position has no previous one: prev is itself, so it adds 0.
            position(0, "MM1", 1.0, 1.0),
            position(0, "MM2", 10.0, 10.0),
            position(100, "MM1", 4.0, 5.0),
            // Not a position: the state is output unchanged.
            at(
                150,
                json!({ "kind": "action", "entity": "MM1", "x": 4.0, "y": 5.0 }),
            ),
            position(200, "MM1", 4.0, 7.0),
            position(200, "MM2", 10.0, 13.0),
        ],
    );
    let distances: Vec<_> = outputs
        .iter()
        .map(|output| (output["entity"].clone(), output["distance_m"].clone()))
        .collect();
    assert_eq!(
        distances,
        vec![
            (json!("MM1"), json!(0.0)),
            (json!("MM2"), json!(0.0)),
            (json!("MM1"), json!(5.0)),
            (json!("MM1"), json!(5.0)),
            (json!("MM1"), json!(7.0)),
            (json!("MM2"), json!(3.0)),
        ]
    );
    assert_eq!(outputs[0], json!({ "entity": "MM1", "distance_m": 0.0 }));
}

#[test]
fn updates_read_the_state_from_before_the_message() {
    let mut stage = stage(json!({
        "initial": { "a": 1, "b": 2 },
        "update": [
            { "field": "a", "expression": "state.b" },
            { "field": "b", "expression": "state.a" },
        ],
    }));

    let outputs = run(&mut stage, vec![at(0, json!({})), at(1, json!({}))]);
    assert_eq!(
        outputs,
        vec![json!({ "a": 2, "b": 1 }), json!({ "a": 1, "b": 2 })]
    );
}

#[test]
fn reset_when_restarts_the_state_count_and_prev() {
    let mut stage = stage(json!({
        "initial": { "distance_m": 0.0, "messages": 0, "since": 0 },
        "update": [
            { "field": "distance_m", "expression": "state.distance_m + math::abs(x - prev.x)" },
            { "field": "messages", "expression": "count" },
            { "field": "since", "expression": "if(count == 1, seq, state.since)" },
        ],
        "reset_when": { "field_path": "action", "operation": "==", "value": "place" },
    }));

    let outputs = run(
        &mut stage,
        vec![
            at(0, json!({ "seq": 1, "x": 0.0 })),
            at(1, json!({ "seq": 2, "x": 2.0 })),
            at(2, json!({ "seq": 3, "x": 5.0 })),
            // Resets: this message is the first again, with prev = itself.
            at(3, json!({ "seq": 4, "x": 9.0, "action": "place" })),
            at(4, json!({ "seq": 5, "x": 10.0 })),
        ],
    );
    let rows: Vec<_> = outputs
        .iter()
        .map(|output| {
            (
                output["distance_m"].as_f64().unwrap(),
                output["messages"].clone(),
                output["since"].clone(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        vec![
            (0.0, json!(1), json!(1.0)),
            (2.0, json!(2), json!(1.0)),
            (5.0, json!(3), json!(1.0)),
            (0.0, json!(1), json!(4.0)),
            (1.0, json!(2), json!(4.0)),
        ]
    );
}

#[test]
fn dt_is_the_event_time_since_the_previous_message() {
    let mut stage = stage(json!({
        "initial": { "kwh": 0.0, "dt": 0.0 },
        "update": [
            { "field": "kwh", "expression": "state.kwh + power * dt / 3600.0" },
            { "field": "dt", "expression": "dt" },
        ],
    }));

    let outputs = run(
        &mut stage,
        vec![
            at(0, json!({ "power": 3600.0 })),
            at(2_000, json!({ "power": 3600.0 })),
            at(2_500, json!({ "power": 7200.0 })),
            // Older than the previous message: dt is 0, with a warning.
            at(1_000, json!({ "power": 3600.0 })),
        ],
    );
    let rows: Vec<_> = outputs
        .iter()
        .map(|output| {
            (
                output["dt"].as_f64().unwrap(),
                output["kwh"].as_f64().unwrap(),
            )
        })
        .collect();
    assert_eq!(rows, vec![(0.0, 0.0), (2.0, 2.0), (0.5, 3.0), (0.0, 3.0)]);
    assert!(stage.warnings.has_warned("out-of-order"));
}

#[test]
fn results_can_be_booleans_and_strings() {
    // A hysteresis alarm: on above 80, off again below 70.
    let mut stage = stage(json!({
        "initial": { "alarm": false, "label": "ok" },
        "update": [
            { "field": "alarm", "expression": "if(state.alarm, value >= 70.0, value > 80.0)" },
            { "field": "label", "expression": "if(value > 80.0, \"high\", \"ok\")" },
        ],
    }));

    let outputs = run(
        &mut stage,
        [60.0, 85.0, 75.0, 65.0]
            .into_iter()
            .enumerate()
            .map(|(index, value)| at(index as u64, json!({ "value": value })))
            .collect(),
    );
    let alarms: Vec<_> = outputs
        .iter()
        .map(|output| output["alarm"].clone())
        .collect();
    assert_eq!(
        alarms,
        vec![json!(false), json!(true), json!(true), json!(false)]
    );
    assert_eq!(outputs[1]["label"], json!("high"));
    assert_eq!(outputs[2]["label"], json!("ok"));
}

#[test]
fn a_failing_update_keeps_its_value_and_warns_once() {
    let mut stage = stage(json!({
        "initial": { "total": 0.0, "messages": 0 },
        "update": [
            { "field": "total", "expression": "state.total + value" },
            { "field": "messages", "expression": "count" },
        ],
    }));

    let outputs = run(
        &mut stage,
        vec![
            at(0, json!({ "value": 1.0 })),
            at(1, json!({ "other": 1.0 })),
            at(2, json!({ "other": 1.0 })),
        ],
    );
    assert_eq!(outputs[2], json!({ "total": 1.0, "messages": 3 }));
    assert!(stage.warnings.has_warned("update:0"));
    assert!(!stage.warnings.has_warned("update:1"));
}

#[test]
fn a_hidden_message_field_is_read_through_msg() {
    let mut stage = stage(json!({
        "initial": { "running": false, "messages": 0 },
        "update": [
            { "field": "running", "expression": "msg.state == \"running\"" },
            { "field": "messages", "expression": "count" },
        ],
        "reset_when": { "field_path": "msg.state", "operation": "==", "value": "stopped" },
    }));

    let outputs = run(
        &mut stage,
        vec![
            at(0, json!({ "state": "running", "count": 99 })),
            at(1, json!({ "state": "stopped" })),
        ],
    );
    assert_eq!(outputs[0], json!({ "running": true, "messages": 1 }));
    assert_eq!(outputs[1], json!({ "running": false, "messages": 1 }));
    assert!(stage.warnings.has_warned("hidden:state"));
    assert!(stage.warnings.has_warned("hidden:count"));
}

#[test]
fn include_message_lays_the_state_over_the_message() {
    let mut stage = stage(json!({
        "initial": { "max": -1.0e9, "value": 0.0 },
        "update": [
            { "field": "max", "expression": "max(state.max, value)" },
            { "field": "value", "expression": "value * 10.0" },
        ],
        "group_by": ["sensor"],
        "include_message": true,
    }));

    let outputs = run(
        &mut stage,
        vec![at(0, json!({ "sensor": "a", "value": 2.0, "unit": "C" }))],
    );
    assert_eq!(
        outputs[0],
        json!({ "sensor": "a", "value": 20.0, "unit": "C", "max": 2.0 })
    );
}

#[test]
fn skips_messages_without_a_key_or_dropped_by_timing() {
    let mut stage = stage(json!({
        "initial": { "messages": 0 },
        "update": [{ "field": "messages", "expression": "count" }],
        "group_by": ["sensor"],
    }));

    let mut expired = at(0, json!({ "sensor": "a" }));
    expired.timing.processing_deadline = Some(UNIX_EPOCH);
    let outputs = run(
        &mut stage,
        vec![
            expired,
            at(1, json!({ "value": 1.0 })),
            at(2, json!({ "sensor": "a" })),
        ],
    );
    assert_eq!(outputs, vec![json!({ "sensor": "a", "messages": 1 })]);
    assert!(stage.warnings.has_warned("missing-key"));
}

#[tokio::test]
async fn publishes_outputs_with_the_input_timing() {
    let mut stage = stage(json!({
        "initial": { "messages": 0 },
        "update": [{ "field": "messages", "expression": "count" }],
    }));
    let mut context = ProcessingContext::new("reduce".to_string());
    let (input, receiver) = mpsc::channel(4);
    context.add_input("in".to_string(), Subscriber::Mpsc(receiver));
    let channel = Arc::new(Channel::new(ChannelType::Broadcast, 4));
    let mut output = channel.subscribe();
    context.attach_output("state".to_string(), channel);

    let message = at(100, json!({ "value": 1.0 })).with_sequence_id(7);
    let event_time = message.timing.event_time;
    input.send(message).await.unwrap();
    stage.process(&mut context).await.unwrap();

    let result = output.try_recv().await.expect("a reduce output");
    assert_eq!(result.source, "reduce");
    assert_eq!(result.topic, "state");
    assert_eq!(result.payload, json!({ "messages": 1 }));
    assert_eq!(result.timing.event_time, event_time);
    assert_eq!(result.timing.sequence_id, Some(7));
}
