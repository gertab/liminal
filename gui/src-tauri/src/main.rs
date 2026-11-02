mod commands;
mod paths;
mod toml_editing;

use commands::draft::{
    add_node, add_node_draft, connect_nodes, connect_nodes_draft, delete_node, delete_node_draft,
    disconnect_edge, disconnect_edge_draft, update_node_field, update_node_field_draft,
    update_node_parameter, update_node_parameter_draft, update_node_parameter_json,
    update_node_parameter_json_draft,
};
use commands::files::{
    copy_config_to_workspace, list_example_configs, list_processor_descriptors,
    list_workspace_configs, load_config_text, load_graph, pick_config_file, pick_save_config_path,
    pick_workspace_folder, save_config_as, save_config_text,
};
use commands::runtime::{
    pipeline_runtime_state, send_stage_command, start_pipeline, stop_pipeline, PipelineRuntime,
};
use commands::shopfloor::{
    list_shopfloor_scenarios, load_shopfloor_scenario, pick_shopfloor_scenario,
};

#[cfg(test)]
use commands::runtime::preflight_processors;
#[cfg(test)]
use liminal::config::{load_config, Config};
#[cfg(test)]
use liminal::processors::create_processor;
#[cfg(test)]
use liminal::processors::descriptor::processor_descriptors;
#[cfg(test)]
use paths::{collect_toml_files, copy_config_to_workspace_path, writable_config_path};
#[cfg(test)]
use std::fs;
#[cfg(test)]
use std::path::{Path, PathBuf};
#[cfg(test)]
use toml_edit::DocumentMut;
#[cfg(test)]
use toml_editing::*;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(PipelineRuntime::default())
        .invoke_handler(tauri::generate_handler![
            load_graph,
            load_config_text,
            save_config_text,
            save_config_as,
            copy_config_to_workspace,
            start_pipeline,
            stop_pipeline,
            pipeline_runtime_state,
            send_stage_command,
            load_shopfloor_scenario,
            list_shopfloor_scenarios,
            pick_shopfloor_scenario,
            list_workspace_configs,
            pick_config_file,
            pick_workspace_folder,
            pick_save_config_path,
            add_node,
            add_node_draft,
            delete_node,
            delete_node_draft,
            connect_nodes,
            connect_nodes_draft,
            disconnect_edge,
            disconnect_edge_draft,
            list_example_configs,
            list_processor_descriptors,
            update_node_field,
            update_node_field_draft,
            update_node_parameter_json,
            update_node_parameter_json_draft,
            update_node_parameter,
            update_node_parameter_draft
        ])
        .run(tauri::generate_context!())
        .expect("error while running Liminal Pipeline GUI");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn updates_inline_parameter_and_preserves_surrounding_comments() {
        let mut document = parse_document(
            r#"
# sensor source
[inputs.sensor]
type = "simulated"
output = "raw_data"
parameters = { field_out = "value", interval_ms = 1000 } # keep this comment

[outputs.console]
type = "console"
inputs = ["raw_data"]
"#,
        );

        update_existing_parameter(&mut document, "input:sensor", "interval_ms", "2500")
            .expect("inline parameter update succeeds");

        let edited = document.to_string();
        assert!(edited.contains("# sensor source"));
        assert!(edited.contains("# keep this comment"));
        assert!(edited.contains("field_out = \"value\""));
        assert!(edited.contains("interval_ms = 2500"));
    }

    #[test]
    fn updates_table_parameter_without_rewriting_nested_rules() {
        let mut document = parse_document(
            r#"
[pipelines.rules]
description = "Rule pipeline"

[pipelines.rules.stages.filter]
type = "rule"
inputs = ["raw_data"]
output = "filtered_data"

[pipelines.rules.stages.filter.parameters]
error_strategy = "log_and_continue"

[[pipelines.rules.stages.filter.parameters.rules]]
# nested rule stays readable
condition = { field_path = "device_id", operation = "startswith", value = "esp32" }
actions = [{ type = "pass_through" }]
"#,
        );

        update_existing_parameter(
            &mut document,
            "pipeline:rules.stage:filter",
            "error_strategy",
            "drop",
        )
        .expect("table parameter update succeeds");

        let edited = document.to_string();
        assert!(edited.contains("error_strategy = \"drop\""));
        assert!(edited.contains("# nested rule stays readable"));
        assert!(edited.contains("[[pipelines.rules.stages.filter.parameters.rules]]"));
        assert!(edited.contains("actions = [{ type = \"pass_through\" }]"));
    }

    #[test]
    fn updates_inline_channel_type_and_capacity() {
        let mut document = parse_document(
            r#"
[inputs.sensor]
type = "simulated"
output = "raw_data"
channel = { type = "broadcast", capacity = 128 }
"#,
        );

        update_existing_field(&mut document, "input:sensor", "channel.type", "fanout")
            .expect("channel type update succeeds");
        update_existing_field(&mut document, "input:sensor", "channel.capacity", "512")
            .expect("channel capacity update succeeds");

        let edited = document.to_string();
        assert!(edited.contains("type = \"fanout\""));
        assert!(edited.contains("capacity = 512"));
    }

    #[test]
    fn updates_table_channel_type_and_capacity() {
        let mut document = parse_document(
            r#"
[inputs.sensor]
type = "simulated"
output = "raw_data"

[inputs.sensor.channel]
type = "broadcast"
capacity = 128
"#,
        );

        update_existing_field(&mut document, "input:sensor", "channel.type", "direct")
            .expect("channel table type update succeeds");
        update_existing_field(&mut document, "input:sensor", "channel.capacity", "32")
            .expect("channel table capacity update succeeds");

        let edited = document.to_string();
        assert!(edited.contains("[inputs.sensor.channel]"));
        assert!(edited.contains("type = \"direct\""));
        assert!(edited.contains("capacity = 32"));
    }

    #[test]
    fn creates_inline_channel_table_for_output_producing_stage() {
        let mut document = parse_document(
            r#"
[inputs.sensor]
type = "simulated"
output = "raw_data"
"#,
        );

        update_existing_field(&mut document, "input:sensor", "channel.capacity", "256")
            .expect("missing channel table is created");

        let edited = document.to_string();
        assert!(edited.contains("channel = {"));
        assert!(edited.contains("capacity = 256"));
    }

    #[test]
    fn creates_and_updates_concurrency_table() {
        let mut document = parse_document(
            r#"
[inputs.sensor]
type = "simulated"
output = "raw_data"
"#,
        );

        update_existing_field(
            &mut document,
            "input:sensor",
            "concurrency.type",
            "pipeline",
        )
        .expect("concurrency type update succeeds");

        let edited = document.to_string();
        assert!(edited.contains("concurrency = {"));
        assert!(edited.contains("type = \"pipeline\""));
    }

    #[test]
    fn updates_inline_timing_fields() {
        let mut document = parse_document(
            r#"
[inputs.sensor]
type = "simulated"
output = "raw_data"
timing = { event_time_field = "ts", max_lateness_ms = 30000, metrics_enabled = true }
"#,
        );

        update_existing_field(
            &mut document,
            "input:sensor",
            "timing.event_time_field",
            "event_ts",
        )
        .expect("timing event field update succeeds");
        update_existing_field(
            &mut document,
            "input:sensor",
            "timing.max_lateness_ms",
            "15000",
        )
        .expect("timing lateness update succeeds");
        update_existing_field(
            &mut document,
            "input:sensor",
            "timing.metrics_enabled",
            "false",
        )
        .expect("timing boolean update succeeds");

        let edited = document.to_string();
        assert!(edited.contains("event_time_field = \"event_ts\""));
        assert!(edited.contains("max_lateness_ms = 15000"));
        assert!(edited.contains("metrics_enabled = false"));
    }

    #[test]
    fn updates_table_timing_fields_and_clears_optional_values() {
        let mut document = parse_document(
            r#"
[inputs.sensor]
type = "simulated"
output = "raw_data"

[inputs.sensor.timing]
event_time_field = "ts"
processing_timeout_ms = 1000
jitter_bounds_ms = 50
"#,
        );

        update_existing_field(
            &mut document,
            "input:sensor",
            "timing.processing_timeout_ms",
            "2000",
        )
        .expect("timing timeout update succeeds");
        update_existing_field(&mut document, "input:sensor", "timing.event_time_field", "")
            .expect("blank optional timing string clears the field");
        update_existing_field(&mut document, "input:sensor", "timing.jitter_bounds_ms", "")
            .expect("blank optional timing number clears the field");

        let edited = document.to_string();
        assert!(edited.contains("[inputs.sensor.timing]"));
        assert!(edited.contains("processing_timeout_ms = 2000"));
        assert!(!edited.contains("event_time_field"));
        assert!(!edited.contains("jitter_bounds_ms"));
    }

    #[test]
    fn rejects_invalid_timing_values() {
        let mut document = parse_document(
            r#"
[inputs.sensor]
type = "simulated"
output = "raw_data"
"#,
        );

        let number_error = update_existing_field(
            &mut document,
            "input:sensor",
            "timing.max_lateness_ms",
            "-10",
        )
        .expect_err("negative timing value is rejected");
        let bool_error = update_existing_field(
            &mut document,
            "input:sensor",
            "timing.metrics_enabled",
            "maybe",
        )
        .expect_err("invalid timing bool is rejected");

        assert!(number_error.contains("cannot be negative"));
        assert!(bool_error.contains("not a boolean"));
    }

    #[test]
    fn rejects_nested_parameter_edit() {
        let mut document = parse_document(
            r#"
[pipelines.rules]
description = "Rule pipeline"

[pipelines.rules.stages.filter]
type = "rule"
inputs = ["raw_data"]
output = "filtered_data"

[[pipelines.rules.stages.filter.parameters.rules]]
condition = { field_path = "device_id", operation = "exists" }
actions = [{ type = "pass_through" }]
"#,
        );

        let error =
            update_existing_parameter(&mut document, "pipeline:rules.stage:filter", "rules", "[]")
                .expect_err("nested parameter edits are rejected");

        assert!(error.contains("Nested parameters are read-only"));
    }

    #[test]
    fn updates_nested_rules_parameter_from_json() {
        let mut document = parse_document(
            r#"
[pipelines.rules]
description = "Rule pipeline"

[pipelines.rules.stages.filter]
type = "rule"
inputs = ["raw_data"]
output = "filtered_data"

[[pipelines.rules.stages.filter.parameters.rules]]
condition = { field_path = "device_id", operation = "startswith", value = "esp32" }
actions = [{ type = "pass_through" }]
"#,
        );
        let value = serde_json::json!([
            {
                "condition": { "field_path": "temperature", "operation": ">", "value": 15 },
                "actions": [
                    { "type": "set_field", "field_path": "state", "value": true }
                ],
                "else_actions": [
                    { "type": "set_field", "field_path": "state", "value": false }
                ]
            }
        ]);

        update_json_parameter(
            &mut document,
            "pipeline:rules.stage:filter",
            "rules",
            &value,
        )
        .expect("nested rule update succeeds");

        let edited = document.to_string();
        let parsed: serde_json::Value = toml::from_str::<toml::Value>(&edited)
            .expect("edited TOML parses")
            .try_into()
            .expect("edited TOML converts to JSON");

        assert!(edited.contains("temperature"));
        assert!(edited.contains("set_field"));
        assert_eq!(
            parsed["pipelines"]["rules"]["stages"]["filter"]["parameters"]["rules"][0]["condition"]
                ["field_path"],
            "temperature"
        );
    }

    #[test]
    fn adds_nested_rules_parameter_to_inline_defaults() {
        let mut document = parse_document(
            r#"
[pipelines.rules]
description = "Rule pipeline"

[pipelines.rules.stages.filter]
type = "rule"
inputs = ["raw_data"]
output = "filtered_data"
parameters = { error_strategy = "continue" }
"#,
        );
        let value = serde_json::json!([
            {
                "condition": { "field_path": "device_id", "operation": "equals", "value": "imu" },
                "actions": [
                    { "type": "pass_through" }
                ],
                "else_actions": []
            }
        ]);

        update_json_parameter(
            &mut document,
            "pipeline:rules.stage:filter",
            "rules",
            &value,
        )
        .expect("missing nested rule parameter is added");

        let edited = document.to_string();

        let parsed: serde_json::Value = toml::from_str::<toml::Value>(&edited)
            .expect("edited TOML parses")
            .try_into()
            .expect("edited TOML converts to JSON");

        assert!(edited.contains("error_strategy = \"continue\""));
        assert!(edited.contains("device_id"));
        assert_eq!(
            parsed["pipelines"]["rules"]["stages"]["filter"]["parameters"]["rules"][0]["condition"]
                ["field_path"],
            "device_id"
        );
    }

    #[test]
    fn adds_input_channel_when_connecting_nodes() {
        let mut document = parse_document(
            r#"
[inputs.sensor]
type = "simulated"
output = "raw_data"

[outputs.console]
type = "console"
"#,
        );

        add_input_channel(&mut document, "output:console", "raw_data")
            .expect("input channel is added");

        let edited = document.to_string();
        assert!(edited.contains("inputs = [\"raw_data\"]"));
    }

    #[test]
    fn rejects_duplicate_input_channel_connection() {
        let mut document = parse_document(
            r#"
[outputs.console]
type = "console"
inputs = ["raw_data"]
"#,
        );

        let error = add_input_channel(&mut document, "output:console", "raw_data")
            .expect_err("duplicate input channel is rejected");

        assert!(error.contains("already connected"));
    }

    #[test]
    fn removes_input_channel_when_disconnects_edge() {
        let mut document = parse_document(
            r#"
[outputs.console]
type = "console"
inputs = ["raw_data", "filtered_data"]
"#,
        );

        remove_input_channel(&mut document, "output:console", "raw_data")
            .expect("input channel is removed");

        let edited = document.to_string();
        assert!(!edited.contains("\"raw_data\""));
        assert!(edited.contains("\"filtered_data\""));
    }

    #[test]
    fn adds_input_node_with_output_and_descriptor_defaults() {
        let mut document = parse_document(
            r#"
[outputs.console]
type = "console"
"#,
        );
        let descriptor = processor_descriptors()
            .into_iter()
            .find(|descriptor| descriptor.type_name == "simulated")
            .expect("simulated descriptor exists");

        add_node_to_document(&mut document, &descriptor, "new_sensor", None)
            .expect("input node is added");

        let edited = document.to_string();
        assert!(edited.contains("[inputs.new_sensor]"));
        assert!(edited.contains("type = \"simulated\""));
        assert!(edited.contains("output = \"new_sensor_data\""));
        assert!(edited.contains("interval_ms = 1000"));
        assert!(edited.contains("distribution = \"uniform\""));
        assert!(!edited.contains("inputs = []"));
    }

    #[test]
    fn adds_pipeline_stage_to_named_pipeline() {
        let mut document = parse_document(
            r#"
[pipelines.rules]
description = "Rules"
"#,
        );
        let descriptor = processor_descriptors()
            .into_iter()
            .find(|descriptor| descriptor.type_name == "rule")
            .expect("rule descriptor exists");

        add_node_to_document(&mut document, &descriptor, "new_filter", Some("rules"))
            .expect("pipeline stage is added");

        let edited = document.to_string();
        assert!(edited.contains("[pipelines.rules.stages.new_filter]"));
        assert!(edited.contains("type = \"rule\""));
        assert!(edited.contains("inputs = []"));
        assert!(edited.contains("output = \"new_filter_data\""));
        assert!(edited.contains("rules = ["));

        let config = toml::from_str::<Config>(&edited).expect("edited config parses");
        let stage = config
            .pipelines
            .get("rules")
            .and_then(|pipeline| pipeline.stages.get("new_filter"))
            .expect("new rule stage exists")
            .clone();
        create_processor("rule", stage).expect("descriptor-created rule stage can be built");
    }

    #[test]
    fn adds_fusion_aggregator_with_descriptor_defaults() {
        let mut document = parse_document(
            r#"
[pipelines.default_pipeline]
description = "Default"
"#,
        );
        let descriptor = processor_descriptors()
            .into_iter()
            .find(|descriptor| descriptor.type_name == "fusion")
            .expect("fusion descriptor exists");

        add_node_to_document(
            &mut document,
            &descriptor,
            "joiner",
            Some("default_pipeline"),
        )
        .expect("fusion stage is added");

        let edited = document.to_string();
        assert!(edited.contains("[pipelines.default_pipeline.stages.joiner]"));
        assert!(edited.contains("type = \"fusion\""));
        assert!(edited.contains("mode = \"merge_objects\""));
        assert!(edited.contains("conflict_strategy = \"prefix\""));
        assert!(edited.contains("join_window_ms = 25"));

        let config = toml::from_str::<Config>(&edited).expect("edited config parses");
        let stage = config
            .pipelines
            .get("default_pipeline")
            .and_then(|pipeline| pipeline.stages.get("joiner"))
            .expect("new fusion stage exists")
            .clone();
        create_processor("fusion", stage).expect("descriptor-created fusion stage can be built");
    }

    #[test]
    fn popcorn_example_processors_are_constructible() {
        let config = load_config(Path::new("../../config/examples/config_popcorn.toml"))
            .expect("popcorn example loads");

        for (name, stage) in &config.inputs {
            create_processor(&stage.r#type, stage.clone())
                .unwrap_or_else(|error| panic!("input '{}' can be built: {}", name, error));
        }

        for (pipeline_name, pipeline) in &config.pipelines {
            for (stage_name, stage) in &pipeline.stages {
                create_processor(&stage.r#type, stage.clone()).unwrap_or_else(|error| {
                    panic!(
                        "pipeline stage '{}.{}' can be built: {}",
                        pipeline_name, stage_name, error
                    )
                });
            }
        }

        for (name, stage) in &config.outputs {
            create_processor(&stage.r#type, stage.clone())
                .unwrap_or_else(|error| panic!("output '{}' can be built: {}", name, error));
        }
    }

    #[test]
    fn preflight_reports_real_processor_construction_error() {
        let config = toml::from_str::<Config>(
            r#"
[inputs.sensor]
type = "simulated"
output = "raw_data"

[pipelines.main]
description = "Main"

[pipelines.main.stages.detector]
type = "rule"
inputs = ["raw_data"]
output = "detector_data"
parameters = { error_strategy = "continue", rules = [{ condition = { field_path = "value", operation = ">", value = 10 }, actions = [{ type = "keep_only_fields", field_paths = "[]" }], else_actions = [] }] }

[outputs.console]
type = "console"
inputs = ["detector_data"]
"#,
        )
        .expect("bad runtime config still parses structurally");

        let error = preflight_processors(&config)
            .expect_err("invalid processor parameters are rejected before launch");

        assert!(error.contains("Runtime preflight failed for stage 'main.detector'"));
        assert!(error.contains("processor 'rule' could not be created"));
        assert!(error.contains("invalid rules parameter"));
    }

    #[test]
    fn rejects_duplicate_added_node() {
        let mut document = parse_document(
            r#"
[inputs.sensor]
type = "simulated"
output = "raw_data"
"#,
        );
        let descriptor = processor_descriptors()
            .into_iter()
            .find(|descriptor| descriptor.type_name == "simulated")
            .expect("simulated descriptor exists");

        let error = add_node_to_document(&mut document, &descriptor, "sensor", None)
            .expect_err("duplicate input node is rejected");

        assert!(error.contains("already exists"));
    }

    #[test]
    fn deletes_producer_node_and_removes_downstream_inputs() {
        let mut document = parse_document(
            r#"
[inputs.sensor]
type = "simulated"
output = "raw_data"

[pipelines.rules.stages.filter]
type = "rule"
inputs = ["raw_data", "other_data"]
output = "filtered_data"

[outputs.console]
type = "console"
inputs = ["raw_data", "filtered_data"]
"#,
        );

        delete_node_from_document(&mut document, "input:sensor").expect("input node is deleted");
        remove_channel_from_all_inputs(&mut document, "raw_data")
            .expect("downstream inputs are cleaned");

        let edited = document.to_string();
        assert!(!edited.contains("[inputs.sensor]"));
        assert!(!edited.contains("\"raw_data\""));
        assert!(edited.contains("\"other_data\""));
        assert!(edited.contains("\"filtered_data\""));
    }

    #[test]
    fn deletes_output_node_without_touching_producers() {
        let mut document = parse_document(
            r#"
[inputs.sensor]
type = "simulated"
output = "raw_data"

[outputs.console]
type = "console"
inputs = ["raw_data"]
"#,
        );

        delete_node_from_document(&mut document, "output:console").expect("output node is deleted");

        let edited = document.to_string();
        assert!(edited.contains("[inputs.sensor]"));
        assert!(edited.contains("output = \"raw_data\""));
        assert!(!edited.contains("[outputs.console]"));
    }

    #[test]
    fn writable_config_path_requires_toml_extension() {
        let error =
            writable_config_path("pipeline.txt").expect_err("non TOML save path is rejected");

        assert!(error.contains(".toml"));
    }

    #[test]
    fn recursively_lists_workspace_toml_configs() {
        let workspace = unique_temp_workspace_path();
        let nested = workspace.join("nested");
        fs::create_dir_all(&nested).expect("workspace folders are created");
        fs::write(workspace.join("root.toml"), "").expect("root toml is written");
        fs::write(nested.join("child.toml"), "").expect("nested toml is written");
        fs::write(nested.join("notes.txt"), "").expect("non toml is written");

        let mut configs = Vec::new();
        collect_toml_files(&workspace, &mut configs).expect("workspace configs are listed");
        let _ = fs::remove_dir_all(&workspace);

        assert_eq!(configs.len(), 2);
        assert!(configs.iter().any(|path| path.ends_with("root.toml")));
        assert!(configs.iter().any(|path| path.ends_with("child.toml")));
        assert!(!configs.iter().any(|path| path.ends_with("notes.txt")));
    }

    #[test]
    fn copies_config_into_workspace() {
        let workspace = unique_temp_workspace_path();
        let source = unique_temp_config_path();
        let content = r#"
[inputs.sensor]
type = "simulated"
output = "raw_data"

[outputs.console]
type = "console"
inputs = ["raw_data"]
"#;
        fs::write(&source, content).expect("source config is written");

        let copied = copy_config_to_workspace_path(
            &workspace.to_string_lossy(),
            &source.to_string_lossy(),
            content,
        )
        .expect("config is copied into workspace");

        assert_eq!(copied.parent(), Some(workspace.as_path()));
        assert_eq!(copied.file_name(), source.file_name());
        assert_eq!(
            fs::read_to_string(&copied).expect("copied file is readable"),
            content
        );

        let _ = fs::remove_file(&source);
        let _ = fs::remove_dir_all(&workspace);
    }

    #[test]
    fn copy_into_workspace_refuses_to_overwrite_existing_config() {
        let workspace = unique_temp_workspace_path();
        fs::create_dir_all(&workspace).expect("workspace folder is created");
        let source = unique_temp_config_path();
        let target = workspace.join(source.file_name().expect("source has a file name"));
        let content = r#"
[inputs.sensor]
type = "simulated"
output = "raw_data"
"#;
        fs::write(&source, content).expect("source config is written");
        fs::write(&target, content).expect("existing workspace config is written");

        let error = copy_config_to_workspace_path(
            &workspace.to_string_lossy(),
            &source.to_string_lossy(),
            content,
        )
        .expect_err("existing workspace config is not overwritten");

        assert!(error.contains("already contains"));

        let _ = fs::remove_file(&source);
        let _ = fs::remove_dir_all(&workspace);
    }

    #[test]
    fn rejects_invalid_scalar_type() {
        let mut document = parse_document(
            r#"
[inputs.sensor]
type = "simulated"
output = "raw_data"
parameters = { interval_ms = 1000, enabled = true }
"#,
        );

        let number_error =
            update_existing_parameter(&mut document, "input:sensor", "interval_ms", "not-a-number")
                .expect_err("invalid integer is rejected");
        let bool_error =
            update_existing_parameter(&mut document, "input:sensor", "enabled", "sometimes")
                .expect_err("invalid boolean is rejected");

        assert!(number_error.contains("not an integer"));
        assert!(bool_error.contains("not a boolean"));
    }

    #[test]
    fn rejects_invalid_config_without_overwriting_file() {
        let content = r#"
[inputs.sensor]
type = "simulated"
output = "raw_data"
channel = { type = "broadcast", capacity = 128 }

[outputs.console]
type = "console"
inputs = ["raw_data"]
"#;
        let path = unique_temp_config_path();
        fs::write(&path, content).expect("test config is written");

        let result = update_node_field(
            path.to_string_lossy().to_string(),
            "input:sensor".to_string(),
            "channel.capacity".to_string(),
            "-1".to_string(),
        );

        let persisted = fs::read_to_string(&path).expect("test config can be reread");
        let _ = fs::remove_file(&path);

        assert!(result.is_err());
        assert_eq!(persisted, content);
    }

    fn parse_document(content: &str) -> DocumentMut {
        content.parse::<DocumentMut>().expect("test TOML parses")
    }

    fn unique_temp_config_path() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time is after UNIX epoch")
            .as_nanos();

        std::env::temp_dir().join(format!(
            "liminal-gui-edit-test-{}-{}.toml",
            std::process::id(),
            nanos
        ))
    }

    fn unique_temp_workspace_path() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time is after UNIX epoch")
            .as_nanos();

        std::env::temp_dir().join(format!(
            "liminal-gui-workspace-test-{}-{}",
            std::process::id(),
            nanos
        ))
    }

    #[test]
    fn shopfloor_example_passes_preflight() {
        let config = load_config(Path::new("../../config/examples/config_shopfloor.toml"))
            .expect("shopfloor example loads");
        liminal::config::validate_config(&config).expect("shopfloor example is valid");
        // Construction doesn't read the scenario (that happens in `init`, in the pipeline's
        // working directory), so preflight works from the GUI's own working directory.
        preflight_processors(&config).expect("shopfloor example passes preflight");
    }

    #[test]
    fn shopfloor_layout_loads_the_bundled_scenario() {
        let path = crate::commands::shopfloor::resolve_scenario_path("config/shopfloor/sample-floor.toml");
        assert!(path.is_absolute());
        let layout = crate::commands::shopfloor::load_layout(&path).expect("sample scenario loads");
        let json = serde_json::to_value(&layout).expect("layout serialises");
        assert_eq!(json["scenario"]["mobile_manipulators"][0]["id"], "mm1");
        assert_eq!(json["state"]["mobile_manipulators"][0]["id"], "mm1");
        assert_eq!(json["state"]["sim_ms"], 0);

        let missing = crate::commands::shopfloor::load_layout(Path::new("/nope/missing.toml"))
            .err()
            .expect("missing scenario is an error");
        assert!(missing.contains("/nope/missing.toml"), "{missing}");
    }

    #[test]
    fn shopfloor_scenarios_are_listed_and_paths_made_relative() {
        let scenarios = crate::commands::shopfloor::list_shopfloor_scenarios().expect("scenarios listed");
        assert!(
            scenarios.contains(&"config/shopfloor/sample-floor.toml".to_string()),
            "{scenarios:?}"
        );
        let absolute = crate::paths::repo_root().join("config/shopfloor/sample-floor.toml");
        assert_eq!(
            crate::commands::shopfloor::scenario_parameter_path(&absolute),
            "config/shopfloor/sample-floor.toml"
        );
        assert_eq!(
            crate::commands::shopfloor::scenario_parameter_path(Path::new("/elsewhere/s.toml")),
            "/elsewhere/s.toml"
        );
    }

    #[test]
    fn stage_commands_are_written_as_json_lines() {
        let mut written = Vec::new();
        crate::commands::runtime::write_stage_command(
            &mut written,
            "floor",
            serde_json::json!({"control": "pause"}),
        )
        .expect("write succeeds");
        assert_eq!(
            String::from_utf8(written).unwrap(),
            "{\"command\":{\"control\":\"pause\"},\"stage\":\"floor\"}\n"
        );
    }
}
