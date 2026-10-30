use super::*;
use crate::scenario::parse_scenario;

const SCENARIO: &str = r#"
    name = "Engine test"

    [floor]
    width_m = 20.0
    height_m = 10.0

    [machines.machine1]
    position = [4.0, 8.0]
    size = [2.0, 1.0]          # dock (4.0, 7.0)

    [machines.machine2]
    position = [12.0, 8.0]
    size = [2.0, 1.0]          # dock (12.0, 7.0)

    [machines.machine2.sensors.temperature]
    unit = "C"
    default = 40.0
    range = [0.0, 100.0]
    period_ms = 1000

    [mobile_manipulators.mm1]
    start_at = "machine1"
    max_speed_mps = 0.8

    [mobile_manipulators.mm1.sensors.vibration]
    unit = "g"
    default = 0.1
    range = [0.0, 2.0]
    period_ms = 500

    [mobile_manipulators.mm2]
    start = [2.0, 2.0]
    max_speed_mps = 0.5
    actions = ["scan"]

    [sensors.hall_temp]
    position = [4.0, 5.0]
    default = 22.0
    range = [0.0, 50.0]
    period_ms = 1000

    [actions.pick]
    requires_machine = true

    [actions.scan]
    allowed_while_moving = true

    [actions.wave]
"#;

const TS: u64 = 1_761_730_000_000;

fn engine() -> Engine {
    Engine::new(Arc::new(parse_scenario(SCENARIO, None).unwrap()))
}

fn ui(command: SimCommand) -> CommandEnvelope {
    CommandEnvelope::new(Origin::Ui, command)
}

fn move_to(mm: &str, target: Target) -> CommandEnvelope {
    ui(SimCommand::MoveTo {
        mm: mm.into(),
        target,
    })
}

fn machine(id: &str) -> Target {
    Target::Machine(id.into())
}

fn run(engine: &mut Engine, ticks: usize) -> Vec<SimEvent> {
    (0..ticks).flat_map(|_| engine.step(TS)).collect()
}

fn positions_of<'a>(events: &'a [SimEvent], mm: &str) -> Vec<&'a SimEvent> {
    events
        .iter()
        .filter(|e| matches!(&e.body, EventBody::Position { entity, .. } if entity == mm))
        .collect()
}

fn place(x: f64, y: f64, at: Option<&str>) -> Place {
    Place {
        x,
        y,
        at: at.map(str::to_string),
    }
}

#[test]
fn move_to_machine_reports_from_and_to_docks() {
    let mut engine = engine();
    let events = engine.apply(move_to("mm1", machine("machine2")), TS).unwrap();
    assert_eq!(
        events,
        vec![SimEvent {
            v: 1,
            seq: 1,
            ts: TS,
            sim_ms: 0,
            body: EventBody::MoveCommand {
                entity: "mm1".into(),
                command_id: 1,
                origin: Origin::Ui,
                from: place(4.0, 7.0, Some("machine1")),
                to: place(12.0, 7.0, Some("machine2")),
                distance_m: 8.0,
                eta_ms: 10_000,
                replaces: None,
            },
        }]
    );
}

#[test]
fn moving_mm_reports_position_every_interval_then_arrives() {
    let mut engine = engine();
    engine.apply(move_to("mm1", machine("machine2")), TS).unwrap();
    let events = run(&mut engine, 600); // 12 s; the trip takes 10 s

    let positions = positions_of(&events, "mm1");
    let times: Vec<u64> = positions.iter().map(|e| e.sim_ms).collect();
    let expected: Vec<u64> = (1..=100).map(|i| i * 100).collect();
    assert_eq!(times, expected, "every 100 ms while moving, plus the arrival");

    assert_eq!(
        positions[0].body,
        EventBody::Position {
            entity: "mm1".into(),
            x: 4.08,
            y: 7.0,
            heading_rad: 0.0,
            speed_mps: 0.8,
        }
    );
    assert_eq!(
        positions.last().unwrap().body,
        EventBody::Position {
            entity: "mm1".into(),
            x: 12.0,
            y: 7.0,
            heading_rad: 0.0,
            speed_mps: 0.0,
        }
    );

    let arrivals: Vec<&SimEvent> = events
        .iter()
        .filter(|e| matches!(e.body, EventBody::Arrived { .. }))
        .collect();
    assert_eq!(arrivals.len(), 1);
    assert_eq!(arrivals[0].sim_ms, 10_000);
    assert_eq!(
        arrivals[0].body,
        EventBody::Arrived {
            entity: "mm1".into(),
            command_id: 1,
            x: 12.0,
            y: 7.0,
            at: Some("machine2".into()),
        }
    );
    let state = engine.state();
    assert_eq!(state.mobile_manipulators[0].at.as_deref(), Some("machine2"));
    assert!(state.mobile_manipulators[0].motion.is_none());
}

#[test]
fn idle_mms_report_no_positions() {
    let mut engine = engine();
    let events = run(&mut engine, 100);
    assert!(positions_of(&events, "mm1").is_empty());
    assert!(positions_of(&events, "mm2").is_empty());
}

#[test]
fn heading_follows_direction_of_travel() {
    let mut engine = engine();
    engine
        .apply(move_to("mm2", Target::Point { x: 2.0, y: 5.0 }), TS)
        .unwrap();
    let events = run(&mut engine, 5);
    match &positions_of(&events, "mm2")[0].body {
        // Rounded to 4 decimals in the event.
        EventBody::Position { heading_rad, .. } => {
            assert_eq!(*heading_rad, round_to(std::f64::consts::FRAC_PI_2, 4))
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn redirect_mid_move_replaces_previous_command() {
    let mut engine = engine();
    engine.apply(move_to("mm1", machine("machine2")), TS).unwrap();
    run(&mut engine, 250); // 5 s → halfway, x = 8.0

    let events = engine
        .apply(move_to("mm1", Target::Point { x: 8.0, y: 3.0 }), TS)
        .unwrap();
    assert_eq!(
        events[0].body,
        EventBody::MoveCommand {
            entity: "mm1".into(),
            command_id: 2,
            origin: Origin::Ui,
            from: place(8.0, 7.0, None),
            to: place(8.0, 3.0, None),
            distance_m: 4.0,
            eta_ms: 5_000,
            replaces: Some(1),
        }
    );

    let events = run(&mut engine, 300);
    let arrivals: Vec<(u64, u64)> = events
        .iter()
        .filter_map(|e| match e.body {
            EventBody::Arrived { command_id, .. } => Some((command_id, e.sim_ms)),
            _ => None,
        })
        .collect();
    assert_eq!(arrivals, vec![(2, 10_000)]);
}

#[test]
fn stop_halts_motion() {
    let mut engine = engine();
    engine.apply(move_to("mm1", machine("machine2")), TS).unwrap();
    run(&mut engine, 100); // 2 s → x = 5.6

    let events = engine
        .apply(ui(SimCommand::Stop { mm: "mm1".into() }), TS)
        .unwrap();
    assert_eq!(
        events[0].body,
        EventBody::Stopped {
            entity: "mm1".into(),
            command_id: 1,
            x: 5.6,
            y: 7.0,
            at: None,
            origin: Origin::Ui,
        }
    );
    assert!(positions_of(&run(&mut engine, 100), "mm1").is_empty());

    // Stopping an idle MM is a no-op.
    let events = engine
        .apply(ui(SimCommand::Stop { mm: "mm1".into() }), TS)
        .unwrap();
    assert!(events.is_empty());
}

#[test]
fn move_to_current_position_arrives_immediately() {
    let mut engine = engine();
    let events = engine.apply(move_to("mm1", machine("machine1")), TS).unwrap();
    let kinds: Vec<&str> = events.iter().map(SimEvent::kind).collect();
    assert_eq!(kinds, vec!["move_command", "arrived"]);
    assert!(matches!(
        events[0].body,
        EventBody::MoveCommand {
            distance_m: 0.0,
            eta_ms: 0,
            ..
        }
    ));
    assert!(engine.state().mobile_manipulators[0].motion.is_none());
}

#[test]
fn point_target_near_a_dock_is_labelled_with_that_machine() {
    let mut engine = engine();
    let events = engine
        .apply(move_to("mm2", Target::Point { x: 12.1, y: 7.1 }), TS)
        .unwrap();
    match &events[0].body {
        EventBody::MoveCommand { to, .. } => assert_eq!(to.at.as_deref(), Some("machine2")),
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn sensors_report_on_their_period_and_follow_setpoints() {
    let mut engine = engine();
    let events = run(&mut engine, 50); // 1 s
    let readings: Vec<(String, u64, f64)> = events
        .iter()
        .filter_map(|e| match &e.body {
            EventBody::Sensor { sensor, value, .. } => Some((sensor.clone(), e.sim_ms, *value)),
            _ => None,
        })
        .collect();
    assert_eq!(
        readings,
        vec![
            ("mm1.vibration".to_string(), 500, 0.1),
            ("machine2.temperature".to_string(), 1000, 40.0),
            ("mm1.vibration".to_string(), 1000, 0.1),
            ("hall_temp".to_string(), 1000, 22.0),
        ]
    );

    let events = engine
        .apply(
            ui(SimCommand::SetSensor {
                sensor: "mm1.vibration".into(),
                value: 0.8,
            }),
            TS,
        )
        .unwrap();
    assert_eq!(
        events[0].body,
        EventBody::SensorSet {
            sensor: "mm1.vibration".into(),
            entity: "mm1".into(),
            sensor_kind: "vibration".into(),
            setpoint: 0.8,
            noise_std: 0.0,
            previous_setpoint: 0.1,
            origin: Origin::Ui,
        }
    );
    let events = run(&mut engine, 25);
    assert!(events.iter().any(|e| e.sim_ms == 1500
        && matches!(&e.body, EventBody::Sensor { sensor, value, .. } if sensor == "mm1.vibration" && *value == 0.8)));
    assert_eq!(engine.state().sensors[1].last_value, Some(0.8));
}

#[test]
fn noise_is_seeded_and_clamped() {
    fn noisy_run() -> Vec<SimEvent> {
        let mut engine = engine();
        engine
            .apply(
                ui(SimCommand::SetSensorNoise {
                    sensor: "mm1.vibration".into(),
                    std: 1.0,
                }),
                TS,
            )
            .unwrap();
        run(&mut engine, 1000)
    }
    let first = noisy_run();
    assert_eq!(first, noisy_run(), "same seed, same readings");

    let values: Vec<f64> = first
        .iter()
        .filter_map(|e| match &e.body {
            EventBody::Sensor { sensor, value, .. } if sensor == "mm1.vibration" => Some(*value),
            _ => None,
        })
        .collect();
    assert_eq!(values.len(), 40);
    assert!(values.iter().all(|v| (0.0..=2.0).contains(v)));
    assert!(values.contains(&0.0), "large noise around 0.1 hits the lower clamp");
    assert!(values.iter().any(|v| *v != 0.0 && *v != 0.1));
}

#[test]
fn invalid_commands_are_rejected_without_side_effects() {
    let mut engine = engine();
    let before = engine.state();
    let cases = [
        (move_to("mm9", machine("machine1")), "Unknown mobile manipulator 'mm9' (known: mm1, mm2)"),
        (move_to("mm1", machine("machine9")), "Unknown machine 'machine9'"),
        (move_to("mm1", Target::Point { x: 0.1, y: 5.0 }), "outside the area mm1 can reach"),
        (move_to("mm1", Target::Point { x: f64::NAN, y: 5.0 }), "outside the area"),
        (
            ui(SimCommand::SetSensor {
                sensor: "mm1.vibration".into(),
                value: 3.0,
            }),
            "outside its range [0, 2]",
        ),
        (
            ui(SimCommand::SetSensor {
                sensor: "mm1.nope".into(),
                value: 1.0,
            }),
            "Unknown sensor 'mm1.nope'",
        ),
        (
            ui(SimCommand::SetSensorNoise {
                sensor: "mm1.vibration".into(),
                std: -1.0,
            }),
            "must be >= 0",
        ),
        (ui(SimCommand::Stop { mm: "mm9".into() }), "Unknown mobile manipulator"),
    ];
    for (command, expected) in cases {
        let error = engine.apply(command.clone(), TS).unwrap_err().to_string();
        assert!(error.contains(expected), "{command:?}: {error}");
    }
    assert_eq!(engine.state(), before);

    // Nothing was consumed: the next valid command gets seq 1 and command_id 1.
    let events = engine.apply(move_to("mm1", machine("machine2")), TS).unwrap();
    assert_eq!(events[0].seq, 1);
    assert!(matches!(events[0].body, EventBody::MoveCommand { command_id: 1, .. }));
}

#[test]
fn pause_freezes_time_and_reset_restores_start() {
    let mut engine = engine();
    engine.apply(move_to("mm1", machine("machine2")), TS).unwrap();
    run(&mut engine, 50);
    assert_eq!(engine.sim_ms(), 1000);

    let events = engine.control(Control::Pause, TS);
    assert!(matches!(
        events[0].body,
        EventBody::Sim {
            state: Lifecycle::Paused,
            ..
        }
    ));
    assert!(engine.control(Control::Pause, TS).is_empty());
    assert!(run(&mut engine, 50).is_empty());
    assert_eq!(engine.sim_ms(), 1000);
    assert!(engine.state().paused);

    engine.control(Control::Resume, TS);
    assert!(!run(&mut engine, 5).is_empty());

    engine
        .apply(
            ui(SimCommand::SetSensor {
                sensor: "mm1.vibration".into(),
                value: 1.5,
            }),
            TS,
        )
        .unwrap();
    let events = engine.control(Control::Reset, TS);
    assert_eq!(events[0].sim_ms, 0);
    assert!(events[0].seq > 1, "sequence numbers continue across a reset");
    assert_eq!(engine.state(), self::engine().state());
}

#[test]
fn sequence_numbers_are_contiguous() {
    let mut engine = engine();
    let mut events = engine.start(TS);
    events.extend(engine.apply(move_to("mm1", machine("machine2")), TS).unwrap());
    events.extend(run(&mut engine, 200));
    events.extend(engine.apply(move_to("mm2", machine("machine1")), TS).unwrap());
    events.extend(run(&mut engine, 200));
    let seqs: Vec<u64> = events.iter().map(|e| e.seq).collect();
    let expected: Vec<u64> = (1..=events.len() as u64).collect();
    assert_eq!(seqs, expected);
    assert_eq!(events[0].kind(), "sim");
}

#[test]
fn state_reports_motion_progress_and_dock() {
    let mut engine = engine();
    assert_eq!(engine.state().mobile_manipulators[0].at.as_deref(), Some("machine1"));

    engine.apply(move_to("mm1", machine("machine2")), TS).unwrap();
    let motion = engine.state().mobile_manipulators[0].motion.clone().unwrap();
    assert_eq!((motion.remaining_m, motion.eta_ms), (8.0, 10_000));

    run(&mut engine, 125); // 2.5 s → 2 m covered
    let state = engine.state();
    let mm1 = &state.mobile_manipulators[0];
    assert!(mm1.at.is_none());
    assert_eq!(mm1.speed_mps, 0.8);
    let motion = mm1.motion.as_ref().unwrap();
    assert!((motion.remaining_m - 6.0).abs() < 1e-9);
    assert_eq!(motion.eta_ms, 7_500);
}

// --- Actions -------------------------------------------------------------------------------

fn perform(mm: &str, action: &str) -> CommandEnvelope {
    ui(SimCommand::Perform {
        mm: mm.into(),
        action: action.into(),
    })
}

fn availability(engine: &Engine, mm: usize, action: &str) -> ActionAvailability {
    engine.state().mobile_manipulators[mm]
        .actions
        .iter()
        .find(|a| a.id == action)
        .cloned()
        .unwrap()
}

#[test]
fn perform_at_a_dock_emits_action_and_changes_nothing() {
    let mut engine = engine();
    let before = engine.state();
    let events = engine.apply(perform("mm1", "pick"), TS).unwrap();
    assert_eq!(
        events,
        vec![SimEvent {
            v: 1,
            seq: 1,
            ts: TS,
            sim_ms: 0,
            body: EventBody::Action {
                entity: "mm1".into(),
                action: "pick".into(),
                command_id: 1,
                x: 4.0,
                y: 7.0,
                at: Some("machine1".into()),
                origin: Origin::Ui,
            },
        }]
    );
    assert_eq!(engine.state(), before, "actions are silent");
}

#[test]
fn machine_only_actions_are_rejected_away_from_a_dock() {
    let mut engine = engine();
    engine
        .apply(move_to("mm1", Target::Point { x: 6.0, y: 3.0 }), TS)
        .unwrap();
    run(&mut engine, 300);
    let error = engine.apply(perform("mm1", "pick"), TS).unwrap_err();
    assert_eq!(error.to_string(), "'pick' needs mm1 at a machine; it is at (6.0, 3.0)");
    // Actions without `requires_machine` work anywhere.
    let events = engine.apply(perform("mm1", "wave"), TS).unwrap();
    assert!(matches!(&events[0].body, EventBody::Action { at: None, .. }));
}

#[test]
fn moving_mms_only_perform_actions_allowed_while_moving() {
    let mut engine = engine();
    engine.apply(move_to("mm1", machine("machine2")), TS).unwrap();
    run(&mut engine, 10); // 0.16 m out: still within the dock tolerance of machine1
    let error = engine.apply(perform("mm1", "pick"), TS).unwrap_err();
    assert_eq!(error.to_string(), "'pick' needs mm1 to be stopped");
    let events = engine.apply(perform("mm1", "scan"), TS).unwrap();
    assert!(matches!(&events[0].body, EventBody::Action { at: Some(m), .. } if m == "machine1"));

    run(&mut engine, 40); // 0.8 m out: between machines
    let events = engine.apply(perform("mm1", "scan"), TS).unwrap();
    match &events[0].body {
        EventBody::Action { x, at, command_id, .. } => {
            assert_eq!((*x, at.clone(), *command_id), (4.8, None, 3), "ids shared with moves");
        }
        other => panic!("unexpected {other:?}"),
    }
    // Still moving: the action changed nothing.
    assert!(engine.state().mobile_manipulators[0].motion.is_some());
}

#[test]
fn mms_only_perform_their_own_actions() {
    let mut engine = engine();
    let error = engine.apply(perform("mm2", "pick"), TS).unwrap_err();
    assert_eq!(error.to_string(), "mm2 cannot perform 'pick' (its actions: scan)");
    let error = engine.apply(perform("mm1", "fly"), TS).unwrap_err();
    assert_eq!(error.to_string(), "Unknown action 'fly' (known: pick, scan, wave)");
    let error = engine.apply(perform("mm9", "scan"), TS).unwrap_err();
    assert!(error.to_string().contains("Unknown mobile manipulator 'mm9'"));
    assert!(engine.apply(perform("mm2", "scan"), TS).is_ok());

    let mm2_actions: Vec<String> = engine.state().mobile_manipulators[1]
        .actions
        .iter()
        .map(|a| a.id.clone())
        .collect();
    assert_eq!(mm2_actions, vec!["scan"]);
}

#[test]
fn action_availability_follows_the_mm() {
    let mut engine = engine();
    assert_eq!(
        availability(&engine, 0, "pick"),
        ActionAvailability {
            id: "pick".into(),
            available: true,
            reason: None
        }
    );

    engine.apply(move_to("mm1", machine("machine2")), TS).unwrap();
    run(&mut engine, 10);
    let pick = availability(&engine, 0, "pick");
    assert!(!pick.available);
    assert_eq!(pick.reason.as_deref(), Some("'pick' needs mm1 to be stopped"));
    assert!(availability(&engine, 0, "scan").available);
    assert!(!availability(&engine, 0, "wave").available);

    run(&mut engine, 600);
    assert!(availability(&engine, 0, "pick").available, "docked at machine2");
}

// --- Sensor positions ------------------------------------------------------------------------

fn reading_positions(events: &[SimEvent], id: &str) -> Vec<(u64, f64, f64)> {
    events
        .iter()
        .filter_map(|e| match &e.body {
            EventBody::Sensor { sensor, x, y, .. } if sensor == id => Some((e.sim_ms, *x, *y)),
            _ => None,
        })
        .collect()
}

#[test]
fn sensor_readings_carry_where_they_were_taken() {
    let mut engine = engine();
    engine.apply(move_to("mm1", machine("machine2")), TS).unwrap();
    let events = run(&mut engine, 50);
    assert_eq!(reading_positions(&events, "hall_temp"), vec![(1000, 4.0, 5.0)]);
    assert_eq!(
        reading_positions(&events, "machine2.temperature"),
        vec![(1000, 12.0, 8.0)],
        "machine sensors report from the machine centre"
    );
    assert_eq!(
        reading_positions(&events, "mm1.vibration"),
        vec![(500, 4.4, 7.0), (1000, 4.8, 7.0)],
        "MM sensors move with the MM"
    );
}
