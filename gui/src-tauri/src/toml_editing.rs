use liminal::config::{Config, ResolvedPipelineGraph};
use liminal::processors::descriptor::{
    processor_descriptors, FieldKind, FieldSpec, ProcessorCategory, ProcessorDescriptor,
};
use serde::Serialize;
use std::fs;
use std::path::Path;
use toml_edit::{Array, DocumentMut, Item, Table, Value};

#[derive(Serialize)]
pub(crate) struct DraftEditResult {
    pub(crate) graph: ResolvedPipelineGraph,
    pub(crate) content: String,
}
pub(crate) fn write_document_and_resolve(
    resolved_path: &Path,
    document: DocumentMut,
) -> Result<ResolvedPipelineGraph, String> {
    let next_content = document.to_string();
    let config = toml::from_str(&next_content)
        .map_err(|error| format!("Edited TOML no longer parses: {}", error))?;

    fs::write(resolved_path, next_content)
        .map_err(|error| format!("Failed to write '{}': {}", resolved_path.display(), error))?;

    Ok(ResolvedPipelineGraph::from_config(&config))
}

pub(crate) fn document_from_content(content: &str) -> Result<DocumentMut, String> {
    content
        .parse::<DocumentMut>()
        .map_err(|error| format!("Failed to parse TOML for editing: {}", error))
}

pub(crate) fn draft_result_from_document(document: DocumentMut) -> Result<DraftEditResult, String> {
    let content = document.to_string();
    let graph = graph_from_content(&content, "editing draft")?;

    Ok(DraftEditResult { graph, content })
}

pub(crate) fn graph_from_content(
    content: &str,
    action: &str,
) -> Result<ResolvedPipelineGraph, String> {
    let config = toml::from_str::<Config>(content)
        .map_err(|error| format!("Failed to parse config before {}: {}", action, error))?;

    Ok(ResolvedPipelineGraph::from_config(&config))
}

pub(crate) fn descriptor_for_type(processor_type: &str) -> Result<ProcessorDescriptor, String> {
    processor_descriptors()
        .into_iter()
        .find(|descriptor| descriptor.type_name == processor_type)
        .ok_or_else(|| format!("Unknown processor type '{}'", processor_type))
}

pub(crate) fn channel_for_connection(
    content: &str,
    source_node_id: &str,
    target_node_id: &str,
) -> Result<String, String> {
    if source_node_id == target_node_id {
        return Err("A node cannot be connected to itself".to_string());
    }

    if target_node_id.starts_with("input:") {
        return Err("Input stages cannot consume channels".to_string());
    }

    let graph = graph_from_content(content, "rewiring")?;
    let source_node = graph
        .nodes
        .iter()
        .find(|node| node.id == source_node_id)
        .ok_or_else(|| format!("Source node '{}' not found", source_node_id))?;
    let target_node = graph
        .nodes
        .iter()
        .find(|node| node.id == target_node_id)
        .ok_or_else(|| format!("Target node '{}' not found", target_node_id))?;
    let channel_name = source_node.output_channel.as_ref().ok_or_else(|| {
        format!(
            "Source node '{}' does not produce an output channel",
            source_node.display_name
        )
    })?;

    if target_node
        .input_channels
        .iter()
        .any(|input| input == channel_name)
    {
        return Err(format!(
            "Target node '{}' already consumes channel '{}'",
            target_node.display_name, channel_name
        ));
    }

    Ok(channel_name.clone())
}

pub(crate) fn update_existing_parameter(
    document: &mut DocumentMut,
    node_id: &str,
    parameter_key: &str,
    value: &str,
) -> Result<(), String> {
    let stage = stage_item_mut(document, node_id)?;
    let parameters = stage
        .get_mut("parameters")
        .ok_or_else(|| format!("Node '{}' has no parameters table", node_id))?;

    if let Some(table) = parameters.as_table_mut() {
        let existing = table
            .get_mut(parameter_key)
            .ok_or_else(|| format!("Parameter '{}' does not exist", parameter_key))?;
        set_existing_scalar(existing, value)
    } else if let Some(inline_table) = parameters.as_inline_table_mut() {
        let existing = inline_table
            .get_mut(parameter_key)
            .ok_or_else(|| format!("Parameter '{}' does not exist", parameter_key))?;
        set_existing_scalar_value(existing, value)
    } else {
        Err(format!(
            "Node '{}' parameters are not editable as a table",
            node_id
        ))
    }
}

/// Removes a parameter, so the processor falls back to its default (or doesn't use it).
pub(crate) fn remove_parameter(
    document: &mut DocumentMut,
    node_id: &str,
    parameter_key: &str,
) -> Result<(), String> {
    let stage = stage_item_mut(document, node_id)?;
    let parameters = stage
        .get_mut("parameters")
        .ok_or_else(|| format!("Node '{}' has no parameters table", node_id))?;
    if parameters.get(parameter_key).is_none() {
        return Err(format!("Parameter '{}' does not exist", parameter_key));
    }
    remove_nested_field(parameters, parameter_key);
    Ok(())
}

pub(crate) fn update_json_parameter(
    document: &mut DocumentMut,
    node_id: &str,
    parameter_key: &str,
    value: &serde_json::Value,
) -> Result<(), String> {
    let stage = stage_item_mut(document, node_id)?;

    if stage.get("parameters").is_none() {
        stage["parameters"] = toml_edit::value(toml_edit::InlineTable::new());
    }

    let parameters = stage
        .get_mut("parameters")
        .ok_or_else(|| format!("Node '{}' has no editable parameters table", node_id))?;
    let next_item = toml_item_from_json(value)?;

    if let Some(table) = parameters.as_table_mut() {
        table[parameter_key] = next_item;
        Ok(())
    } else if parameters.is_inline_table() {
        if !json_value_needs_table(value) {
            let next_value = next_item
                .clone()
                .into_value()
                .map_err(|_| "Inline parameters cannot store this nested value".to_string())?;
            let inline_table = parameters.as_inline_table_mut().ok_or_else(|| {
                format!("Node '{}' parameters are not editable as a table", node_id)
            })?;
            inline_table.insert(parameter_key, next_value);
            Ok(())
        } else {
            let existing_inline = parameters
                .as_inline_table()
                .ok_or_else(|| {
                    format!("Node '{}' parameters are not editable as a table", node_id)
                })?
                .clone();
            let mut table = Table::new();
            for (key, value) in existing_inline.iter() {
                table[key] = Item::Value(value.clone());
            }
            table[parameter_key] = next_item;
            *parameters = Item::Table(table);
            Ok(())
        }
    } else {
        Err(format!(
            "Node '{}' parameters are not editable as a table",
            node_id
        ))
    }
}

pub(crate) fn json_value_needs_table(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Array(values) => values.iter().any(|value| {
            matches!(
                value,
                serde_json::Value::Array(_) | serde_json::Value::Object(_)
            )
        }),
        serde_json::Value::Object(_) => true,
        _ => false,
    }
}

pub(crate) fn add_input_channel(
    document: &mut DocumentMut,
    target_node_id: &str,
    channel_name: &str,
) -> Result<(), String> {
    let stage = stage_item_mut(document, target_node_id)?;
    let inputs = inputs_array_mut(stage)?;

    if inputs
        .iter()
        .any(|input| input.as_str() == Some(channel_name))
    {
        return Err(format!("Channel '{}' is already connected", channel_name));
    }

    inputs.push(channel_name);
    Ok(())
}

pub(crate) fn remove_input_channel(
    document: &mut DocumentMut,
    target_node_id: &str,
    channel_name: &str,
) -> Result<(), String> {
    let stage = stage_item_mut(document, target_node_id)?;
    let inputs = inputs_array_mut(stage)?;
    let mut next_inputs = Array::default();
    let mut removed = false;

    for input in inputs.iter() {
        let Some(input_name) = input.as_str() else {
            return Err("Stage inputs must be string channel names".to_string());
        };

        if input_name == channel_name {
            removed = true;
        } else {
            next_inputs.push(input_name);
        }
    }

    if !removed {
        return Err(format!("Channel '{}' is not connected", channel_name));
    }

    *inputs = next_inputs;
    Ok(())
}

pub(crate) fn add_node_to_document(
    document: &mut DocumentMut,
    descriptor: &ProcessorDescriptor,
    node_name: &str,
    pipeline_name: Option<&str>,
) -> Result<(), String> {
    let stage = new_node_table(descriptor, node_name)?;

    match descriptor.category {
        ProcessorCategory::Input => {
            let inputs = ensure_document_table(document, "inputs")?;
            insert_named_node(inputs, node_name, stage, "input")
        }
        ProcessorCategory::Output => {
            let outputs = ensure_document_table(document, "outputs")?;
            insert_named_node(outputs, node_name, stage, "output")
        }
        ProcessorCategory::Transform | ProcessorCategory::Aggregator => {
            let pipeline_name = pipeline_name
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .unwrap_or("default_pipeline");
            validate_node_name(pipeline_name)?;

            let pipelines = ensure_document_table(document, "pipelines")?;
            let pipeline = ensure_child_table(pipelines, pipeline_name)?;
            if pipeline.get("description").is_none() {
                pipeline["description"] = toml_edit::value(format!("{} pipeline", pipeline_name));
            }
            let stages = ensure_child_table(pipeline, "stages")?;
            insert_named_node(stages, node_name, stage, "pipeline stage")
        }
    }
}

pub(crate) fn delete_node_from_document(
    document: &mut DocumentMut,
    node_id: &str,
) -> Result<(), String> {
    if let Some(name) = node_id.strip_prefix("input:") {
        return remove_named_node(document, "inputs", name, "input");
    }

    if let Some(name) = node_id.strip_prefix("output:") {
        return remove_named_node(document, "outputs", name, "output");
    }

    if let Some(rest) = node_id.strip_prefix("pipeline:") {
        let (pipeline_name, stage_name) = rest
            .split_once(".stage:")
            .ok_or_else(|| format!("Invalid pipeline node id: {}", node_id))?;
        let stages = document
            .get_mut("pipelines")
            .and_then(|item| item.get_mut(pipeline_name))
            .and_then(|item| item.get_mut("stages"))
            .ok_or_else(|| {
                format!(
                    "Pipeline stage '{}.{}' not found",
                    pipeline_name, stage_name
                )
            })?;

        return remove_child_from_item(stages, stage_name, "pipeline stage");
    }

    Err(format!("Unsupported node id: {}", node_id))
}

pub(crate) fn remove_named_node(
    document: &mut DocumentMut,
    table_key: &str,
    node_name: &str,
    label: &str,
) -> Result<(), String> {
    let parent = document
        .get_mut(table_key)
        .ok_or_else(|| format!("{} table does not exist", table_key))?;

    remove_child_from_item(parent, node_name, label)
}

pub(crate) fn remove_child_from_item(
    parent: &mut Item,
    node_name: &str,
    label: &str,
) -> Result<(), String> {
    if let Some(table) = parent.as_table_mut() {
        table
            .remove(node_name)
            .map(|_| ())
            .ok_or_else(|| format!("A {} named '{}' does not exist", label, node_name))
    } else {
        Err(format!("{} parent is not editable as a table", label))
    }
}

pub(crate) fn remove_channel_from_all_inputs(
    document: &mut DocumentMut,
    channel_name: &str,
) -> Result<(), String> {
    if let Some(outputs) = document.get_mut("outputs") {
        remove_channel_from_stage_table(outputs, channel_name)?;
    }

    if let Some(pipelines) = document.get_mut("pipelines") {
        let Some(pipelines_table) = pipelines.as_table_mut() else {
            return Err("Pipelines are not editable as a table".to_string());
        };

        for (_, pipeline) in pipelines_table.iter_mut() {
            if let Some(stages) = pipeline.get_mut("stages") {
                remove_channel_from_stage_table(stages, channel_name)?;
            }
        }
    }

    Ok(())
}

pub(crate) fn remove_channel_from_stage_table(
    stages: &mut Item,
    channel_name: &str,
) -> Result<(), String> {
    let Some(stages_table) = stages.as_table_mut() else {
        return Err("Stages are not editable as a table".to_string());
    };

    for (_, stage) in stages_table.iter_mut() {
        remove_input_channel_if_present(stage, channel_name)?;
    }

    Ok(())
}

pub(crate) fn remove_input_channel_if_present(
    stage: &mut Item,
    channel_name: &str,
) -> Result<(), String> {
    let Some(inputs) = stage.get_mut("inputs") else {
        return Ok(());
    };
    let inputs = inputs
        .as_value_mut()
        .and_then(Value::as_array_mut)
        .ok_or_else(|| "Stage inputs are not editable as an array".to_string())?;
    let mut next_inputs = Array::default();

    for input in inputs.iter() {
        let Some(input_name) = input.as_str() else {
            return Err("Stage inputs must be string channel names".to_string());
        };

        if input_name != channel_name {
            next_inputs.push(input_name);
        }
    }

    *inputs = next_inputs;
    Ok(())
}

pub(crate) fn new_node_table(
    descriptor: &ProcessorDescriptor,
    node_name: &str,
) -> Result<Item, String> {
    let mut table = Table::new();
    table["type"] = toml_edit::value(descriptor.type_name.as_str());

    if descriptor.category != ProcessorCategory::Input {
        table["inputs"] = toml_edit::value(Array::default());
    }

    if descriptor.category != ProcessorCategory::Output {
        table["output"] = toml_edit::value(default_channel_name(node_name));
    }

    let mut parameters = toml_edit::InlineTable::new();
    for field in &descriptor.fields {
        if let Some(default_value) = default_value_for_field(field)? {
            parameters.insert(&field.key, default_value);
        }
    }

    if !parameters.is_empty() {
        table["parameters"] = toml_edit::value(parameters);
    }

    Ok(Item::Table(table))
}

pub(crate) fn default_value_for_field(field: &FieldSpec) -> Result<Option<Value>, String> {
    let Some(default_value) = field.default_value.as_deref() else {
        return Ok(None);
    };

    let value = match field.kind {
        FieldKind::String | FieldKind::Enum => Value::from(default_value),
        FieldKind::Integer => Value::from(
            default_value
                .parse::<i64>()
                .map_err(|_| format!("Default for '{}' is not an integer", field.key))?,
        ),
        FieldKind::Number => Value::from(
            default_value
                .parse::<f64>()
                .map_err(|_| format!("Default for '{}' is not a number", field.key))?,
        ),
        FieldKind::Boolean => Value::from(
            default_value
                .parse::<bool>()
                .map_err(|_| format!("Default for '{}' is not a boolean", field.key))?,
        ),
        FieldKind::Array | FieldKind::Object | FieldKind::JsonValue => {
            let item = toml_item_from_default_literal(default_value)?;
            item.into_value()
                .map_err(|_| format!("Default for '{}' is not an inline TOML value", field.key))?
        }
    };

    Ok(Some(value))
}

pub(crate) fn toml_item_from_default_literal(default_value: &str) -> Result<Item, String> {
    let snippet = format!("value = {}", default_value);
    let mut document = snippet.parse::<DocumentMut>().map_err(|error| {
        format!(
            "Failed to parse descriptor default '{}': {}",
            default_value, error
        )
    })?;

    Ok(document.remove("value").unwrap_or(Item::None))
}

pub(crate) fn ensure_document_table<'a>(
    document: &'a mut DocumentMut,
    key: &str,
) -> Result<&'a mut Item, String> {
    if document.get(key).is_none() {
        document[key] = Item::Table(Table::new());
    }

    document
        .get_mut(key)
        .filter(|item| item.as_table().is_some())
        .ok_or_else(|| format!("'{}' is not editable as a table", key))
}

pub(crate) fn ensure_child_table<'a>(
    parent: &'a mut Item,
    key: &str,
) -> Result<&'a mut Item, String> {
    if parent.get(key).is_none() {
        parent[key] = Item::Table(Table::new());
    }

    parent
        .get_mut(key)
        .filter(|item| item.as_table().is_some())
        .ok_or_else(|| format!("'{}' is not editable as a table", key))
}

pub(crate) fn insert_named_node(
    parent: &mut Item,
    node_name: &str,
    stage: Item,
    label: &str,
) -> Result<(), String> {
    if parent.get(node_name).is_some() {
        return Err(format!("A {} named '{}' already exists", label, node_name));
    }

    parent[node_name] = stage;
    Ok(())
}

pub(crate) fn validate_node_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("Node name cannot be empty".to_string());
    }

    if !name
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || character == '_' || character == '-')
    {
        return Err(
            "Node names may only contain letters, numbers, underscores, and hyphens".to_string(),
        );
    }

    Ok(())
}

pub(crate) fn default_channel_name(node_name: &str) -> String {
    format!("{}_data", node_name.replace('-', "_"))
}

pub(crate) fn inputs_array_mut(stage: &mut Item) -> Result<&mut Array, String> {
    if stage.get("inputs").is_none() {
        stage["inputs"] = toml_edit::value(Array::default());
    }

    stage
        .get_mut("inputs")
        .and_then(Item::as_value_mut)
        .and_then(Value::as_array_mut)
        .ok_or_else(|| "Stage inputs are not editable as an array".to_string())
}

pub(crate) fn toml_item_from_json(value: &serde_json::Value) -> Result<Item, String> {
    let snippet = format!("value = {}", toml_literal_from_json(value)?);
    let mut document = snippet
        .parse::<DocumentMut>()
        .map_err(|error| format!("Failed to convert JSON parameter to TOML: {}", error))?;

    Ok(document.remove("value").unwrap_or(Item::None))
}

pub(crate) fn toml_literal_from_json(value: &serde_json::Value) -> Result<String, String> {
    match value {
        serde_json::Value::Null => Err("TOML parameters cannot be null".to_string()),
        serde_json::Value::Bool(value) => Ok(value.to_string()),
        serde_json::Value::Number(value) => Ok(value.to_string()),
        serde_json::Value::String(value) => Ok(Value::from(value.as_str()).to_string()),
        serde_json::Value::Array(values) => values
            .iter()
            .map(toml_literal_from_json)
            .collect::<Result<Vec<_>, _>>()
            .map(|values| format!("[{}]", values.join(", "))),
        serde_json::Value::Object(values) => values
            .iter()
            .map(|(key, value)| {
                Ok(format!(
                    "{} = {}",
                    toml_key(key),
                    toml_literal_from_json(value)?
                ))
            })
            .collect::<Result<Vec<_>, String>>()
            .map(|fields| format!("{{ {} }}", fields.join(", "))),
    }
}

pub(crate) fn toml_key(key: &str) -> String {
    if key
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || character == '_' || character == '-')
    {
        key.to_string()
    } else {
        Value::from(key).to_string()
    }
}

pub(crate) fn update_existing_field(
    document: &mut DocumentMut,
    node_id: &str,
    field_key: &str,
    value: &str,
) -> Result<(), String> {
    let stage = stage_item_mut(document, node_id)?;

    match field_key {
        "output" => {
            let output = stage
                .get_mut("output")
                .ok_or_else(|| format!("Node '{}' has no editable output field", node_id))?;
            set_existing_scalar(output, value)
        }
        "channel.type" => set_channel_field(stage, "type", value),
        "channel.capacity" => set_channel_field(stage, "capacity", value),
        "concurrency.type" => set_concurrency_field(stage, value),
        "timing.event_time_field" => set_timing_field(stage, "event_time_field", value),
        "timing.max_lateness_ms" => set_timing_field(stage, "max_lateness_ms", value),
        "timing.processing_timeout_ms" => set_timing_field(stage, "processing_timeout_ms", value),
        "timing.jitter_bounds_ms" => set_timing_field(stage, "jitter_bounds_ms", value),
        "timing.metrics_enabled" => set_timing_field(stage, "metrics_enabled", value),
        _ => Err(format!("Unsupported editable field: {}", field_key)),
    }
}

pub(crate) fn set_channel_field(
    stage: &mut Item,
    field_key: &str,
    value: &str,
) -> Result<(), String> {
    if stage.get("output").is_none() {
        return Err("Only output-producing stages have channel settings".to_string());
    }

    if stage.get("channel").is_none() {
        stage["channel"] = toml_edit::value(toml_edit::InlineTable::new());
    }

    let channel = stage
        .get_mut("channel")
        .ok_or_else(|| "Channel settings are not editable on this stage".to_string())?;

    if let Some(table) = channel.as_table_mut() {
        match field_key {
            "type" => {
                table["type"] = toml_edit::value(value);
                Ok(())
            }
            "capacity" => {
                let parsed = parse_non_negative_integer(value, "capacity")?;
                table["capacity"] = toml_edit::value(parsed);
                Ok(())
            }
            _ => Err(format!("Unsupported channel field: {}", field_key)),
        }
    } else if let Some(inline_table) = channel.as_inline_table_mut() {
        match field_key {
            "type" => {
                inline_table.insert("type", Value::from(value));
                Ok(())
            }
            "capacity" => {
                let parsed = parse_non_negative_integer(value, "capacity")?;
                inline_table.insert("capacity", Value::from(parsed));
                Ok(())
            }
            _ => Err(format!("Unsupported channel field: {}", field_key)),
        }
    } else {
        Err("Channel settings are not editable as a table".to_string())
    }
}

pub(crate) fn set_concurrency_field(stage: &mut Item, value: &str) -> Result<(), String> {
    ensure_inline_table(stage, "concurrency");
    let concurrency = stage
        .get_mut("concurrency")
        .ok_or_else(|| "Concurrency settings are not editable on this stage".to_string())?;

    set_nested_string(concurrency, "type", value)
}

pub(crate) fn set_timing_field(
    stage: &mut Item,
    field_key: &str,
    value: &str,
) -> Result<(), String> {
    if value.trim().is_empty()
        && matches!(
            field_key,
            "event_time_field" | "processing_timeout_ms" | "jitter_bounds_ms"
        )
    {
        if let Some(timing) = stage.get_mut("timing") {
            remove_nested_field(timing, field_key);
        }
        return Ok(());
    }

    ensure_inline_table(stage, "timing");
    let timing = stage
        .get_mut("timing")
        .ok_or_else(|| "Timing settings are not editable on this stage".to_string())?;

    match field_key {
        "event_time_field" => set_nested_string(timing, field_key, value),
        "max_lateness_ms" | "processing_timeout_ms" | "jitter_bounds_ms" => {
            let parsed = parse_non_negative_integer(value, field_key)?;
            set_nested_integer(timing, field_key, parsed)
        }
        "metrics_enabled" => {
            let parsed = value
                .parse::<bool>()
                .map_err(|_| format!("'{}' is not a boolean", value))?;
            set_nested_bool(timing, field_key, parsed)
        }
        _ => Err(format!("Unsupported timing field: {}", field_key)),
    }
}

pub(crate) fn ensure_inline_table(stage: &mut Item, field_key: &str) {
    if stage.get(field_key).is_none() {
        stage[field_key] = toml_edit::value(toml_edit::InlineTable::new());
    }
}

pub(crate) fn set_nested_string(
    item: &mut Item,
    field_key: &str,
    value: &str,
) -> Result<(), String> {
    if let Some(table) = item.as_table_mut() {
        table[field_key] = toml_edit::value(value);
        return Ok(());
    }

    if let Some(inline_table) = item.as_inline_table_mut() {
        inline_table.insert(field_key, Value::from(value));
        return Ok(());
    }

    Err(format!("'{}' is not editable as a table field", field_key))
}

pub(crate) fn set_nested_integer(
    item: &mut Item,
    field_key: &str,
    value: i64,
) -> Result<(), String> {
    if let Some(table) = item.as_table_mut() {
        table[field_key] = toml_edit::value(value);
        return Ok(());
    }

    if let Some(inline_table) = item.as_inline_table_mut() {
        inline_table.insert(field_key, Value::from(value));
        return Ok(());
    }

    Err(format!("'{}' is not editable as a table field", field_key))
}

pub(crate) fn set_nested_bool(item: &mut Item, field_key: &str, value: bool) -> Result<(), String> {
    if let Some(table) = item.as_table_mut() {
        table[field_key] = toml_edit::value(value);
        return Ok(());
    }

    if let Some(inline_table) = item.as_inline_table_mut() {
        inline_table.insert(field_key, Value::from(value));
        return Ok(());
    }

    Err(format!("'{}' is not editable as a table field", field_key))
}

pub(crate) fn remove_nested_field(item: &mut Item, field_key: &str) {
    if let Some(table) = item.as_table_mut() {
        table.remove(field_key);
    } else if let Some(inline_table) = item.as_inline_table_mut() {
        inline_table.remove(field_key);
    }
}

pub(crate) fn parse_non_negative_integer(value: &str, label: &str) -> Result<i64, String> {
    let parsed = value
        .parse::<i64>()
        .map_err(|_| format!("'{}' is not an integer {}", value, label))?;

    if parsed < 0 {
        return Err(format!("'{}' cannot be negative", label));
    }

    Ok(parsed)
}

pub(crate) fn stage_item_mut<'a>(
    document: &'a mut DocumentMut,
    node_id: &str,
) -> Result<&'a mut Item, String> {
    if let Some(name) = node_id.strip_prefix("input:") {
        return document
            .get_mut("inputs")
            .and_then(|item| item.get_mut(name))
            .ok_or_else(|| format!("Input node '{}' not found", name));
    }

    if let Some(name) = node_id.strip_prefix("output:") {
        return document
            .get_mut("outputs")
            .and_then(|item| item.get_mut(name))
            .ok_or_else(|| format!("Output node '{}' not found", name));
    }

    if let Some(rest) = node_id.strip_prefix("pipeline:") {
        let (pipeline_name, stage_name) = rest
            .split_once(".stage:")
            .ok_or_else(|| format!("Invalid pipeline node id: {}", node_id))?;

        return document
            .get_mut("pipelines")
            .and_then(|item| item.get_mut(pipeline_name))
            .and_then(|item| item.get_mut("stages"))
            .and_then(|item| item.get_mut(stage_name))
            .ok_or_else(|| {
                format!(
                    "Pipeline stage '{}.{}' not found",
                    pipeline_name, stage_name
                )
            });
    }

    Err(format!("Unsupported node id: {}", node_id))
}

pub(crate) fn set_existing_scalar(item: &mut Item, value: &str) -> Result<(), String> {
    let existing = item
        .as_value_mut()
        .ok_or_else(|| "Nested parameters are read-only in this phase".to_string())?;
    set_existing_scalar_value(existing, value)
}

pub(crate) fn set_existing_scalar_value(existing: &mut Value, value: &str) -> Result<(), String> {
    if existing.is_str() {
        *existing = Value::from(value);
        return Ok(());
    }

    if existing.is_bool() {
        let parsed = value
            .parse::<bool>()
            .map_err(|_| format!("'{}' is not a boolean", value))?;
        *existing = Value::from(parsed);
        return Ok(());
    }

    if existing.is_integer() {
        let parsed = value
            .parse::<i64>()
            .map_err(|_| format!("'{}' is not an integer", value))?;
        *existing = Value::from(parsed);
        return Ok(());
    }

    if existing.is_float() {
        let parsed = value
            .parse::<f64>()
            .map_err(|_| format!("'{}' is not a number", value))?;
        *existing = Value::from(parsed);
        return Ok(());
    }

    Err("Nested parameters are read-only in this phase".to_string())
}
