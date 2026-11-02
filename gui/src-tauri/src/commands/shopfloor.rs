//! Shopfloor support for the canvas: the layout of a `shopfloor` input's scenario, so the floor
//! can be drawn before the pipeline runs. While it runs, live state arrives as `processor_view`
//! runtime events and commands go through `send_stage_command`.

use serde::Serialize;
use shopfloor_sim::{Engine, Scenario, WorldState};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::paths::repo_root;

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

pub(crate) fn load_layout(path: &Path) -> Result<ShopfloorLayout, String> {
    let scenario = shopfloor_sim::load_scenario(path).map_err(|error| format!("{error:#}"))?;
    let state = Engine::new(Arc::new(scenario.clone())).state();
    Ok(ShopfloorLayout { scenario, state })
}
