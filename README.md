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

## Shopfloor Simulator

`shopfloor/` holds a separate desktop app (Tauri + React, with its own Rust engine crate) that models an industrial
shopfloor. In it you can drive mobile manipulators between machines and set sensor values live. It emits a JSON event
stream (positions, move commands, sensor readings) designed to feed Liminal. See [shopfloor/README.md](shopfloor/README.md).

```bash
cd shopfloor
npm install
npm run tauri dev
```

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
