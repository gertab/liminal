use liminal::config::{Config, ResolvedPipelineGraph};
use std::fs;
use toml_edit::DocumentMut;

use crate::paths::resolve_config_path;
use crate::toml_editing::*;
#[tauri::command]
pub(crate) fn update_node_parameter_draft(
    content: String,
    node_id: String,
    parameter_key: String,
    value: String,
) -> Result<DraftEditResult, String> {
    let mut document = document_from_content(&content)?;
    update_existing_parameter(&mut document, &node_id, &parameter_key, &value)?;
    draft_result_from_document(document)
}

#[tauri::command]
pub(crate) fn update_node_parameter_json_draft(
    content: String,
    node_id: String,
    parameter_key: String,
    value_json: String,
) -> Result<DraftEditResult, String> {
    let value = serde_json::from_str::<serde_json::Value>(&value_json)
        .map_err(|error| format!("Failed to parse parameter JSON: {}", error))?;
    let mut document = document_from_content(&content)?;
    update_json_parameter(&mut document, &node_id, &parameter_key, &value)?;
    draft_result_from_document(document)
}

#[tauri::command]
pub(crate) fn remove_node_parameter_draft(
    content: String,
    node_id: String,
    parameter_key: String,
) -> Result<DraftEditResult, String> {
    let mut document = document_from_content(&content)?;
    remove_parameter(&mut document, &node_id, &parameter_key)?;
    draft_result_from_document(document)
}

#[tauri::command]
pub(crate) fn update_node_field_draft(
    content: String,
    node_id: String,
    field_key: String,
    value: String,
) -> Result<DraftEditResult, String> {
    let mut document = document_from_content(&content)?;
    update_existing_field(&mut document, &node_id, &field_key, &value)?;
    draft_result_from_document(document)
}

#[tauri::command]
pub(crate) fn connect_nodes_draft(
    content: String,
    source_node_id: String,
    target_node_id: String,
) -> Result<DraftEditResult, String> {
    let channel_name = channel_for_connection(&content, &source_node_id, &target_node_id)?;
    let mut document = document_from_content(&content)?;
    add_input_channel(&mut document, &target_node_id, &channel_name)?;
    draft_result_from_document(document)
}

#[tauri::command]
pub(crate) fn disconnect_edge_draft(
    content: String,
    target_node_id: String,
    channel_name: String,
) -> Result<DraftEditResult, String> {
    if target_node_id.starts_with("input:") {
        return Err("Input stages do not have input channel lists".to_string());
    }

    let mut document = document_from_content(&content)?;
    remove_input_channel(&mut document, &target_node_id, &channel_name)?;
    draft_result_from_document(document)
}

#[tauri::command]
pub(crate) fn add_node_draft(
    content: String,
    processor_type: String,
    node_name: String,
    pipeline_name: Option<String>,
) -> Result<DraftEditResult, String> {
    let descriptor = descriptor_for_type(&processor_type)?;
    let node_name = node_name.trim();
    validate_node_name(node_name)?;

    let mut document = document_from_content(&content)?;
    add_node_to_document(
        &mut document,
        &descriptor,
        node_name,
        pipeline_name.as_deref(),
    )?;
    draft_result_from_document(document)
}

#[tauri::command]
pub(crate) fn delete_node_draft(
    content: String,
    node_id: String,
) -> Result<DraftEditResult, String> {
    let graph = graph_from_content(&content, "deleting node")?;
    let output_channel = graph
        .nodes
        .iter()
        .find(|node| node.id == node_id)
        .ok_or_else(|| format!("Node '{}' not found", node_id))?
        .output_channel
        .clone();
    let mut document = document_from_content(&content)?;

    delete_node_from_document(&mut document, &node_id)?;
    if let Some(channel_name) = output_channel {
        remove_channel_from_all_inputs(&mut document, &channel_name)?;
    }

    draft_result_from_document(document)
}

#[tauri::command]
pub(crate) fn update_node_parameter(
    path: String,
    node_id: String,
    parameter_key: String,
    value: String,
) -> Result<ResolvedPipelineGraph, String> {
    let resolved_path = resolve_config_path(&path)?;
    let content = fs::read_to_string(&resolved_path)
        .map_err(|error| format!("Failed to read '{}': {}", resolved_path.display(), error))?;
    let mut document = content
        .parse::<DocumentMut>()
        .map_err(|error| format!("Failed to parse TOML for editing: {}", error))?;

    update_existing_parameter(&mut document, &node_id, &parameter_key, &value)?;
    let next_content = document.to_string();
    let config = toml::from_str(&next_content)
        .map_err(|error| format!("Edited TOML no longer parses: {}", error))?;

    fs::write(&resolved_path, next_content)
        .map_err(|error| format!("Failed to write '{}': {}", resolved_path.display(), error))?;

    Ok(ResolvedPipelineGraph::from_config(&config))
}

#[tauri::command]
pub(crate) fn update_node_parameter_json(
    path: String,
    node_id: String,
    parameter_key: String,
    value_json: String,
) -> Result<ResolvedPipelineGraph, String> {
    let resolved_path = resolve_config_path(&path)?;
    let value = serde_json::from_str::<serde_json::Value>(&value_json)
        .map_err(|error| format!("Failed to parse parameter JSON: {}", error))?;
    let content = fs::read_to_string(&resolved_path)
        .map_err(|error| format!("Failed to read '{}': {}", resolved_path.display(), error))?;
    let mut document = content
        .parse::<DocumentMut>()
        .map_err(|error| format!("Failed to parse TOML for editing: {}", error))?;

    update_json_parameter(&mut document, &node_id, &parameter_key, &value)?;
    let next_content = document.to_string();
    let config = toml::from_str(&next_content)
        .map_err(|error| format!("Edited TOML no longer parses: {}", error))?;

    fs::write(&resolved_path, next_content)
        .map_err(|error| format!("Failed to write '{}': {}", resolved_path.display(), error))?;

    Ok(ResolvedPipelineGraph::from_config(&config))
}

#[tauri::command]
pub(crate) fn connect_nodes(
    path: String,
    source_node_id: String,
    target_node_id: String,
) -> Result<ResolvedPipelineGraph, String> {
    let resolved_path = resolve_config_path(&path)?;
    let content = fs::read_to_string(&resolved_path)
        .map_err(|error| format!("Failed to read '{}': {}", resolved_path.display(), error))?;
    let channel_name = channel_for_connection(&content, &source_node_id, &target_node_id)?;
    let mut document = content
        .parse::<DocumentMut>()
        .map_err(|error| format!("Failed to parse TOML for editing: {}", error))?;

    add_input_channel(&mut document, &target_node_id, &channel_name)?;
    write_document_and_resolve(&resolved_path, document)
}

#[tauri::command]
pub(crate) fn disconnect_edge(
    path: String,
    target_node_id: String,
    channel_name: String,
) -> Result<ResolvedPipelineGraph, String> {
    if target_node_id.starts_with("input:") {
        return Err("Input stages do not have input channel lists".to_string());
    }

    let resolved_path = resolve_config_path(&path)?;
    let content = fs::read_to_string(&resolved_path)
        .map_err(|error| format!("Failed to read '{}': {}", resolved_path.display(), error))?;
    let mut document = content
        .parse::<DocumentMut>()
        .map_err(|error| format!("Failed to parse TOML for editing: {}", error))?;

    remove_input_channel(&mut document, &target_node_id, &channel_name)?;
    write_document_and_resolve(&resolved_path, document)
}

#[tauri::command]
pub(crate) fn delete_node(path: String, node_id: String) -> Result<ResolvedPipelineGraph, String> {
    let resolved_path = resolve_config_path(&path)?;
    let content = fs::read_to_string(&resolved_path)
        .map_err(|error| format!("Failed to read '{}': {}", resolved_path.display(), error))?;
    let config = toml::from_str::<Config>(&content)
        .map_err(|error| format!("Failed to parse config before deleting node: {}", error))?;
    let graph = ResolvedPipelineGraph::from_config(&config);
    let output_channel = graph
        .nodes
        .iter()
        .find(|node| node.id == node_id)
        .ok_or_else(|| format!("Node '{}' not found", node_id))?
        .output_channel
        .clone();
    let mut document = content
        .parse::<DocumentMut>()
        .map_err(|error| format!("Failed to parse TOML for editing: {}", error))?;

    delete_node_from_document(&mut document, &node_id)?;
    if let Some(channel_name) = output_channel {
        remove_channel_from_all_inputs(&mut document, &channel_name)?;
    }

    write_document_and_resolve(&resolved_path, document)
}

#[tauri::command]
pub(crate) fn add_node(
    path: String,
    processor_type: String,
    node_name: String,
    pipeline_name: Option<String>,
) -> Result<ResolvedPipelineGraph, String> {
    let resolved_path = resolve_config_path(&path)?;
    let descriptor = descriptor_for_type(&processor_type)?;
    let node_name = node_name.trim();

    validate_node_name(node_name)?;

    let content = fs::read_to_string(&resolved_path)
        .map_err(|error| format!("Failed to read '{}': {}", resolved_path.display(), error))?;
    let mut document = content
        .parse::<DocumentMut>()
        .map_err(|error| format!("Failed to parse TOML for editing: {}", error))?;

    add_node_to_document(
        &mut document,
        &descriptor,
        node_name,
        pipeline_name.as_deref(),
    )?;
    write_document_and_resolve(&resolved_path, document)
}

#[tauri::command]
pub(crate) fn update_node_field(
    path: String,
    node_id: String,
    field_key: String,
    value: String,
) -> Result<ResolvedPipelineGraph, String> {
    let resolved_path = resolve_config_path(&path)?;
    let content = fs::read_to_string(&resolved_path)
        .map_err(|error| format!("Failed to read '{}': {}", resolved_path.display(), error))?;
    let mut document = content
        .parse::<DocumentMut>()
        .map_err(|error| format!("Failed to parse TOML for editing: {}", error))?;

    update_existing_field(&mut document, &node_id, &field_key, &value)?;
    let next_content = document.to_string();
    let config = toml::from_str(&next_content)
        .map_err(|error| format!("Edited TOML no longer parses: {}", error))?;

    fs::write(&resolved_path, next_content)
        .map_err(|error| format!("Failed to write '{}': {}", resolved_path.display(), error))?;

    Ok(ResolvedPipelineGraph::from_config(&config))
}
