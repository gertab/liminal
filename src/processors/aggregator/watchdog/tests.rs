//! The watchdog stage's behaviour, driven through `step` with explicit times, plus one test of
//! `process` for publishing.

use super::*;
use crate::config::types::ChannelType;
use crate::core::channel::{Channel, PubSubChannel, Subscriber};
use crate::core::message::Message;
use std::sync::Arc;
use tokio::sync::mpsc;

/// Epoch ms the tests start at.
const BASE: u64 = 1_000_000;

fn watchdog_with(parameters: Value, timing: Option<Value>, inputs: &[&str]) -> WatchdogStage {
    let Value::Object(parameters) = parameters else {
        panic!("parameters must be an object");
    };
    let config = StageConfig {
        r#type: "watchdog".to_string(),
        inputs: Some(inputs.iter().map(|input| input.to_string()).collect()),
        output: Some("alerts".to_string()),
        concurrency: None,
        channel: None,
        timing: timing.map(|timing| serde_json::from_value(timing).expect("timing parses")),
        parameters: Some(parameters.into_iter().collect()),
    };
    WatchdogStage::from_config("watchdog", &config).expect("valid config")
}

fn watchdog(parameters: Value) -> WatchdogStage {
    watchdog_with(parameters, None, &["in"])
}

/// A message on `input` whose event time is `ms` after `BASE`.
fn on(input: &str, ms: u64, payload: Value) -> InputMessage {
    InputMessage {
        input_name: input.to_string(),
        message: Message::new_with_event_time(
            "source",
            input,
            payload,
            UNIX_EPOCH + Duration::from_millis(BASE + ms),
        ),
    }
}

/// A reading from `sensor` (live tests: its event time doesn't matter, its arrival does).
fn reading(sensor: &str) -> InputMessage {
    on("in", 0, json!({ "sensor": sensor, "value": 1.0 }))
}

/// A round at `ms` after `BASE`; returns the alerts.
fn step(stage: &mut WatchdogStage, ms: u64, messages: Vec<InputMessage>) -> Vec<Value> {
    stage
        .step(BASE + ms, messages)
        .into_iter()
        .map(|pending| pending.payload)
        .collect()
}

fn statuses(alerts: &[Value]) -> Vec<(String, String)> {
    alerts
        .iter()
        .map(|alert| {
            let key = alert
                .get("sensor")
                .or_else(|| alert.get("input"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            (
                key.to_string(),
                alert["status"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

fn silent(key: &str) -> (String, String) {
    (key.to_string(), "silent".to_string())
}

#[test]
fn alerts_once_when_a_key_has_been_silent_for_its_timeout() {
    let mut stage = watchdog(json!({ "group_by": ["sensor"], "timeout_ms": 1000 }));

    assert!(step(&mut stage, 0, vec![reading("a")]).is_empty());
    assert!(step(&mut stage, 600, vec![reading("a")]).is_empty());
    assert!(step(&mut stage, 1599, vec![]).is_empty());

    let alerts = step(&mut stage, 1600, vec![]);
    assert_eq!(
        alerts,
        vec![json!({
            "sensor": "a", "status": "silent", "timeout_ms": 1000, "silent_ms": 1000, "last_seen": BASE + 600,
        })]
    );
    // One alert per silence.
    assert!(step(&mut stage, 9000, vec![]).is_empty());
}

#[test]
fn a_key_can_have_its_own_timeout() {
    let mut stage = watchdog(json!({
        "group_by": ["sensor"], "timeout_ms": 1000, "timeouts": { "machine2.temperature": 3000 },
    }));

    step(
        &mut stage,
        0,
        vec![reading("mm1.vibration"), reading("machine2.temperature")],
    );
    assert_eq!(
        statuses(&step(&mut stage, 1000, vec![])),
        vec![silent("mm1.vibration")]
    );
    assert!(step(&mut stage, 2999, vec![]).is_empty());
    let alerts = step(&mut stage, 3000, vec![]);
    assert_eq!(statuses(&alerts), vec![silent("machine2.temperature")]);
    assert_eq!(alerts[0]["timeout_ms"], json!(3000));
}

#[test]
fn expected_keys_are_watched_from_the_start() {
    let mut stage = watchdog(json!({
        "group_by": ["sensor"], "timeout_ms": 1000, "expected_keys": ["a", "b"],
    }));

    step(&mut stage, 0, vec![reading("a")]);
    let alerts = step(&mut stage, 1000, vec![]);
    assert_eq!(statuses(&alerts), vec![silent("a"), silent("b")]);
    // b never reported: no last_seen.
    assert_eq!(alerts[0]["last_seen"], json!(BASE));
    assert_eq!(
        alerts[1],
        json!({ "sensor": "b", "status": "silent", "timeout_ms": 1000, "silent_ms": 1000 })
    );
}

#[test]
fn emit_recovered_reports_the_gap_and_the_key_can_go_silent_again() {
    let mut stage = watchdog(json!({
        "group_by": ["sensor"], "timeout_ms": 1000, "emit_recovered": true,
    }));

    step(&mut stage, 0, vec![reading("a")]);
    assert_eq!(statuses(&step(&mut stage, 1000, vec![])), vec![silent("a")]);
    let alerts = step(&mut stage, 4000, vec![reading("a")]);
    assert_eq!(
        alerts,
        vec![json!({
            "sensor": "a", "status": "recovered", "timeout_ms": 1000, "silent_ms": 4000, "last_seen": BASE + 4000,
        })]
    );
    assert_eq!(statuses(&step(&mut stage, 5000, vec![])), vec![silent("a")]);
}

#[test]
fn without_emit_recovered_a_returning_key_is_quiet() {
    let mut stage = watchdog(json!({ "group_by": ["sensor"], "timeout_ms": 1000 }));

    step(&mut stage, 0, vec![reading("a")]);
    step(&mut stage, 1000, vec![]);
    assert!(step(&mut stage, 4000, vec![reading("a")]).is_empty());
    assert_eq!(statuses(&step(&mut stage, 5000, vec![])), vec![silent("a")]);
}

#[test]
fn without_group_by_each_input_is_watched_from_the_start() {
    let mut stage = watchdog_with(
        json!({ "timeout_ms": 1000, "timeouts": { "status": 3000 } }),
        None,
        &["readings", "status"],
    );

    step(
        &mut stage,
        0,
        vec![on("readings", 0, json!({ "value": 1.0 }))],
    );
    let alerts = step(&mut stage, 1000, vec![]);
    assert_eq!(
        alerts,
        vec![json!({
            "input": "readings", "status": "silent", "timeout_ms": 1000, "silent_ms": 1000, "last_seen": BASE,
        })]
    );
    // status never sent anything.
    let alerts = step(&mut stage, 3000, vec![]);
    assert_eq!(statuses(&alerts), vec![silent("status")]);
    assert!(alerts[0].get("last_seen").is_none());
}

#[test]
fn an_event_time_watermark_measures_silence_in_event_time() {
    // As when replaying a recording: the readings' own times (ts) move time, not the wall clock.
    let mut stage = watchdog_with(
        json!({ "group_by": ["sensor"], "timeout_ms": 1000, "emit_recovered": true }),
        Some(
            json!({ "watermark_strategy": { "type": "punctuated", "field": "ts" }, "max_lateness_ms": 0 }),
        ),
        &["in"],
    );
    let replayed =
        |ms: u64, sensor: &str| on("in", ms, json!({ "sensor": sensor, "ts": BASE + ms }));
    let much_later = 50_000_000;

    assert!(
        step(
            &mut stage,
            much_later,
            vec![replayed(0, "a"), replayed(0, "b")]
        )
        .is_empty()
    );
    assert!(step(&mut stage, much_later, vec![replayed(500, "b")]).is_empty());
    // No messages: the watermark stands still, whatever the wall clock says.
    assert!(step(&mut stage, much_later + 60_000, vec![]).is_empty());

    let alerts = step(&mut stage, much_later, vec![replayed(1200, "b")]);
    assert_eq!(
        alerts,
        vec![json!({
            "sensor": "a", "status": "silent", "timeout_ms": 1000, "silent_ms": 1200, "last_seen": BASE,
        })]
    );
    // An older reading changes nothing; a newer one is a recovery.
    assert!(step(&mut stage, much_later, vec![replayed(800, "b")]).is_empty());
    let alerts = step(&mut stage, much_later, vec![replayed(1500, "a")]);
    assert_eq!(alerts[0]["status"], json!("recovered"));
    assert_eq!(alerts[0]["silent_ms"], json!(1500));
}

#[test]
fn a_silence_inside_one_batch_is_still_reported_before_its_recovery() {
    // A fast replay can deliver a whole gap at once: a goes quiet from 0 to 1600 while b reports.
    let mut stage = watchdog_with(
        json!({ "group_by": ["sensor"], "timeout_ms": 1000, "emit_recovered": true }),
        Some(
            json!({ "watermark_strategy": { "type": "punctuated", "field": "ts" }, "max_lateness_ms": 0 }),
        ),
        &["in"],
    );
    let replayed =
        |ms: u64, sensor: &str| on("in", ms, json!({ "sensor": sensor, "ts": BASE + ms }));

    let batch = [
        (0, "a"),
        (0, "b"),
        (500, "b"),
        (1000, "b"),
        (1500, "b"),
        (1600, "a"),
    ]
    .into_iter()
    .map(|(ms, sensor)| replayed(ms, sensor))
    .collect();
    let alerts = step(&mut stage, 50_000_000, batch);
    assert_eq!(
        statuses(&alerts),
        vec![silent("a"), ("a".to_string(), "recovered".to_string())]
    );
    // Reported when the watermark reached the deadline (b at 1000), not at the end of the batch.
    assert_eq!(alerts[0]["silent_ms"], json!(1000));
    assert_eq!(alerts[1]["silent_ms"], json!(1600));

    // Live, too: a step that comes late still reports the gap before the message that ends it.
    let mut live =
        watchdog(json!({ "group_by": ["sensor"], "timeout_ms": 1000, "emit_recovered": true }));
    step(&mut live, 0, vec![reading("a")]);
    assert_eq!(
        statuses(&step(&mut live, 5000, vec![reading("a")])),
        vec![silent("a"), ("a".to_string(), "recovered".to_string())]
    );
}

#[test]
fn skips_messages_without_a_key_or_dropped_by_timing() {
    let mut stage = watchdog(json!({ "group_by": ["sensor"], "timeout_ms": 1000 }));

    let mut expired = reading("a");
    expired.message.timing.processing_deadline = Some(UNIX_EPOCH);
    step(
        &mut stage,
        0,
        vec![expired, on("in", 0, json!({ "value": 1.0 }))],
    );
    assert!(step(&mut stage, 5000, vec![]).is_empty());
    assert!(stage.warnings.has_warned("missing-key"));
}

#[test]
fn keys_due_together_are_alerted_in_deadline_order() {
    let mut stage = watchdog(json!({ "group_by": ["sensor"], "timeout_ms": 1000 }));

    step(&mut stage, 0, vec![reading("c")]);
    step(&mut stage, 50, vec![reading("b")]);
    step(&mut stage, 100, vec![reading("a")]);
    assert_eq!(
        statuses(&step(&mut stage, 2000, vec![])),
        vec![silent("c"), silent("b"), silent("a")]
    );
}

#[test]
fn numeric_keys_are_named_as_written() {
    let mut stage = watchdog(json!({
        "group_by": ["machine"], "timeout_ms": 1000, "timeouts": { "7": 3000 }, "expected_keys": ["7"],
    }));

    step(&mut stage, 0, vec![on("in", 0, json!({ "machine": 7 }))]);
    let alerts = step(&mut stage, 3000, vec![]);
    // One key, with the number from the message (not the expected key's text).
    assert_eq!(alerts.len(), 1);
    assert_eq!(alerts[0]["machine"], json!(7));
    assert_eq!(alerts[0]["timeout_ms"], json!(3000));
}

#[tokio::test]
async fn publishes_alerts_as_timed_messages() {
    let mut stage = watchdog_with(
        json!({ "group_by": ["sensor"], "timeout_ms": 1000 }),
        Some(
            json!({ "watermark_strategy": { "type": "punctuated", "field": "ts" }, "max_lateness_ms": 0 }),
        ),
        &["in"],
    );
    let mut context = ProcessingContext::new("watchdog".to_string());
    let (input, receiver) = mpsc::channel(4);
    context.add_input("in".to_string(), Subscriber::Mpsc(receiver));
    let channel = Arc::new(Channel::new(ChannelType::Broadcast, 4));
    let mut output = channel.subscribe();
    context.attach_output("alerts".to_string(), channel);

    for (ms, sensor) in [(0, "a"), (1000, "b")] {
        input
            .send(on("in", ms, json!({ "sensor": sensor, "ts": BASE + ms })).message)
            .await
            .unwrap();
    }
    stage.process(&mut context).await.unwrap();

    let alert = output.try_recv().await.expect("an alert");
    assert_eq!(alert.source, "watchdog");
    assert_eq!(alert.topic, "alerts");
    assert_eq!(alert.payload["sensor"], json!("a"));
    assert_eq!(alert.payload["status"], json!("silent"));
    assert_eq!(
        alert.timing.event_time,
        UNIX_EPOCH + Duration::from_millis(BASE + 1000)
    );
    assert_eq!(alert.timing.sequence_id, Some(1));
}
