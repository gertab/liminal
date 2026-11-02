#![allow(dead_code)]

use clap::{Parser, ValueEnum};

mod config;
mod core;
mod logging;
mod processors;

/// Liminal - A framework for building data processing pipelines
#[derive(Parser)]
#[command(name = "liminal")]
#[command(author = "Keith Bugeja <keith.bugeja@um.edu.mt>")]
#[command(version = "0.2.0")]
#[command(about = "Liminal: A Zero-Code Stream Processing Engine for Sensor Data")]
#[command(
    long_about = "------------------------------------------------------------
    ██╗     ██╗███╗   ███╗██╗███╗   ██╗ █████╗ ██╗     
    ██║     ██║████╗ ████║██║████╗  ██║██╔══██╗██║     
    ██║     ██║██╔████╔██║██║██╔██╗ ██║███████║██║     
    ██║     ██║██║╚██╔╝██║██║██║╚██╗██║██╔══██║██║     
    ███████╗██║██║ ╚═╝ ██║██║██║ ╚████║██║  ██║███████╗
    ╚══════╝╚═╝╚═╝     ╚═╝╚═╝╚═╝  ╚═══╝╚═╝  ╚═╝╚══════╝                                                        

    Stream processing engine for sensor data. Build real-
    time pipelines using TOML configuration files.
------------------------------------------------------------"
)]
struct Cli {
    /// Configuration file path
    #[arg(short, long, default_value = "./config/config.toml")]
    config: String,

    /// Log level (trace, debug, info, warn, error)
    #[arg(short, long, default_value = "info")]
    log_level: String,

    /// List available processor types
    #[arg(short = 'L', long)]
    list_processors: bool,

    /// Print the resolved pipeline graph as JSON and exit
    #[arg(long)]
    graph_json: bool,

    /// Emit structured runtime events for integrations.
    #[arg(long, value_enum, default_value_t = RuntimeEventsMode::Off)]
    runtime_events: RuntimeEventsMode,

    /// Accept commands for running stages, one JSON line each:
    /// {"stage": "<name>", "command": {...}} (used by the GUI to drive interactive inputs).
    #[arg(long, value_enum, default_value_t = CommandsMode::Off)]
    commands: CommandsMode,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum CommandsMode {
    /// Do not read commands.
    Off,

    /// Read command lines from stdin.
    Stdin,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum RuntimeEventsMode {
    /// Do not emit structured runtime events.
    Off,

    /// Emit JSON runtime events to stderr with a stable line prefix.
    Jsonl,
}

#[tokio::main(flavor = "multi_thread", worker_threads = 32)]
async fn main() {
    // Parse command line arguments
    let cli = Cli::parse();

    // Initialize logging with specified level
    logging::init_logging(&cli.log_level);

    // Handle list processors command
    if cli.list_processors {
        println!("Available processor types:");
        let processors = processors::factory::list_processors();
        for processor in processors {
            println!("  - {}", processor);
        }
        return;
    }

    // Load configuration from specified file
    let config = match config::load_config(&cli.config) {
        Ok(cfg) => cfg,
        Err(e) => {
            tracing::error!("Failed to load config from '{}': {}", cli.config, e);
            std::process::exit(1);
        }
    };

    if cli.graph_json {
        let graph = config::ResolvedPipelineGraph::from_config(&config);
        match serde_json::to_string_pretty(&graph) {
            Ok(json) => println!("{}", json),
            Err(e) => {
                tracing::error!("Failed to serialise graph JSON: {}", e);
                std::process::exit(1);
            }
        }
        return;
    }

    // Validate configuration
    if let Err(e) = config::validate_config(&config) {
        tracing::error!("Configuration error: {e}");
        std::process::exit(1);
    }

    // Configuration loaded and validated
    tracing::info!("Configuration loaded and validated successfully.");
    let runtime_observer = match cli.runtime_events {
        RuntimeEventsMode::Off => core::runtime_observer::noop_runtime_observer(),
        RuntimeEventsMode::Jsonl => core::runtime_observer::jsonl_runtime_observer(),
    };

    // Initialize the pipeline manager
    tracing::info!("Initialising pipeline manager...");
    let pipeline_manager = match core::pipeline::PipelineManager::new(config)
        .with_runtime_observer(runtime_observer)
        .build_all()
    {
        Ok(manager) => manager,
        Err(error) => {
            tracing::error!("Pipeline build failed: {}", error);
            std::process::exit(1);
        }
    };

    let pipeline_manager = match pipeline_manager.connect_stages().await {
        Ok(manager) => manager,
        Err(error) => {
            tracing::error!("Pipeline connection failed: {}", error);
            std::process::exit(1);
        }
    };

    let pipeline_manager = match pipeline_manager.start_all().await {
        Ok(manager) => manager,
        Err(error) => {
            tracing::error!("Pipeline start failed: {}", error);
            std::process::exit(1);
        }
    };

    if cli.commands == CommandsMode::Stdin {
        let router = pipeline_manager.command_router();
        tracing::info!("Reading stage commands from stdin (stages: {:?})", router.stages());
        tokio::spawn(core::commands::read_commands(
            tokio::io::BufReader::new(tokio::io::stdin()),
            router,
        ));
    }

    let _ = pipeline_manager.wait_for_all().await;

    // Pipeline terminated
    tracing::info!("All input sources have been processed.");
}
