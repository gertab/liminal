# Liminal

```
    ██╗     ██╗███╗   ███╗██╗███╗   ██╗ █████╗ ██╗     
    ██║     ██║████╗ ████║██║████╗  ██║██╔══██╗██║     
    ██║     ██║██╔████╔██║██║██╔██╗ ██║███████║██║     
    ██║     ██║██║╚██╔╝██║██║██║╚██╗██║██╔══██║██║     
    ███████╗██║██║ ╚═╝ ██║██║██║ ╚████║██║  ██║███████╗
    ╚══════╝╚═╝╚═╝     ╚═╝╚═╝╚═╝  ╚═══╝╚═╝  ╚═╝╚══════╝                     
```

A high-performance, configurable data processing pipeline framework written in Rust. Liminal enables real-time data transformation, filtering, and routing through a modular stage-based architecture that you configure with TOML files.

## Key Features

- **Declarative pipeline configuration**: Define complex data processing workflows in TOML without writing code
- **Pluggable processor architecture**: Easy to write custom input sources, transforms, and output sinks
- **Comprehensive timing semantics**: Event time, watermarks, sequence tracking, deadlines for real-time processing
- **Multiple channel types**: Choose communication patterns (broadcast, direct, shared, fanout) with configurable backpressure
- **Cross-language integration**: TCP protocol with length-prefixed JSON for connecting external systems

## Quick Start

### Prerequisites

- Rust 1.70 or later
- (Optional) Docker for MQTT broker testing

### Installation

```bash
git clone https://github.com/keithbugeja/liminal.git
cd liminal
cargo build --release
```

### Running Your First Pipeline

1. **Run with the default configuration**:
```bash
cargo run -- --config config/config.toml
```

2. **See the output**: The default config demonstrates several features:
   - Simulated temperature data with timing semantics
   - Rule-based processing (LED control based on temperature thresholds)
   - Multiple outputs: console logging, MQTT publishing, and TCP integration
   - Processing of MQTT sensor data (if available)

3. **Test TCP integration** (optional):
```bash
# In another terminal, send JSON data to the TCP input
echo '{"test": "data", "value": 42}' | nc 127.0.0.1 9999
```

For MQTT examples, you can optionally start a broker:
```bash
# Using Docker
docker run -it -p 1883:1883 eclipse-mosquitto
```

## Architecture

Liminal uses a message-passing architecture where:

- **Messages** carry data with source, topic, payload, and comprehensive timing metadata (event time, ingestion time, sequence IDs, watermarks)
- **Channels** provide communication between stages with four types: broadcast, direct (point-to-point), shared (MPMC), and fanout
- **Processors** transform data and forward to output channels using configurable concurrency
- **Pipelines** compose processors into data processing workflows
- **Configuration** defines the complete system declaratively with timing constraints

### Built-in Processors

**Input Processors:**
- **`simulated`**: Generate test data (normal, uniform distributions)
- **`mqtt_sub`**: Subscribe to MQTT topics
- **`tcp_input`**: Receive JSON over TCP with length-prefixed protocol (compatible with Erlang `{packet, 4}`)

**Transform Processors:**
- **`rule`**: Conditional logic and field transformations with mathematical expressions

**Aggregator Processors:**
- **`fusion`**: Combine messages from several inputs into one
- **`window`**: Aggregate the messages of a time span, or the last N messages, per key (count, avg, min, max, std, …)
- **`reduce`**: Keep a state per key, updated by expressions with every message (totals, odometers, alarms, …)
- **`watchdog`**: Alert when an input, or a key within it (e.g. a sensor), has been silent too long

**Output Processors:**
- **`console`**: Display messages to stdout
- **`file`**: Write messages to files with configurable formats
- **`mqtt_pub`**: Publish messages to MQTT topics
- **`tcp_output`**: Send JSON over TCP with length-prefixed protocol

This design enables high-throughput, low-latency processing with clear separation of concerns.

## Configuration

Liminal uses TOML configuration files to define processing pipelines. The main configuration file is `config/config.toml`.

### Basic Structure

```toml
# Input sources
[inputs.input_name]
type = "processor_type"        # simulated, mqtt_sub, tcp_input
output = "channel_name"
concurrency = { type = "thread" }  
channel = { type = "broadcast", capacity = 256 }

[inputs.input_name.parameters]
# processor-specific parameters

# Processing pipelines
[pipelines.pipeline_name]
description = "Pipeline description"

[pipelines.pipeline_name.stages.stage_name]
type = "rule"                  # currently only rule processor available
inputs = ["input_channel"]
output = "output_channel"
concurrency = { type = "thread" }  
channel = { type = "broadcast", capacity = 256 }

[pipelines.pipeline_name.stages.stage_name.parameters]
# processor-specific parameters

# Output sinks (no timing config - they just sink data)
[outputs.output_name]
type = "processor_type"        # console, file, mqtt_pub, tcp_output
inputs = ["input_channel"]

[outputs.output_name.parameters]
# processor-specific parameters
```

### Channel Types

Choose communication patterns between processing stages:
- **`broadcast`** (default): Fast fan-out, slow consumers may miss messages
- **`direct`**: Point-to-point with backpressure, single consumer
- **`shared`**: Multi-consumer load balancing, each message to one consumer
- **`fanout`**: Each consumer gets copy of every message with backpressure

### Rule Actions

The rule processor supports conditional transformations:

Field paths use dot-separated object keys and array indexes, such as
`device.id` or `readings[0].value`.

```toml
[[pipelines.stage_name.parameters.rules]]
condition = { field_path = "temperature", operation = ">", value = 25.0 }
actions = [
    { type = "set_field", field_path = "status", value = "hot" },
    { type = "compute_field", field_path = "temp_f", expression = "temperature * 9/5 + 32" },
    { type = "copy_field", source_field = "device", target_field = "sensor_id" },
    { type = "rename_field", old_field = "temp", new_field = "temperature" },
    { type = "remove_field", field_path = "debug_info" },
    { type = "keep_only_fields", field_paths = ["temperature", "status"] },
    { type = "pass_through" }
]
else_actions = [
    { type = "drop_message" }
]
```

Available actions: `set_field`, `compute_field`, `copy_field`, `rename_field`, `remove_field`, `keep_only_fields`, `pass_through`, `drop_message`

### Windows

A `window` stage collects messages over a time span, or the last N messages, and outputs aggregates over them.
With `group_by`, each key (for example each sensor) gets its own windows.

```toml
[pipelines.health.stages.vibration_stats]
type = "window"
inputs = ["vibration"]
output = "vibration_stats"

[pipelines.health.stages.vibration_stats.parameters]
kind = "tumbling"                 # tumbling | sliding | last_n
size_ms = 10000
group_by = ["sensor"]
aggregates = [
  { function = "avg", field = "value", as = "avg" },
  { function = "std", field = "value", as = "std" },
  { function = "count", as = "readings" },
]
```

Each result is one message: the `group_by` fields, `window_start` and `window_end` (epoch ms), and each aggregate,
e.g. `{"sensor": "s1", "window_start": 1762790000000, "window_end": 1762790010000, "avg": 0.10, "std": 0.01, "readings": 20}`.

| `kind` | Window | `emit = "on_close"` (default) | `emit = "every_message"` |
|---|---|---|---|
| `tumbling` (`size_ms`) | back-to-back windows, aligned to the clock | once per window | the current window, with every message |
| `sliding` (`size_ms`, `slide_ms`) | the last `size_ms` | every `slide_ms` | the last `size_ms`, with every message (no `slide_ms`) |
| `last_n` (`size_n`) | the last N messages | every N messages | the last N, with every message |

- **Aggregates:** `count`, `distinct_count`, `sum`, `avg`, `min`, `max`, `spread` (max − min), `std` (population),
  and `first`/`last` (the field's earliest and latest value in the window, of any type). `count` counts the window's
  messages; the others need a `field`, and skip messages where it's missing (the numeric ones also skip values that
  aren't numbers). `as` names the result; the default is `<field>_<function>` (or `count`).
- **Time:** time windows use each message's event time (see [Timing Semantics](#timing-semantics)): when the input
  received it, or the payload field set by the input's `event_time_field`. A window is output once the stage's
  watermark passes its end: the wall clock minus the stage's `timing.max_lateness_ms` (250 ms without a `timing`
  block). With a `punctuated` or `heuristic` `watermark_strategy`, windows follow the messages' own times instead,
  e.g. when replaying a recording. Messages too late for any window still open are dropped.
- **Empty windows** output nothing, unless `emit_empty = true` (`count` 0, and `null` for the other aggregates).
- `max_items` (default 100000) caps the messages kept per key. Unknown parameters are errors.
- **In the GUI,** the inspector offers only the parameters that apply to the chosen `kind` and `emit` (e.g. `size_n`
  for `last_n`), marks left-over ones as not used, with a button to remove them, and edits `aggregates` as a list.

See [config/examples/config_window.toml](config/examples/config_window.toml).

### Reduce

A `reduce` stage keeps a small state and outputs it with every message. Each message updates the state through
expressions. It covers what builds up over the whole stream, or since an event, rather than over a window: totals,
an odometer, a running maximum, a smoothed value, an alarm that stays on until the value drops back.

```toml
# Positions and actions of mobile machines: the distance each one drove, and its top speed, since its last place.
[pipelines.fleet.stages.odometer]
type = "reduce"
inputs = ["moves"]
output = "distance"

[pipelines.fleet.stages.odometer.parameters]
group_by = ["entity"]
initial = { distance_m = 0.0, top_speed = 0.0 }
update = [
  { field = "distance_m", expression = "state.distance_m + math::hypot(x - prev.x, y - prev.y)",
    when = { field_path = "kind", operation = "==", value = "position" } },
  { field = "top_speed", expression = "max(state.top_speed, speed_mps)",
    when = { field_path = "kind", operation = "==", value = "position" } },
]
reset_when = { field_path = "action", operation = "==", value = "place" }
```

Each output is the `group_by` fields and the state, e.g. `{"entity": "mm1", "distance_m": 20.11, "top_speed": 1.2}`.

- **`initial`** is the state at the start, and after a reset: plain names with a number, text or true/false each.
- **`update`** sets state values with expressions (the same expressions as `compute_field`), optionally only `when` a
  condition holds (the same conditions as rules). A message's updates all read the state from before the message, so
  their order doesn't matter. Results can be numbers, true/false or text. An integer state value stays an integer
  while only integers are added to it (`initial = { n = 0 }` and `state.n + 1`).
- **Expressions and conditions read** the message's fields as they are (`x`, `kind`), plus:
  - `state.<name>`: the state before this message.
  - `prev.<field>`: the previous message of the same key, whatever its kind. For the first message, it is the
    message itself, so a difference such as `x - prev.x` is 0. If some messages lack the fields an expression reads
    from `prev`, drop them with a `rule` before the stage (above, both positions and actions have `x` and `y`).
  - `count`: the messages so far (of the key), this one included: `count == 1` on the first.
  - `dt`: the seconds since the previous message, in event time (see [Timing Semantics](#timing-semantics)).
  - `msg.<field>`: the message, for a field these names hide (e.g. a message field called `state`).
- **`reset_when`** puts the state back to `initial` before the updates. The message that resets counts as the first
  again.
- **Errors:** an update that fails (e.g. a field the message lacks) keeps its old value, with a warning. A mistake in
  an expression's syntax, an update of a value not in `initial`, or an unknown parameter is an error when the config
  loads.
- `include_message = true` also outputs the message's fields, with the state laid over them. The output keeps the
  message's event time.
- **In the GUI,** `initial` is edited as name/value rows, `update` as a list with an optional condition per update,
  and `reset_when` as a condition.

See [config/examples/config_reduce.toml](config/examples/config_reduce.toml).

### Watchdog

A `watchdog` stage outputs an alert when a sensor (or any key) has been silent for longer than its timeout, and,
with `emit_recovered`, a message when it reports again. Nothing else in a pipeline notices missing data: a dead sensor
just stops appearing.

```toml
[pipelines.health.stages.sensors_alive]
type = "watchdog"
inputs = ["sensor_readings"]
output = "silence_alerts"

[pipelines.health.stages.sensors_alive.parameters]
group_by = ["sensor"]
timeout_ms = 1500
timeouts = { "machine2.temperature" = 3000 }        # sensors may have different rates
expected_keys = ["mm1.vibration", "mm1.lidar", "machine2.temperature"]
emit_recovered = true
```

Alerts look like this:

```json
{"sensor": "mm1.vibration", "status": "silent", "timeout_ms": 1500, "silent_ms": 1500, "last_seen": 1762790000000}
{"sensor": "mm1.vibration", "status": "recovered", "timeout_ms": 1500, "silent_ms": 10500, "last_seen": 1762790010500}
```

- **Keys** are the values of the `group_by` field: here each sensor has its own timer. Without `group_by`, each
  input of the stage is watched instead, with `"input": "<channel>"` in the alerts.
- **`timeout_ms`** applies to every key; **`timeouts`** gives a key its own (by its `group_by` value, or by input name
  without `group_by`).
- **`expected_keys`** are watched from the start, so a sensor that never reports is caught too (its alert has no
  `last_seen`). Without `group_by`, every input is watched from the start.
- **One alert per silence.** `silent_ms` is how long the key had been silent when the alert was raised; in a
  `recovered` message, how long the gap lasted. `last_seen` is when the key's last message came (epoch ms).
- **Time:** live, silence is the time since a key's last message *arrived*. With a `punctuated` or `heuristic`
  `watermark_strategy` in the stage's `timing` block, it's measured in event time instead, so a recording can be
  checked at any speed, e.g. `watermark_strategy = { type = "punctuated", field = "ts" }` with `max_lateness_ms = 0`
  (set it: Liminal's default lateness is 30 s). Event time only moves with messages: it catches a sensor that goes
  quiet while others report, not a stream that stops altogether.
- **In the GUI,** `timeouts` is edited as name/value rows and `expected_keys` as a list.

See [config/examples/config_watchdog.toml](config/examples/config_watchdog.toml).

## Advanced Features

### Timing Semantics

Configure timing on input sources and transform stages for real-time processing:

```toml
[inputs.sensor_data.timing]
event_time_field = "timestamp"          # Extract event time from payload
watermark_strategy = { type = "periodic", interval_ms = 1000 }
max_lateness_ms = 5000                   # Drop messages older than 5s
processing_timeout_ms = 10000            # Processing deadline
jitter_bounds_ms = 100                   # Acceptable timing variation
metrics_enabled = true                   # Collect timing metrics
```

Features:
- **Event Time vs Ingestion Time**: Track when events occurred vs when received
- **Watermarks**: Handle out-of-order data (periodic, punctuated, heuristic strategies)
- **Processing Deadlines**: Drop messages that exceed time bounds
- **Sequence Tracking**: Automatic message ordering
- **Jitter Control**: Manage timing variations for real-time guarantees

## Examples

The `config/examples/` directory contains working examples:

- `config_rule_test.toml` - Basic rule processing with device metadata and sensor math
- `config_led_control_with_else.toml` - LED control using else_actions for binary states
- `config_advanced_rule.toml` - Complex filtering and transformation chains

Run any example with:
```bash
cargo run -- --config config/examples/config_rule_test.toml
```

### Scenarios

The `config/shopfloor/config_scenario_*.toml` configs each solve one monitoring problem from a factory floor with the
`window`, `reduce` and `watchdog` stages. Their events come from the
[shopfloor simulator](https://github.com/gertab/shopfloor-sim)'s `proposed-liminal-scenarios.toml`, whose scenes
produce each situation:

```bash
cargo run -- --config config/shopfloor/config_scenario_noise_exposure.toml                # start Liminal first
cargo run --bin shopfloor -- run scenarios/proposed-liminal-scenarios.toml --scene shift  # in shopfloor-sim
```

The `shift` scene plays every episode in 2 minutes, and each episode has its own scene of 20-40 s. Periods are
shortened so the examples run quickly. For example, the 8-hour shift takes 2 minutes, 15 min becomes 15 s, and the 60 s
for a place after a pick becomes 15 s. Each config says which values to change for real time.

| Config | Problem | How |
|---|---|---|
| `noise_exposure` | Daily noise exposure against the EU limits (80/85/87 dB(A) as `LEX,8h`) | energy average per period (window), exposure so far and projected (reduce) |
| `temperature_rate` | At most 5 °C in any 15 min (deflasher) or 1.1 °C (hall, ASHRAE 55) | sliding windows: `spread` against a per-sensor limit |
| `adaptive_threshold` | A vibration unusual for that sensor (over its average + 3σ) | sliding window per sensor: `avg`, `std`, `last` |
| `deflasher_settled` | Place only once the temperature has been within ±0.3 °C for 2 min | sliding window `spread`, then a reduce that flags early places |
| `pick_place` | Every pick followed by a place within 60 s (15 s in the demo) | reduce per MM, timed by its telemetry |
| `utilisation` | Share of the last minutes each MM spent moving, performing or idle | reduce (time per mode), then a sliding `sum` |
| `downsampling` | One row per sensor per second for a dashboard | tumbling window: `min`, `max`, `avg`, `last` |
| `odometer` | Distance per MM, a service due every 1 km | reduce per MM |
| `stocker` | Stocker fill level: full, over-full, empty, a pick from empty | reduce |
| `lost_events` | Gaps or disorder in the event numbers | reduce on the raw stream |

## Extending Liminal

Liminal is designed to be extensible. You can add new processors by implementing the required traits and registering them with the factory.

### Adding a Custom Processor

1. **Create a processor config struct** implementing `ProcessorConfig`:

```rust
use crate::config::{
    FieldConfig, ProcessorConfig, StageConfig, 
    extract_field_params, extract_param
};

#[derive(Debug)]
struct MyProcessorConfig {
    bias: f64,
    field: FieldConfig,
    timing: Option<crate::config::TimingConfig>,
}

impl ProcessorConfig for MyProcessorConfig {
    fn from_stage_config(config: &StageConfig) -> anyhow::Result<Self> {
        // Extract scalar parameters
        let bias = extract_param(&config.parameters, "bias", 0.0);
        
        // Extract field configuration
        let field_config = extract_field_params(&config.parameters);
        
        // Validate field config is appropriate for this processor
        match &field_config {
            FieldConfig::Single { .. } | FieldConfig::Multiple { .. } => {
                // This processor supports these field patterns
            },
            _ => return Err(anyhow::anyhow!("MyProcessor requires field mapping configuration")),
        }
        
        // Extract timing configuration
        let timing_config = config.timing.clone();
        
        let config = Self { 
            bias, 
            field: field_config,
            timing: timing_config,
        };
        config.validate()?;
        Ok(config)
    }
    
    fn validate(&self) -> anyhow::Result<()> {
        self.field.validate()?;
        Ok(())
    }
}
```

2. **Implement the Processor trait**:

```rust
use async_trait::async_trait;
use crate::processors::Processor;
use crate::core::context::ProcessingContext;

pub struct MyProcessor {
    config: MyProcessorConfig,
    // Add TimingMixin if needed for timing semantics
    timing: TimingMixin,
}

impl MyProcessor {
    pub fn new(name: &str, stage_config: StageConfig) -> anyhow::Result<Box<dyn Processor>> {
        let config = MyProcessorConfig::from_stage_config(&stage_config)?;
        let timing = TimingMixin::new(stage_config.timing.as_ref());
        
        Ok(Box::new(Self { config, timing }))
    }
}

#[async_trait]
impl Processor for MyProcessor {
    async fn init(&mut self) -> anyhow::Result<()> {
        tracing::info!("Initialising MyProcessor");
        Ok(())
    }

    async fn process(&mut self, context: &mut ProcessingContext) -> anyhow::Result<()> {
        // Process messages from input channels and send to output
        // Use self.config for parameters
        // Use self.timing for timing operations if needed
        Ok(())
    }
}
```

3. **Register with the factory** in `src/processors/factory.rs`:

```rust
register_processor("my_processor", Box::new(MyProcessor::new));
```

4. **Use in configuration**:

```toml
# Single field transformation
[inputs.my_input]
type = "my_processor"
output = "processed_data"

[inputs.my_input.parameters]
bias = 2.0
field_in = "input_field"
field_out = "output_field"

# Multiple field transformations
[inputs.my_input_multi]
type = "my_processor"
output = "processed_data"

[inputs.my_input_multi.parameters]
bias = 2.0
fields_in = ["temp", "humidity"]
fields_out = ["adjusted_temp", "adjusted_humidity"]

# Output-only (for input processors)
[inputs.my_generator]
type = "my_processor"
output = "generated_data"

[inputs.my_generator.parameters]
bias = 2.0
field_out = "generated_value"
```

### Processors with Timing Semantics

For processors that need timing features, implement `WithTimingMixin`:

```rust
use crate::core::timing_mixin::{TimingMixin, WithTimingMixin};

impl WithTimingMixin for MyProcessor {
    fn timing_mixin(&self) -> &TimingMixin {
        &self.timing
    }
    
    fn timing_mixin_mut(&mut self) -> &mut TimingMixin {
        &mut self.timing
    }
}
```

Then use timing features in your `process()` method:

```rust
// Capture event time once to ensure consistency
let event_time = SystemTime::now();

// Get sequence ID and create message with timing
let sequence_id = self.timing.next_sequence_id();
let message = self.timing.create_message_with_event_time_extraction(
    &self.name,
    "output_topic",
    new_payload,
    event_time  // Use the captured event_time consistently
).with_sequence_id(sequence_id);

// Check if messages should be dropped due to timing constraints
if self.timing.should_drop_message(&message) {
    continue; // Skip processing
}
```

## Liminal GUI

![Liminal GUI](docs/images/liminal-gui.png)

Liminal also includes a desktop GUI for designing, inspecting, editing, and running
pipeline configurations. The GUI is built with Tauri and React, while the pipeline
model, TOML editing, validation, and runtime integration are handled by Rust.

The GUI keeps TOML as the source of truth. It can open existing configuration files,
create new empty TOML configs, edit processor parameters, add and remove nodes, rewire
channels, save or save-as without unnecessary TOML churn, and inspect graph diagnostics
visually.

### Running the GUI

From the repository root:

```bash
cd gui
npm install
npm run tauri dev
```

For a production build:

```bash
cd gui
npm run tauri build
```

When launched from the source tree, the GUI starts the Liminal runtime with
`cargo run` in `target/gui-runtime/` so runtime changes are picked up without
depending on a stale `target/debug` binary. You can set `LIMINAL_BIN` to point
the GUI at a specific Liminal executable.

## Citation

If you use this framework in academic work (papers, theses, or technical reports), please cite it using the following reference.

```bibtex
@software{bugeja_liminal_2026,
  author       = {Keith Bugeja},
  title        = {Liminal: A Modular Framework for Real-Time Data Processing Pipelines},
  year         = {2026},
  publisher    = {Zenodo},
  version      = {v0.2.0-alpha},
  doi          = {10.5281/zenodo.18256609},
  url          = {https://doi.org/10.5281/zenodo.18256609}
}
```
