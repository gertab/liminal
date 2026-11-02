//! Shopfloor support for the canvas: the layout of a `shopfloor` input's scenario, so the floor
//! can be drawn before the pipeline runs. While it runs, live state arrives as `processor_view`
//! runtime events and commands go through `send_stage_command`.

use serde::Serialize;
use shopfloor_sim::{Engine, Scenario, WorldState};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri_plugin_dialog::DialogExt;

use crate::paths::{collect_toml_files, dialog_path_to_string, repo_root, wait_for_dialog_path};

/// Where scenario files live: `config/shopfloor/`, next to the pipeline configs.
pub(crate) fn scenarios_dir() -> PathBuf {
    repo_root().join("config").join("shopfloor")
}

#[derive(Serialize)]
pub(crate) struct ShopfloorLayout {
    scenario: Scenario,
    /// The world before anything happens (start positions, default sensor values).
    state: WorldState,
}

/// Loads a scenario file; relative paths are resolved like the pipeline does (from the repo root,
/// its working directory when the GUI runs it).
#[tauri::command]
pub(crate) fn load_shopfloor_scenario(path: String) -> Result<ShopfloorLayout, String> {
    load_layout(&resolve_scenario_path(&path))
}

pub(crate) fn resolve_scenario_path(path: &str) -> PathBuf {
    let path = Path::new(path);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        repo_root().join(path)
    }
}

/// Scenario files in `config/shopfloor/`, as the repo-relative paths the `scenario` parameter takes.
#[tauri::command]
pub(crate) fn list_shopfloor_scenarios() -> Result<Vec<String>, String> {
    let dir = scenarios_dir();
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut scenarios = Vec::new();
    collect_toml_files(&dir, &mut scenarios)?;
    scenarios.sort();
    Ok(scenarios)
}

/// File picker for a scenario, starting in `config/shopfloor/`. Returns the path to put in the
/// `scenario` parameter: repo-relative when the file is inside the repo, absolute otherwise.
#[tauri::command]
pub(crate) async fn pick_shopfloor_scenario(window: tauri::Window) -> Result<Option<String>, String> {
    let (sender, receiver) = std::sync::mpsc::channel();
    window
        .dialog()
        .file()
        .set_parent(&window)
        .set_title("Choose Shopfloor Scenario")
        .set_directory(scenarios_dir())
        .add_filter("Shopfloor scenario (TOML)", &["toml"])
        .pick_file(move |selected| {
            let _ = sender.send(selected.map(dialog_path_to_string).transpose());
        });
    Ok(wait_for_dialog_path(receiver)
        .await?
        .map(|path| scenario_parameter_path(Path::new(&path))))
}

/// Repo-relative if `path` is inside the repo (comparing canonical paths, so symlinked folders
/// such as Google Drive's still match), otherwise the path as given.
pub(crate) fn scenario_parameter_path(path: &Path) -> String {
    let root = repo_root();
    let canonical_root = root.canonicalize().unwrap_or(root);
    let canonical_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    match canonical_path.strip_prefix(&canonical_root) {
        Ok(relative) => relative.to_string_lossy().replace('\\', "/"),
        Err(_) => path.to_string_lossy().to_string(),
    }
}

pub(crate) fn load_layout(path: &Path) -> Result<ShopfloorLayout, String> {
    let scenario = shopfloor_sim::load_scenario(path).map_err(|error| format!("{error:#}"))?;
    let state = Engine::new(Arc::new(scenario.clone())).state();
    Ok(ShopfloorLayout { scenario, state })
}
