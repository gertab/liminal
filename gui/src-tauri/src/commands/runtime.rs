use liminal::config::{load_config, Config};
use liminal::core::runtime_observer::RUNTIME_EVENT_PREFIX;
use liminal::processors::create_processor;
use serde::Serialize;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{Emitter, State};

use crate::paths::{repo_root, resolve_config_path};

#[derive(Clone, Default)]
pub(crate) struct PipelineRuntime {
    running: Arc<StdMutex<Option<RunningPipeline>>>,
}

struct RunningPipeline {
    process_id: u32,
    /// Held open while the pipeline runs. Liminal runs with `--exit-with-parent`, so if the GUI
    /// dies without stopping it (killed, crashed, relaunched by `tauri dev`), the OS closes this pipe
    /// and the pipeline stops too.
    _stdin: Option<ChildStdin>,
}

impl PipelineRuntime {
    /// Stops the running pipeline, if there is one.
    pub(crate) fn stop(&self) -> Result<(), String> {
        let running = self
            .running
            .lock()
            .map_err(|_| "Pipeline runtime lock is poisoned".to_string())?
            .take();

        let Some(running) = running else {
            return Ok(());
        };

        terminate_process(running.process_id)
    }
}

struct RuntimeCommand {
    command: Command,
    launcher: String,
    /// A copy of the binary made for this run, deleted when the run ends.
    copied_binary: Option<PathBuf>,
}

#[derive(Clone, Serialize)]
struct PipelineLogEvent {
    stream: String,
    line: String,
    emitted_at_ms: u64,
}

#[derive(Clone, Serialize)]
struct PipelineStateEvent {
    state: String,
    message: Option<String>,
    emitted_at_ms: u64,
}

#[derive(Clone, Serialize)]
struct PipelineRuntimeEvent {
    event: serde_json::Value,
    emitted_at_ms: u64,
}

#[tauri::command]
pub(crate) fn start_pipeline(
    window: tauri::Window,
    runtime: State<'_, PipelineRuntime>,
    path: String,
) -> Result<(), String> {
    let resolved_path = resolve_config_path(&path)?;
    let config = load_config(&resolved_path)
        .map_err(|error| format!("Failed to load '{}': {}", resolved_path.display(), error))?;
    liminal::config::validate_config(&config)
        .map_err(|error| format!("Configuration error: {}", error))?;
    preflight_processors(&config)?;

    {
        let running = runtime
            .running
            .lock()
            .map_err(|_| "Pipeline runtime lock is poisoned".to_string())?;
        if running.is_some() {
            return Err("A pipeline is already running.".to_string());
        }
    }

    let RuntimeCommand {
        mut command,
        launcher,
        copied_binary,
    } = pipeline_command(&resolved_path)?;
    emit_pipeline_log(
        &window,
        "system",
        format!(
            "Runtime launcher: {}; config: {}",
            launcher,
            resolved_path.display()
        ),
    );

    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("Failed to start pipeline: {}", error))?;

    let process_id = child.id();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    {
        let mut running = runtime
            .running
            .lock()
            .map_err(|_| "Pipeline runtime lock is poisoned".to_string())?;
        *running = Some(RunningPipeline {
            process_id,
            _stdin: child.stdin.take(),
        });
    }

    emit_pipeline_state(
        &window,
        "running",
        Some(format!("Started process {}", process_id)),
    );

    if let Some(stdout) = stdout {
        spawn_pipeline_log_reader(window.clone(), "stdout", stdout);
    }

    if let Some(stderr) = stderr {
        spawn_pipeline_log_reader(window.clone(), "stderr", stderr);
    }

    let runtime_running = runtime.running.clone();
    std::thread::spawn(move || {
        let exit_result = child.wait();
        if let Ok(mut running) = runtime_running.lock() {
            if running.as_ref().map(|running| running.process_id) == Some(process_id) {
                *running = None;
            }
        }
        if let Some(copied_binary) = copied_binary {
            let _ = fs::remove_file(copied_binary);
        }

        let message = match exit_result {
            Ok(status) => Some(format!("Pipeline exited with status {}", status)),
            Err(error) => Some(format!("Failed while waiting for pipeline: {}", error)),
        };
        emit_pipeline_state(&window, "stopped", message);
    });

    Ok(())
}

pub(crate) fn preflight_processors(config: &Config) -> Result<(), String> {
    for (name, stage) in &config.inputs {
        create_processor(&stage.r#type, stage.clone()).map_err(|error| {
            format!(
                "Runtime preflight failed for input '{}': processor '{}' could not be created: {}",
                name, stage.r#type, error
            )
        })?;
    }

    for (pipeline_name, pipeline) in &config.pipelines {
        for (stage_name, stage) in &pipeline.stages {
            create_processor(&stage.r#type, stage.clone()).map_err(|error| {
                format!(
                    "Runtime preflight failed for stage '{}.{}': processor '{}' could not be created: {}",
                    pipeline_name, stage_name, stage.r#type, error
                )
            })?;
        }
    }

    for (name, stage) in &config.outputs {
        create_processor(&stage.r#type, stage.clone()).map_err(|error| {
            format!(
                "Runtime preflight failed for output '{}': processor '{}' could not be created: {}",
                name, stage.r#type, error
            )
        })?;
    }

    Ok(())
}

#[tauri::command]
pub(crate) fn stop_pipeline(runtime: State<'_, PipelineRuntime>) -> Result<(), String> {
    runtime.stop()
}

#[tauri::command]
pub(crate) fn pipeline_runtime_state(
    runtime: State<'_, PipelineRuntime>,
) -> Result<String, String> {
    let running = runtime
        .running
        .lock()
        .map_err(|_| "Pipeline runtime lock is poisoned".to_string())?;
    Ok(if running.is_some() {
        "running".to_string()
    } else {
        "idle".to_string()
    })
}

fn pipeline_command(config_path: &Path) -> Result<RuntimeCommand, String> {
    let mut copied_binary = None;
    let (mut command, launcher) = if let Ok(binary_path) = std::env::var("LIMINAL_BIN") {
        let command = Command::new(&binary_path);
        (command, format!("LIMINAL_BIN ({binary_path})"))
    } else if should_launch_runtime_with_cargo() {
        let runtime_binary = prepare_cargo_runtime_binary()?;
        let command = Command::new(&runtime_binary);
        let launcher = format!("cargo-built binary ({})", runtime_binary.display());
        copied_binary = Some(runtime_binary);
        (command, launcher)
    } else {
        let candidate = repo_root()
            .join("target")
            .join("debug")
            .join(format!("liminal{}", std::env::consts::EXE_SUFFIX));

        if candidate.exists() {
            let command = Command::new(&candidate);
            (command, format!("debug binary ({})", candidate.display()))
        } else {
            return Err(format!(
                "No pipeline runtime binary found at '{}' and cargo is not available",
                candidate.display()
            ));
        }
    };

    command
        .arg("--config")
        .arg(config_path)
        .arg("--runtime-events")
        .arg("jsonl")
        .arg("--exit-with-parent");
    command.current_dir(repo_root());
    Ok(RuntimeCommand {
        command,
        launcher,
        copied_binary,
    })
}

fn should_launch_runtime_with_cargo() -> bool {
    repo_root().join("Cargo.toml").is_file()
        && Command::new("cargo")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
}

fn prepare_cargo_runtime_binary() -> Result<PathBuf, String> {
    let build_target_dir = repo_root().join("target").join("gui-runtime-build");
    let output = Command::new("cargo")
        .arg("build")
        .arg("--quiet")
        .arg("--manifest-path")
        .arg(repo_root().join("Cargo.toml"))
        .arg("--target-dir")
        .arg(&build_target_dir)
        .current_dir(repo_root())
        .output()
        .map_err(|error| format!("Failed to build pipeline runtime: {error}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(format!(
            "Pipeline runtime build failed: {}{}",
            stderr.trim(),
            if stdout.trim().is_empty() {
                String::new()
            } else {
                format!("\n{}", stdout.trim())
            }
        ));
    }

    let built_binary = build_target_dir
        .join("debug")
        .join(format!("liminal{}", std::env::consts::EXE_SUFFIX));
    if !built_binary.is_file() {
        return Err(format!(
            "Pipeline runtime build did not produce '{}'",
            built_binary.display()
        ));
    }

    let run_dir = repo_root().join("target").join("gui-runtime-runs");
    fs::create_dir_all(&run_dir)
        .map_err(|error| format!("Failed to create '{}': {}", run_dir.display(), error))?;
    remove_stale_runtime_binaries(&run_dir);
    let runtime_binary = run_dir.join(format!(
        "liminal-{}-{}{}",
        std::process::id(),
        runtime_binary_suffix(),
        std::env::consts::EXE_SUFFIX
    ));

    fs::copy(&built_binary, &runtime_binary).map_err(|error| {
        format!(
            "Failed to copy runtime binary from '{}' to '{}': {}",
            built_binary.display(),
            runtime_binary.display(),
            error
        )
    })?;

    Ok(runtime_binary)
}

/// Removes copies left behind by runs whose GUI died before they ended. Only one pipeline runs at a
/// time, so none of them is this GUI's. Removing a binary that is still running is fine on Unix; on
/// Windows it fails, and the copy is removed on a later run.
fn remove_stale_runtime_binaries(run_dir: &Path) {
    let Ok(entries) = fs::read_dir(run_dir) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().starts_with("liminal-") {
            let _ = fs::remove_file(entry.path());
        }
    }
}

fn runtime_binary_suffix() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0)
}

fn spawn_pipeline_log_reader<R>(window: tauri::Window, stream: &str, reader: R)
where
    R: std::io::Read + Send + 'static,
{
    let stream = stream.to_string();
    std::thread::spawn(move || {
        for line in BufReader::new(reader).lines() {
            let Ok(line) = line else {
                break;
            };

            if let Some(json) = line.strip_prefix(RUNTIME_EVENT_PREFIX) {
                match serde_json::from_str::<serde_json::Value>(json) {
                    Ok(event) => {
                        let _ = window.emit(
                            "pipeline://runtime-event",
                            PipelineRuntimeEvent {
                                event,
                                emitted_at_ms: now_ms(),
                            },
                        );
                    }
                    Err(error) => {
                        emit_pipeline_log(
                            &window,
                            "system",
                            format!("Failed to parse runtime event: {}", error),
                        );
                    }
                }
                continue;
            }

            let _ = window.emit(
                "pipeline://log",
                PipelineLogEvent {
                    stream: stream.clone(),
                    line,
                    emitted_at_ms: now_ms(),
                },
            );
        }
    });
}

fn emit_pipeline_log(window: &tauri::Window, stream: &str, line: String) {
    let _ = window.emit(
        "pipeline://log",
        PipelineLogEvent {
            stream: stream.to_string(),
            line,
            emitted_at_ms: now_ms(),
        },
    );
}

fn emit_pipeline_state(window: &tauri::Window, state: &str, message: Option<String>) {
    let _ = window.emit(
        "pipeline://state",
        PipelineStateEvent {
            state: state.to_string(),
            message,
            emitted_at_ms: now_ms(),
        },
    );
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().try_into().unwrap_or(u64::MAX))
        .unwrap_or(0)
}

fn terminate_process(process_id: u32) -> Result<(), String> {
    #[cfg(windows)]
    {
        let status = Command::new("taskkill")
            .arg("/PID")
            .arg(process_id.to_string())
            .arg("/T")
            .arg("/F")
            .status()
            .map_err(|error| format!("Failed to stop pipeline: {}", error))?;

        if !status.success() {
            return Err(format!("Failed to stop pipeline process {}", process_id));
        }
    }

    #[cfg(not(windows))]
    {
        let status = Command::new("kill")
            .arg("-TERM")
            .arg(process_id.to_string())
            .status()
            .map_err(|error| format!("Failed to stop pipeline: {}", error))?;

        if !status.success() {
            return Err(format!("Failed to stop pipeline process {}", process_id));
        }
    }

    Ok(())
}
