# Liminal Shopfloor

An interactive shopfloor simulator. It lives in the Liminal repo next to the core runtime (`src/`) and the pipeline
GUI (`gui/`), but is a separate application with no code dependency on either.

A rectangle of floor holds fixed **machines** and **mobile manipulators (MMs)**. Both can carry **sensors**, and
sensors can also stand on their own at a fixed spot.
- You send MMs to machines or to any point on the floor.
- MMs perform **actions** such as pick and place. These are *silent*: they only produce an event.
- You change what each sensor reports while the simulation runs.
- Everything that happens is emitted as a stream of JSON events: positions, move commands, actions and sensor readings.

The stream is designed to be consumed by Liminal. Events are written to the in-app log, and optionally to stdout and
JSONL files (see [Todos](#todos) for the TCP link to Liminal).

## Run it

```bash
cd shopfloor
npm install                 # once
npm run tauri dev           # Vite on 127.0.0.1:5174 + the app window
```

- The app opens `scenarios/default.toml`.
- It uses port **5174**, so it can run at the same time as the pipeline GUI (5173).
- App logs go to **stderr**.
- **stdout** carries only event JSON lines (when the `stdout` sink is enabled). For example:
  `npm run tauri dev 2>/dev/null | grep '^{' | jq .`

Tests and checks:

```bash
cd shopfloor
cargo test                  # engine, scenario validation, golden event JSON, runtime + sinks
cargo clippy --all-targets
npm run build               # TypeScript type-check + production bundle
```

## Using the app

| Do | How |
|---|---|
| Select an MM | Click it on the floor or click its card on the right. Click again or press **Esc** to deselect. |
| Send it to a machine | With an MM selected, click the machine. Or use the **Go to** buttons on the MM's card. The MM drives to the machine's *dock* (the small circle). |
| Send it to a point | With an MM selected, click anywhere on the floor. A ghost circle shows where it will stop. |
| Re-target | Give a new target while it moves. The old move is replaced immediately. |
| Stop | **Stop** on the MM's card. |
| Perform an action | Use the action buttons (✋ Pick, Place, …) on the MM's card. A greyed-out button shows why on hover (e.g. "'pick' needs mm1 at a machine"). The action name flashes above the MM, and the card shows the last action. |
| Change a sensor | Use the slider or number field in **Sensors** on the right. **↺** resets it to the default. **noise σ** adds Gaussian noise. Clicking a reading or a ◇ floor sensor on the floor jumps to its control. Free-standing sensors are listed under **Floor**. |
| Pause / reset | Toolbar, or **Space** to pause and resume. Reset returns to start positions, default sensor values and time 0. |
| Inspect the output | The bottom panel shows every event. You can filter by kind or entity. **{}** shows the raw JSON lines that the sinks write. |
| Switch scenario | **Open** (the picker starts in `scenarios/`). **Reload** re-reads the current file. |

## Architecture

```
shopfloor/
  sim/          shopfloor-sim: the engine (pure Rust, no Tauri). Tests live here.
  src-tauri/    shopfloor-gui: thin Tauri shell (commands + event bridge)
  src/          React UI: draws state, sends commands
  scenarios/    layout files (TOML)
  docs/         design notes
```

```
React UI ── invoke("send_command", {move_to …}) ──► SimRuntime (tokio task owning the Engine)
   ▲                                                   │  ticks every sim.tick_ms
   │  "shopfloor://state"  (~30 Hz, WorldState)  ◄─────┤  watch channel
   │  "shopfloor://events" (every 100 ms, batch)  ◄────┤  broadcast channel ──► sinks (stdout, jsonl; own threads)
```

- **`Engine`** ([sim/src/engine.rs](sim/src/engine.rs)) is synchronous and deterministic.
  - It changes only through `apply(command)` and `step()`. One step is one fixed tick of sim time.
  - Both return the events they caused.
  - Wall-clock time is passed in and used only to stamp events. Runs are therefore reproducible, and the engine can be
    stepped faster than real time.
- **Commands** ([sim/src/command.rs](sim/src/command.rs)) are the only way in: `move_to`, `stop`, `perform`,
  `set_sensor` and `set_sensor_noise`.
  - Each one carries an origin: `ui`, `scene` or `api`.
  - The UI is just one command source. Scripted scenes or external tools can send the same commands.
- **Action rules** live in one place, `Engine::action_blocker`.
  - It rejects a `perform` command that isn't allowed.
  - It also fills `MmView.actions` (available + reason), so the UI never re-implements the rules.
- **Path planning** sits behind `trait PathPlanner` ([sim/src/motion.rs](sim/src/motion.rs)). The default,
  `StraightLine`, ignores obstacles.
- **`SimRuntime`** ([sim/src/runtime.rs](sim/src/runtime.rs)) drives an engine in real time and fans out events.
  Loading a scenario starts a fresh runtime.
- **Sinks** ([sim/src/sinks/](sim/src/sinks/)) each run on their own thread. A slow sink skips batches with a warning
  instead of slowing the simulation.

## Scenario files

TOML, in metres and seconds.
- The origin is the bottom-left corner of the floor, with **y pointing up**.
- Heading 0 is +x, counter-clockwise.
- **Unknown keys are errors.** All validation errors are reported together.

See [scenarios/default.toml](scenarios/default.toml) for a commented example.

| Key | Default | Meaning |
|---|---|---|
| `name` | "Untitled scenario" | Shown in the UI and in `sim` events |
| `[sim] tick_ms` | 20 | Fixed engine step |
| `[sim] seed` | 42 | Noise RNG seed (runs are reproducible) |
| `[sim] dock_tolerance_m` | 0.3 | An MM this close to a dock counts as "at" that machine |
| `[floor] width_m`, `height_m` | required | Floor size |
| `[floor] grid_m` | 1.0 | Grid spacing in the UI |
| `[machines.<id>] position`, `size` | required | Footprint centre `[x, y]` and `[w, h]` |
| `[machines.<id>] dock` | 0.5 m below the footprint | Where MMs park to service the machine |
| `[machines.<id>] label` | id | Display name |
| `[mobile_manipulators.<id>] start` *or* `start_at` | one required | `[x, y]`, or a machine id to start docked there |
| `… start_heading_deg` | 0 | Initial heading |
| `… max_speed_mps` | 0.5 | Driving speed (constant) |
| `… radius_m` | 0.35 | Footprint. The MM's centre stays this far from the walls. |
| `… actions` | all of `[actions]` | Ids of the actions this MM can perform |
| `[actions.<id>]` | | An action MMs can perform. It is silent and only emits an `action` event. |
| `… label` | `<id>` humanised | Button label |
| `… requires_machine` | false | Only while docked at a machine (within `dock_tolerance_m` of its dock) |
| `… allowed_while_moving` | false | Otherwise the MM must be stopped |
| `[<machines\|mobile_manipulators>.<id>.sensors.<name>]` | | A sensor mounted on that entity. Its id is `<id>.<name>`. |
| `[sensors.<id>]` + `position = [x, y]` | | A free-standing sensor at a fixed spot. It is its own entity: `entity` = `<id>`. It takes the same keys as below. |
| `… default`, `range` | required | Initial value and `[min, max]`. Readings are clamped to the range. |
| `… kind` | `<name>` | Sensor type, e.g. `vibration`, `lidar` |
| `… unit` | "" | Unit string |
| `… label` | `<name>` humanised (`hall_temp` → "Hall temp") | Display name |
| `… period_ms` | 1000 | Reporting period |
| `… noise_std` | 0 | Gaussian noise σ |
| `[output] position_interval_ms` | 100 | How often a moving MM reports its position |
| `[output.sinks.<name>] type = "stdout"` | | One JSON line per event on stdout |
| `[output.sinks.<name>] type = "jsonl", path, append = true` | | One JSON line per event in a file. The path is relative to the scenario file. `shopfloor/recordings/` is git-ignored. |

Rules worth knowing:
- Entity ids use letters, digits, `_` and `-`.
- Machines, MMs and free-standing sensors share one id namespace.
- Footprints, docks, starts and sensor positions must lie inside the floor.
- Only free-standing sensors have a `position`. A mounted sensor is where its host is.
- Periods must be at least `tick_ms`.

## Output events: the Liminal contract

Every event is **one flat JSON object** with the same envelope:

| Field | Meaning |
|---|---|
| `v` | Schema version, currently `1` |
| `seq` | Sequence number, +1 per event. It continues across Reset and restarts when a scenario is loaded. |
| `ts` | Wall-clock time, epoch **milliseconds** (integer) |
| `sim_ms` | Simulation time in ms. It excludes pauses and restarts at 0 on Reset. |
| `kind` | One of the kinds below |

| `kind` | When | Fields |
|---|---|---|
| `position` | Every `position_interval_ms` while an MM moves, plus once on arrival (with `speed_mps` 0). **Not emitted while idle.** | `entity`, `x`, `y`, `heading_rad`, `speed_mps` |
| `move_command` | An MM gets a target | `entity`, `command_id`, `origin`, `from {x,y,at}`, `to {x,y,at}`, `distance_m`, `eta_ms`, `replaces` (previous `command_id` or null) |
| `arrived` | An MM reaches its target | `entity`, `command_id`, `x`, `y`, `at` |
| `stopped` | An MM is stopped before arriving | `entity`, `command_id`, `x`, `y`, `at`, `origin` |
| `sensor` | Every `period_ms` per sensor | `sensor` (id), `entity` (host, or the sensor itself if free-standing), `sensor_kind`, `value`, `unit`, `x`, `y` (where the reading was taken: the MM's current position, the machine's centre, or the fixed position) |
| `sensor_set` | A sensor's setpoint or noise is changed | `sensor`, `entity`, `sensor_kind`, `setpoint`, `noise_std`, `previous_setpoint`, `origin` |
| `action` | An MM performs an action (silent: nothing else changes) | `entity`, `action`, `command_id` (shared with moves), `x`, `y`, `at`, `origin` |
| `sim` | Started, paused, resumed or reset | `state`, `scenario` |

`at` is the machine id when the point is within `dock_tolerance_m` of that machine's dock, otherwise `null`. So
"mm1 moves from machine1 to machine2, then picks there" is:

```json
{"v":1,"seq":41,"ts":1761730004200,"sim_ms":4200,"kind":"move_command","entity":"mm1","command_id":3,"origin":"ui","from":{"x":4.0,"y":8.0,"at":"machine1"},"to":{"x":10.0,"y":8.0,"at":"machine2"},"distance_m":6.0,"eta_ms":7500,"replaces":null}
{"v":1,"seq":42,"ts":1761730004300,"sim_ms":4300,"kind":"position","entity":"mm1","x":4.08,"y":8.0,"heading_rad":0.0,"speed_mps":0.8}
{"v":1,"seq":43,"ts":1761730004500,"sim_ms":4500,"kind":"sensor","sensor":"mm1.vibration","entity":"mm1","sensor_kind":"vibration","value":1.0,"unit":"g","x":4.4,"y":8.0}
{"v":1,"seq":88,"ts":1761730012300,"sim_ms":12300,"kind":"action","entity":"mm1","action":"pick","command_id":7,"x":10.0,"y":8.0,"at":"machine2","origin":"ui"}
```

The exact strings are pinned by golden tests in [sim/src/event.rs](sim/src/event.rs).

Schema version rule:
- **Adding** a kind or a field keeps `v`. Consumers should ignore what they don't know.
- **Renaming, removing or retyping** a field bumps `v`.

Notes for Liminal pipelines:
- Liminal's `tcp_input` makes each whole frame the message payload and keeps no topic. Route on the top-level fields,
  for example a `rule` with `condition = { field_path = "kind", operation = "equals", value = "sensor" }`.
- Measurements (`x`, `y`, `value`, …) are always JSON floats. Liminal's `equals` is strict (`1` ≠ `1.0`), so compare
  numbers with `>`, `<`, `>=`, `<=`.
- `ts` works directly as `timing.event_time_field = "ts"`.
- `sensor_set`, `move_command` and `action` are ground truth: what the operator (or a script) did and when. That's
  useful for labelling, and it means a recording contains its own script.
- Route actions with two rules: `kind` equals `"action"`, then `action` equals `"pick"`.

## Extending

- **New sink** (e.g. TCP):
  1. Add a `SinkKind` variant in [sim/src/scenario.rs](sim/src/scenario.rs).
  2. Implement `EventSink` in `sim/src/sinks/`.
  3. Construct it in `build_sink`.
  4. Mirror it in `SinkDef` in [src/types.ts](src/types.ts) and give it an icon in the toolbar.
- **New MM action** (e.g. `weld`): no code needed. Add `[actions.weld]` to the scenario, and optionally list it in an
  MM's `actions`.
- **New command:**
  1. Add a `SimCommand` variant.
  2. Handle it in `Engine::apply`, emitting an event if it changes the world.
  3. Mirror it in `types.ts`.
- **New event kind:**
  1. Add an `EventBody` variant with a golden test.
  2. Update the `types.ts` union, `EVENT_KINDS`, `KIND_LABELS` / `describeEvent` in [src/format.ts](src/format.ts),
     and the `--kind-*` colour in `styles.css`.
- **Obstacle avoidance:** implement `PathPlanner` and pass it with `Engine::with_planner`.

## Todos

1. **Feed Liminal over TCP.**
   - Add a `tcp` sink: a server that broadcasts each event as a frame (4-byte big-endian length + JSON, written in one
     `write_all`).
   - Add an example Liminal config using `tcp_input` (mode = client) that routes by `kind`.
2. **Automated scenes.** A scene file is a list of timed (later event-triggered) commands, using the same shape the
   engine already accepts:
   ```toml
   [[steps]]
   at_ms = 0
   command = { type = "set_sensor", sensor = "mm1.vibration", value = 0.05 }
   [[steps]]
   at_ms = 0
   command = { type = "move_to", mm = "mm1", target = { machine = "machine2" } }
   [[steps]]
   at_ms = 12000
   command = { type = "perform", mm = "mm1", action = "pick" }
   ```
   A scene player is just another command source (`origin = "scene"`). It could also run headless on `shopfloor-sim`.
3. **Later:**
   - obstacle-avoiding planner;
   - lidar computed from the layout;
   - in-app layout editing;
   - MQTT sink.
