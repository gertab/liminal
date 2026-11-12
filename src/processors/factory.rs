//! Processor Factory Module
//!
//! This module provides a dynamic registry system for creating processors at runtime.
//! It maintains a thread-safe registry of processor constructors that can be looked up
//! by name, enabling extensible processor creation without compile-time dependencies.
//!
//! # Architecture
//!
//! The factory uses a singleton pattern with `OnceLock` to ensure thread-safe initialisation
//! of the processor registry. Default processors are automatically registered on first access,
//! but custom processors can be registered at any time.
//!
//! # Example Usage
//!
//! ```rust
//! use liminal::processors::factory::{create_processor, register_processor};
//! use liminal::config::StageConfig;
//!
//! // Create a built-in processor
//! let config = StageConfig { /* ... */ };
//! let processor = create_processor("rule", config)?;
//!
//! // Register and create a custom processor
//! register_processor("custom", Box::new(MyProcessor::new));
//! let custom = create_processor("custom", config)?;
//! ```
//!
//! # Thread Safety
//!
//! All functions in this module are thread-safe and can be called concurrently
//! from multiple threads without external synchronisation.
//!
//! # Processor Metadata
//!
//! Runtime construction lives here. GUI and documentation metadata lives in
//! `crate::processors::descriptor`, which describes the built-in processors and
//! their editable parameters.

use crate::processors::{
    Processor,
    aggregator::{FusionStage, ReduceStage, WatchdogStage, WindowStage},
    input::{MqttInputProcessor, SimulatedSignalProcessor, TcpInputProcessor},
    output::{
        ConsoleOutputProcessor, FileOutputProcessor, MqttOutputProcessor, TcpOutputProcessor,
    },
    transform::RuleProcessor,
};

use crate::config::StageConfig;

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// Type alias for processor constructor functions.
///
/// A processor constructor takes a name and configuration, and returns either
/// a boxed processor or an error if construction fails. All constructors must
/// be thread-safe (`Send + Sync`).
///
/// # Parameters
/// - `&str`: The name/identifier for the processor instance
/// - `StageConfig`: Configuration parameters for the processor
///
/// # Returns
/// - `anyhow::Result<Box<dyn Processor>>`: The created processor or an error
type ProcessorConstructor =
    Box<dyn Fn(&str, StageConfig) -> anyhow::Result<Box<dyn Processor>> + Send + Sync>;

/// Global registry for processor constructors.
///
/// This static variable holds the singleton registry that maps processor type names
/// to their constructor functions. It uses `OnceLock` for thread-safe lazy initialisation.
static PROCESSOR_REGISTRY: OnceLock<Mutex<HashMap<String, ProcessorConstructor>>> = OnceLock::new();

/// Retrieves the global processor registry, initializing it if necessary.
///
/// This function provides access to the singleton processor registry. The registry
/// is initialized on first access and remains valid for the lifetime of the program.
///
/// # Returns
/// A reference to the mutex-protected registry HashMap.
///
/// # Thread Safety
/// This function is thread-safe and can be called concurrently.
fn get_processor_registry() -> &'static Mutex<HashMap<String, ProcessorConstructor>> {
    PROCESSOR_REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Lists all registered processor type names.
///
/// Returns a vector containing the names of all processor types that are currently
/// registered in the factory. This includes both built-in processors (which are
/// auto-registered on first access) and any custom processors that have been
/// manually registered.
///
/// # Returns
/// A vector of strings containing the names of all registered processors.
///
/// # Example
/// ```rust
/// let processors = list_processors();
/// println!("Available processors: {:?}", processors);
/// ```
pub fn list_processors() -> Vec<String> {
    ensure_default_processors();

    let registry = get_processor_registry().lock().unwrap();
    registry.keys().cloned().collect()
}

/// Checks if a processor with the given type name exists in the registry.
///
/// This function can be used to validate processor type names before attempting
/// to create them, helping to provide better error messages and validation.
///
/// # Arguments
/// * `name` - The processor type name to check for
///
/// # Returns
/// * `true` if a processor with this name is registered, `false` otherwise
///
/// # Example
/// ```rust
/// if processor_exists("rule") {
///     let processor = create_processor("rule", config)?;
/// } else {
///     eprintln!("Rule processor not available");
/// }
/// ```
pub fn processor_exists(name: &str) -> bool {
    ensure_default_processors();

    let registry = get_processor_registry().lock().unwrap();
    registry.contains_key(name)
}

/// Registers a processor constructor with the given type name.
///
/// This function allows runtime registration of new processor types. The constructor
/// function will be stored in the registry and can be used by `create_processor()`.
/// If a processor with the same name already exists, it will be replaced.
///
/// # Arguments
/// * `name` - The unique type name for this processor
/// * `constructor` - A function that creates instances of this processor type
///
/// # Thread Safety
/// This function is thread-safe and can be called from multiple threads.
///
/// # Example
/// ```rust
/// // Register a custom processor
/// register_processor("my_processor", Box::new(|name, config| {
///     MyCustomProcessor::new(name, config)
/// }));
///
/// // Now it can be created like built-in processors
/// let processor = create_processor("my_processor", config)?;
/// ```
pub fn register_processor(name: &str, constructor: ProcessorConstructor) {
    let mut registry = get_processor_registry().lock().unwrap();
    registry.insert(name.to_string(), constructor);
}

/// Ensures that the default built-in processors are registered.
///
/// This function performs one-time initialization of the default processor types.
/// It uses `OnceLock` to guarantee that registration happens exactly once, even
/// in multi-threaded environments. This is called automatically by other factory
/// functions, so manual invocation is typically not necessary.
///
/// # Registered Processors
/// - `"simulated"` - Generates simulated signal data
/// - `"rule"` - Applies conditional transformations and filtering
/// - `"fusion"` - Combines data from multiple inputs
/// - `"window"` - Aggregates the messages of a time span or the last N messages, per key
/// - `"reduce"` - Keeps a state per key, updated by expressions with every message
/// - `"watchdog"` - Alerts when an input, or a key within it, has been silent too long
/// - `"console"` - Outputs received messages to console
/// - `"file"` - Outputs received messages to file
/// - `"mqtt_sub"` - Subscribes to MQTT topics for input
/// - `"mqtt_pub"` - Publishes messages to MQTT topics
///
/// # Thread Safety
/// This function is thread-safe and idempotent - calling it multiple times
/// has the same effect as calling it once.
fn ensure_default_processors() {
    static INITIALIZED: OnceLock<()> = OnceLock::new();
    INITIALIZED.get_or_init(|| {
        register_processor("mqtt_sub", Box::new(MqttInputProcessor::new));
        register_processor("mqtt_pub", Box::new(MqttOutputProcessor::new));
        register_processor("tcp_input", Box::new(TcpInputProcessor::new));
        register_processor("tcp_output", Box::new(TcpOutputProcessor::new));
        register_processor("simulated", Box::new(SimulatedSignalProcessor::new));
        register_processor("rule", Box::new(RuleProcessor::new));
        register_processor("fusion", Box::new(FusionStage::new));
        register_processor("window", Box::new(WindowStage::new));
        register_processor("reduce", Box::new(ReduceStage::new));
        register_processor("watchdog", Box::new(WatchdogStage::new));
        register_processor("console", Box::new(ConsoleOutputProcessor::new));
        register_processor("file", Box::new(FileOutputProcessor::new));

        tracing::info!("Default processors registered!");
    });
}

/// Creates a processor instance of the specified type with the given configuration.
///
/// This is the main entry point for processor creation. It looks up the constructor
/// for the requested processor type and invokes it with the provided configuration.
/// Built-in processors are automatically available, but the registry can be extended
/// with custom processors using `register_processor()`.
///
/// # Arguments
/// * `name` - The processor type name (e.g., "simulated", "rule", "console")
/// * `config` - Configuration parameters for the processor
///
/// # Returns
/// * `Ok(Box<dyn Processor>)` - Successfully created processor
/// * `Err(anyhow::Error)` - Creation failed due to:
///   - Unknown processor type
///   - Invalid configuration parameters
///   - Constructor-specific validation errors
///
/// # Example
/// ```rust
/// use liminal::config::StageConfig;
///
/// let config = StageConfig {
///     r#type: "fusion".to_string(),
///     parameters: Some(HashMap::from([
///         ("mode".to_string(), json!("merge_objects")),
///         ("conflict_strategy".to_string(), json!("prefix")),
///     ])),
///     // ... other fields
/// };
///
/// match create_processor("fusion", config) {
///     Ok(processor) => println!("Processor created successfully"),
///     Err(e) => eprintln!("Failed to create processor: {}", e),
/// }
/// ```
///
/// # Error Handling
/// This function can fail in several ways:
/// - **Unknown processor type**: The requested type is not registered
/// - **Configuration errors**: Invalid or missing required parameters
/// - **Resource errors**: Insufficient resources or system constraints
///
/// # Thread Safety
/// This function is thread-safe and can be called concurrently from multiple threads.
pub fn create_processor(name: &str, config: StageConfig) -> anyhow::Result<Box<dyn Processor>> {
    tracing::info!("Creating processor '{}'", name);

    ensure_default_processors();

    let registry = get_processor_registry().lock().unwrap();

    registry
        .get(name)
        .ok_or_else(|| anyhow::anyhow!("Processor '{}' not found", name))
        .and_then(|constructor| constructor(name, config))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::load_config;

    /// The scenario examples are built from the strict stages (window, reduce, watchdog), so build
    /// every processor they name: a mistake in a parameter or an expression fails here, not at Run.
    #[test]
    fn scenario_examples_can_be_built() {
        let mut built = 0;
        for entry in std::fs::read_dir("config/shopfloor").expect("scenario directory exists") {
            let path = entry.expect("readable entry").path();
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            if !(name.starts_with("config_scenario_") && name.ends_with(".toml")) {
                continue;
            }
            let config = load_config(&path).unwrap_or_else(|error| panic!("{name} loads: {error}"));
            let stages = config.inputs.iter().chain(&config.outputs).chain(
                config
                    .pipelines
                    .values()
                    .flat_map(|pipeline| &pipeline.stages),
            );
            for (stage_name, stage) in stages {
                create_processor(&stage.r#type, stage.clone()).unwrap_or_else(|error| {
                    panic!("{name}: stage '{stage_name}' can't be built: {error}")
                });
            }
            built += 1;
        }
        assert!(built >= 10, "expected the scenario examples, built {built}");
    }
}
