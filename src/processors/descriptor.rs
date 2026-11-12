use serde::Serialize;

use crate::processors::common::tcp::DEFAULT_MAX_FRAME_BYTES_STR;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessorCategory {
    Input,
    Transform,
    Aggregator,
    Output,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldKind {
    String,
    Integer,
    Number,
    Boolean,
    Enum,
    Array,
    Object,
    JsonValue,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldStatus {
    #[default]
    Stable,
    Experimental,
    Reserved,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SchemaSpec {
    Object {
        fields: Vec<FieldSpec>,
    },
    Array {
        item: Box<SchemaSpec>,
    },
    TaggedUnion {
        tag: String,
        variants: Vec<TaggedVariantSpec>,
    },
    JsonValue,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TaggedVariantSpec {
    pub tag_value: String,
    pub label: String,
    pub fields: Vec<FieldSpec>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FieldSpec {
    pub key: String,
    pub label: String,
    pub kind: FieldKind,
    /// With `applies_when`: required whenever the field applies.
    pub required: bool,
    pub default_value: Option<String>,
    pub options: Vec<String>,
    pub help: String,
    pub schema: Option<SchemaSpec>,
    pub renderer: Option<String>,
    pub status: FieldStatus,
    pub status_note: Option<String>,
    /// When the field applies, depending on other fields of the same processor: every condition
    /// must hold. Empty: always. A field that doesn't apply isn't used, and editors can hide it.
    pub applies_when: Vec<FieldCondition>,
}

/// A condition on another field: its value (or its default, when unset) is one of `values`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FieldCondition {
    pub field: String,
    pub values: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ProcessorDescriptor {
    pub type_name: String,
    pub category: ProcessorCategory,
    pub display_name: String,
    pub description: String,
    pub fields: Vec<FieldSpec>,
}

pub fn processor_descriptors() -> Vec<ProcessorDescriptor> {
    let mut descriptors = vec![
        ProcessorDescriptor {
            type_name: "simulated".to_string(),
            category: ProcessorCategory::Input,
            display_name: "Simulated Signal".to_string(),
            description: "Generates synthetic numeric signal messages.".to_string(),
            fields: vec![
                field(
                    "field_out",
                    "Output field",
                    FieldKind::String,
                    false,
                    None,
                    &[],
                    "Payload field written by the generated value.",
                ),
                field(
                    "interval_ms",
                    "Interval",
                    FieldKind::Integer,
                    false,
                    Some("1000"),
                    &[],
                    "Delay between generated messages in milliseconds.",
                ),
                field(
                    "distribution",
                    "Distribution",
                    FieldKind::Enum,
                    false,
                    Some("uniform"),
                    &["uniform", "normal"],
                    "Random distribution used for generated values.",
                ),
                field(
                    "min_value",
                    "Minimum",
                    FieldKind::Number,
                    false,
                    Some("0.0"),
                    &[],
                    "Minimum generated numeric value.",
                ),
                field(
                    "max_value",
                    "Maximum",
                    FieldKind::Number,
                    false,
                    Some("100.0"),
                    &[],
                    "Maximum generated numeric value.",
                ),
            ],
        },
        ProcessorDescriptor {
            type_name: "mqtt_sub".to_string(),
            category: ProcessorCategory::Input,
            display_name: "MQTT Subscriber".to_string(),
            description: "Subscribes to MQTT topics and emits received messages.".to_string(),
            fields: vec![
                field(
                    "broker_url",
                    "Broker URL",
                    FieldKind::String,
                    false,
                    Some("mqtt://localhost:1883"),
                    &[],
                    "MQTT broker URL.",
                ),
                field(
                    "client_id",
                    "Client ID",
                    FieldKind::String,
                    false,
                    None,
                    &[],
                    "Optional MQTT client ID.",
                ),
                field(
                    "qos",
                    "QoS",
                    FieldKind::Integer,
                    false,
                    Some("0"),
                    &[],
                    "MQTT quality-of-service level, 0 through 2.",
                ),
                field(
                    "clean_session",
                    "Clean session",
                    FieldKind::Boolean,
                    false,
                    Some("true"),
                    &[],
                    "Whether the MQTT connection starts a clean session.",
                ),
                field(
                    "username",
                    "Username",
                    FieldKind::String,
                    false,
                    None,
                    &[],
                    "Optional broker username.",
                ),
                field(
                    "password",
                    "Password",
                    FieldKind::String,
                    false,
                    None,
                    &[],
                    "Optional broker password.",
                ),
                field_with_schema(
                    field(
                        "topics",
                        "Topics",
                        FieldKind::Array,
                        false,
                        Some("[\"#\"]"),
                        &[],
                        "MQTT topic filters to subscribe to.",
                    ),
                    SchemaSpec::Array {
                        item: Box::new(SchemaSpec::JsonValue),
                    },
                    Some("string_array"),
                ),
            ],
        },
        ProcessorDescriptor {
            type_name: "tcp_input".to_string(),
            category: ProcessorCategory::Input,
            display_name: "TCP Input".to_string(),
            description: "Receives length-prefixed JSON messages over TCP.".to_string(),
            fields: tcp_fields(),
        },
        ProcessorDescriptor {
            type_name: "rule".to_string(),
            category: ProcessorCategory::Transform,
            display_name: "Rule Processor".to_string(),
            description: "Applies conditional message transformation rules.".to_string(),
            fields: rule_fields(),
        },
        ProcessorDescriptor {
            type_name: "fusion".to_string(),
            category: ProcessorCategory::Aggregator,
            display_name: "Fusion".to_string(),
            description: "Combines messages from multiple inputs.".to_string(),
            fields: fusion_fields(),
        },
        ProcessorDescriptor {
            type_name: "window".to_string(),
            category: ProcessorCategory::Aggregator,
            display_name: "Window".to_string(),
            description: "Aggregates the messages of a time span or the last N messages, per key."
                .to_string(),
            fields: window_fields(),
        },
        ProcessorDescriptor {
            type_name: "reduce".to_string(),
            category: ProcessorCategory::Aggregator,
            display_name: "Reduce".to_string(),
            description:
                "Keeps a state per key, updates it with expressions and outputs it with every message."
                    .to_string(),
            fields: reduce_fields(),
        },
        ProcessorDescriptor {
            type_name: "watchdog".to_string(),
            category: ProcessorCategory::Aggregator,
            display_name: "Watchdog".to_string(),
            description: "Alerts when an input, or a key within it, has been silent too long."
                .to_string(),
            fields: watchdog_fields(),
        },
        ProcessorDescriptor {
            type_name: "console".to_string(),
            category: ProcessorCategory::Output,
            display_name: "Console Output".to_string(),
            description: "Consumes messages and writes them to the console/log.".to_string(),
            fields: vec![],
        },
        ProcessorDescriptor {
            type_name: "file".to_string(),
            category: ProcessorCategory::Output,
            display_name: "File Output".to_string(),
            description: "Writes consumed messages to a file.".to_string(),
            fields: vec![
                field(
                    "file_path",
                    "File path",
                    FieldKind::String,
                    true,
                    None,
                    &[],
                    "Destination file path.",
                ),
                field(
                    "format",
                    "Format",
                    FieldKind::Enum,
                    false,
                    Some("json"),
                    &["json", "csv", "text", "pretty"],
                    "Output serialization format.",
                ),
                field(
                    "append",
                    "Append",
                    FieldKind::Boolean,
                    false,
                    Some("true"),
                    &[],
                    "Append instead of overwriting.",
                ),
                field(
                    "create_dirs",
                    "Create directories",
                    FieldKind::Boolean,
                    false,
                    Some("true"),
                    &[],
                    "Create missing parent directories.",
                ),
                field(
                    "buffer_size",
                    "Buffer size",
                    FieldKind::Integer,
                    false,
                    Some("8192"),
                    &[],
                    "Write buffer size in bytes.",
                ),
                field(
                    "auto_flush",
                    "Auto flush",
                    FieldKind::Boolean,
                    false,
                    Some("false"),
                    &[],
                    "Flush after each message.",
                ),
            ],
        },
        ProcessorDescriptor {
            type_name: "mqtt_pub".to_string(),
            category: ProcessorCategory::Output,
            display_name: "MQTT Publisher".to_string(),
            description: "Publishes consumed messages to MQTT topics.".to_string(),
            fields: vec![
                field(
                    "broker_url",
                    "Broker URL",
                    FieldKind::String,
                    false,
                    Some("mqtt://localhost:1883"),
                    &[],
                    "MQTT broker URL.",
                ),
                field(
                    "client_id",
                    "Client ID",
                    FieldKind::String,
                    false,
                    None,
                    &[],
                    "Optional MQTT client ID.",
                ),
                field(
                    "qos",
                    "QoS",
                    FieldKind::Integer,
                    false,
                    Some("0"),
                    &[],
                    "MQTT quality-of-service level, 0 through 2.",
                ),
                field(
                    "clean_session",
                    "Clean session",
                    FieldKind::Boolean,
                    false,
                    Some("true"),
                    &[],
                    "Whether the MQTT connection starts a clean session.",
                ),
                field(
                    "username",
                    "Username",
                    FieldKind::String,
                    false,
                    None,
                    &[],
                    "Optional broker username.",
                ),
                field(
                    "password",
                    "Password",
                    FieldKind::String,
                    false,
                    None,
                    &[],
                    "Optional broker password.",
                ),
                field(
                    "topic_map",
                    "Topic map",
                    FieldKind::Object,
                    false,
                    Some("{}"),
                    &[],
                    "Map from input channel names to MQTT topics.",
                ),
                field(
                    "default_topic",
                    "Default topic",
                    FieldKind::String,
                    false,
                    None,
                    &[],
                    "Fallback MQTT topic for unmapped inputs.",
                ),
                field(
                    "retain",
                    "Retain",
                    FieldKind::Boolean,
                    false,
                    Some("false"),
                    &[],
                    "Publish retained MQTT messages.",
                ),
            ],
        },
        ProcessorDescriptor {
            type_name: "tcp_output".to_string(),
            category: ProcessorCategory::Output,
            display_name: "TCP Output".to_string(),
            description: "Sends length-prefixed JSON messages over TCP.".to_string(),
            fields: tcp_fields(),
        },
    ];

    descriptors.sort_by(|a, b| a.type_name.cmp(&b.type_name));
    descriptors
}

fn field(
    key: &'static str,
    label: &'static str,
    kind: FieldKind,
    required: bool,
    default_value: Option<&'static str>,
    options: &'static [&'static str],
    help: &'static str,
) -> FieldSpec {
    FieldSpec {
        key: key.to_string(),
        label: label.to_string(),
        kind,
        required,
        default_value: default_value.map(str::to_string),
        options: options.iter().map(|option| option.to_string()).collect(),
        help: help.to_string(),
        schema: None,
        renderer: None,
        status: FieldStatus::Stable,
        status_note: None,
        applies_when: Vec::new(),
    }
}

fn field_with_schema(
    mut field: FieldSpec,
    schema: SchemaSpec,
    renderer: Option<&'static str>,
) -> FieldSpec {
    field.schema = Some(schema);
    field.renderer = renderer.map(str::to_string);
    field
}

/// A field that only applies when each of the other fields named holds one of the values given.
fn applies_when(mut field: FieldSpec, conditions: &[(&'static str, &[&'static str])]) -> FieldSpec {
    field.applies_when = conditions
        .iter()
        .map(|(other, values)| FieldCondition {
            field: other.to_string(),
            values: values.iter().map(|value| value.to_string()).collect(),
        })
        .collect();
    field
}

fn rule_fields() -> Vec<FieldSpec> {
    vec![
        field_with_schema(
            field(
                "rules",
                "Rules",
                FieldKind::Array,
                true,
                Some(
                    "[{ condition = { operation = \"always\" }, actions = [{ type = \"pass_through\" }], else_actions = [] }]",
                ),
                &[],
                "Ordered rule list with conditions, actions, and else actions.",
            ),
            SchemaSpec::Array {
                item: Box::new(rule_schema()),
            },
            Some("rule_builder"),
        ),
        field(
            "error_strategy",
            "Error strategy",
            FieldKind::Enum,
            false,
            Some("continue"),
            &["continue", "skip", "abort", "use_default"],
            "Behavior when a rule action fails.",
        ),
    ]
}

fn fusion_fields() -> Vec<FieldSpec> {
    vec![
        field(
            "mode",
            "Mode",
            FieldKind::Enum,
            false,
            Some("merge_objects"),
            &["merge_objects", "nest_by_input", "latest_by_input"],
            "How incoming payloads are combined.",
        ),
        field(
            "conflict_strategy",
            "Conflict strategy",
            FieldKind::Enum,
            false,
            Some("prefix"),
            &["prefix", "overwrite"],
            "How duplicate field names are handled when merging objects.",
        ),
        field(
            "join_window_ms",
            "Join window",
            FieldKind::Integer,
            false,
            Some("25"),
            &[],
            "Milliseconds to wait for other ready inputs before emitting a fused message.",
        ),
    ]
}

fn window_fields() -> Vec<FieldSpec> {
    const TIME_WINDOWS: &[&str] = &["tumbling", "sliding"];

    vec![
        field(
            "kind",
            "Kind",
            FieldKind::Enum,
            true,
            Some("tumbling"),
            &["tumbling", "sliding", "last_n"],
            "tumbling: back-to-back windows of size_ms; sliding: the last size_ms; last_n: the last size_n messages.",
        ),
        applies_when(
            field(
                "size_ms",
                "Size (ms)",
                FieldKind::Integer,
                true,
                Some("10000"),
                &[],
                "Window length, in event time.",
            ),
            &[("kind", TIME_WINDOWS)],
        ),
        applies_when(
            field(
                "slide_ms",
                "Slide (ms)",
                FieldKind::Integer,
                true,
                None,
                &[],
                "How often the sliding window outputs (at most size_ms).",
            ),
            &[("kind", &["sliding"]), ("emit", &["on_close"])],
        ),
        applies_when(
            field(
                "size_n",
                "Size (messages)",
                FieldKind::Integer,
                true,
                None,
                &[],
                "Number of messages in the window.",
            ),
            &[("kind", &["last_n"])],
        ),
        field_with_schema(
            field(
                "aggregates",
                "Aggregates",
                FieldKind::Array,
                true,
                Some("[{ function = \"count\" }]"),
                &[],
                "What to compute over each window: a function, the field it reads, and its output name (as).",
            ),
            SchemaSpec::Array {
                item: Box::new(window_aggregate_schema()),
            },
            Some("object_list"),
        ),
        field(
            "emit",
            "Emit",
            FieldKind::Enum,
            false,
            Some("on_close"),
            &["on_close", "every_message"],
            "on_close: once per window; every_message: with every message (a moving value).",
        ),
        field_with_schema(
            field(
                "group_by",
                "Group by",
                FieldKind::Array,
                false,
                Some("[]"),
                &[],
                "Field paths that split the stream into separate windows, e.g. sensor.",
            ),
            SchemaSpec::Array {
                item: Box::new(SchemaSpec::JsonValue),
            },
            Some("string_array"),
        ),
        applies_when(
            field(
                "emit_empty",
                "Emit empty windows",
                FieldKind::Boolean,
                false,
                Some("false"),
                &[],
                "Also output windows with no messages (count 0, null for the others).",
            ),
            &[("kind", TIME_WINDOWS), ("emit", &["on_close"])],
        ),
        field(
            "max_items",
            "Max messages per key",
            FieldKind::Integer,
            false,
            Some("100000"),
            &[],
            "Messages kept per key; the oldest are dropped beyond this.",
        ),
    ]
}

fn window_aggregate_schema() -> SchemaSpec {
    SchemaSpec::Object {
        fields: vec![
            field(
                "function",
                "Function",
                FieldKind::Enum,
                true,
                Some("avg"),
                &[
                    "count",
                    "distinct_count",
                    "sum",
                    "avg",
                    "min",
                    "max",
                    "spread",
                    "std",
                    "first",
                    "last",
                ],
                "count, distinct_count, sum, avg, min, max, spread (max - min), std, or first/last (the earliest/latest value).",
            ),
            applies_when(
                field(
                    "field",
                    "Field",
                    FieldKind::String,
                    true,
                    None,
                    &[],
                    "Field path to aggregate.",
                ),
                &[(
                    "function",
                    &[
                        "distinct_count",
                        "sum",
                        "avg",
                        "min",
                        "max",
                        "spread",
                        "std",
                        "first",
                        "last",
                    ],
                )],
            ),
            field(
                "as",
                "Output name",
                FieldKind::String,
                false,
                None,
                &[],
                "Name in the output. Default: <field>_<function>, or count.",
            ),
        ],
    }
}

fn reduce_fields() -> Vec<FieldSpec> {
    vec![
        FieldSpec {
            renderer: Some("key_value".to_string()),
            ..field(
                "initial",
                "Initial state",
                FieldKind::Object,
                true,
                Some("{ messages = 0 }"),
                &[],
                "The state at the start and after a reset: plain names and a number, text or true/false each. Expressions read them as state.<name>.",
            )
        },
        field_with_schema(
            field(
                "update",
                "Updates",
                FieldKind::Array,
                true,
                Some("[{ field = \"messages\", expression = \"state.messages + 1\" }]"),
                &[],
                "With every message, each state value's new value, optionally only when a condition holds. All read the state from before the message.",
            ),
            SchemaSpec::Array {
                item: Box::new(reduce_update_schema()),
            },
            Some("object_list"),
        ),
        field_with_schema(
            field(
                "reset_when",
                "Reset when",
                FieldKind::Object,
                false,
                None,
                &[],
                "Puts the state back to the initial one when this holds, before the updates.",
            ),
            condition_schema(),
            Some("condition"),
        ),
        field_with_schema(
            field(
                "group_by",
                "Group by",
                FieldKind::Array,
                false,
                Some("[]"),
                &[],
                "Field paths that split the stream into separate states, e.g. sensor.",
            ),
            SchemaSpec::Array {
                item: Box::new(SchemaSpec::JsonValue),
            },
            Some("string_array"),
        ),
        field(
            "include_message",
            "Include message",
            FieldKind::Boolean,
            false,
            Some("false"),
            &[],
            "Also output the message's fields, with the state laid over them.",
        ),
    ]
}

fn reduce_update_schema() -> SchemaSpec {
    SchemaSpec::Object {
        fields: vec![
            field(
                "field",
                "State value",
                FieldKind::String,
                true,
                None,
                &[],
                "Name of the state value to set (one of the initial state's).",
            ),
            FieldSpec {
                renderer: Some("expression".to_string()),
                ..field(
                    "expression",
                    "Expression",
                    FieldKind::String,
                    true,
                    None,
                    &[],
                    "Its new value. Reads the message's fields, state.<name>, prev.<field> (the previous message), msg.<field>, count and dt (seconds since prev).",
                )
            },
            field_with_schema(
                field(
                    "when",
                    "When",
                    FieldKind::Object,
                    false,
                    None,
                    &[],
                    "Only update when this holds.",
                ),
                condition_schema(),
                Some("condition"),
            ),
        ],
    }
}

fn watchdog_fields() -> Vec<FieldSpec> {
    let string_array = || SchemaSpec::Array {
        item: Box::new(SchemaSpec::JsonValue),
    };
    vec![
        field(
            "timeout_ms",
            "Timeout (ms)",
            FieldKind::Integer,
            true,
            Some("5000"),
            &[],
            "How long a key can be silent before an alert.",
        ),
        FieldSpec {
            renderer: Some("key_value".to_string()),
            ..field(
                "timeouts",
                "Timeouts per key",
                FieldKind::Object,
                false,
                None,
                &[],
                "A key's own timeout in ms, e.g. machine2.temperature = 3000 for a slower sensor. Keys are group_by values, or input names without group_by.",
            )
        },
        field_with_schema(
            field(
                "group_by",
                "Group by",
                FieldKind::Array,
                false,
                Some("[]"),
                &[],
                "Field whose values are watched separately, e.g. sensor. Without it, each input is watched.",
            ),
            string_array(),
            Some("string_array"),
        ),
        field_with_schema(
            field(
                "expected_keys",
                "Expected keys",
                FieldKind::Array,
                false,
                Some("[]"),
                &[],
                "Keys watched from the start, so one that never reports is caught (needs group_by; without it, every input is).",
            ),
            string_array(),
            Some("string_array"),
        ),
        field(
            "emit_recovered",
            "Emit recovered",
            FieldKind::Boolean,
            false,
            Some("false"),
            &[],
            "Also output a message when a silent key reports again.",
        ),
    ]
}

fn rule_schema() -> SchemaSpec {
    SchemaSpec::Object {
        fields: vec![
            field_with_schema(
                field(
                    "condition",
                    "Condition",
                    FieldKind::Object,
                    true,
                    None,
                    &[],
                    "Message predicate that decides whether actions run.",
                ),
                condition_schema(),
                None,
            ),
            field_with_schema(
                field(
                    "actions",
                    "Actions",
                    FieldKind::Array,
                    true,
                    None,
                    &[],
                    "Actions executed when the condition matches.",
                ),
                SchemaSpec::Array {
                    item: Box::new(action_schema()),
                },
                None,
            ),
            field_with_schema(
                field(
                    "else_actions",
                    "Else actions",
                    FieldKind::Array,
                    false,
                    None,
                    &[],
                    "Actions executed when the condition does not match.",
                ),
                SchemaSpec::Array {
                    item: Box::new(action_schema()),
                },
                None,
            ),
        ],
    }
}

fn condition_schema() -> SchemaSpec {
    SchemaSpec::Object {
        fields: vec![
            field(
                "field_path",
                "Field path",
                FieldKind::String,
                false,
                None,
                &[],
                "Payload field path to evaluate, e.g. device.id or items[0].value. Not required for always or never.",
            ),
            field(
                "operation",
                "Operation",
                FieldKind::Enum,
                true,
                Some("equals"),
                &[
                    "always",
                    "never",
                    "equals",
                    "not_equals",
                    "startswith",
                    "endswith",
                    "contains",
                    ">",
                    ">=",
                    "<",
                    "<=",
                ],
                "Comparison operation. Always runs actions; never runs else actions.",
            ),
            field(
                "value",
                "Value",
                FieldKind::JsonValue,
                false,
                None,
                &[],
                "Expected JSON value. Not required for always or never.",
            ),
        ],
    }
}

fn action_schema() -> SchemaSpec {
    SchemaSpec::TaggedUnion {
        tag: "type".to_string(),
        variants: vec![
            action_variant(
                "set_field",
                "Set field",
                vec![
                    field(
                        "field_path",
                        "Field path",
                        FieldKind::String,
                        true,
                        None,
                        &[],
                        "Payload field to set, e.g. device.id or items[0].value.",
                    ),
                    field(
                        "value",
                        "Value",
                        FieldKind::JsonValue,
                        true,
                        None,
                        &[],
                        "JSON value to assign.",
                    ),
                ],
            ),
            action_variant(
                "remove_field",
                "Remove field",
                vec![field(
                    "field_path",
                    "Field path",
                    FieldKind::String,
                    true,
                    None,
                    &[],
                    "Payload field to remove, e.g. device.id or items[0].value.",
                )],
            ),
            action_variant(
                "copy_field",
                "Copy field",
                vec![
                    field(
                        "source_field",
                        "Source field",
                        FieldKind::String,
                        true,
                        None,
                        &[],
                        "Payload field to copy from, e.g. device.id or items[0].value.",
                    ),
                    field(
                        "target_field",
                        "Target field",
                        FieldKind::String,
                        true,
                        None,
                        &[],
                        "Payload field to copy into, e.g. device.id or items[0].value.",
                    ),
                ],
            ),
            action_variant(
                "rename_field",
                "Rename field",
                vec![
                    field(
                        "old_field",
                        "Old field",
                        FieldKind::String,
                        true,
                        None,
                        &[],
                        "Payload field to rename, e.g. device.id or items[0].value.",
                    ),
                    field(
                        "new_field",
                        "New field",
                        FieldKind::String,
                        true,
                        None,
                        &[],
                        "New payload field path, e.g. device.id or items[0].value.",
                    ),
                ],
            ),
            action_variant(
                "compute_field",
                "Compute field",
                vec![
                    field(
                        "field_path",
                        "Field path",
                        FieldKind::String,
                        true,
                        None,
                        &[],
                        "Payload field to write, e.g. device.id or items[0].value.",
                    ),
                    field(
                        "expression",
                        "Expression",
                        FieldKind::String,
                        true,
                        None,
                        &[],
                        "Expression evaluated against the payload.",
                    ),
                ],
            ),
            action_variant("drop_message", "Drop message", vec![]),
            action_variant("pass_through", "Pass through", vec![]),
            action_variant(
                "keep_only_fields",
                "Keep only fields",
                vec![field_with_schema(
                    field(
                        "field_paths",
                        "Field paths",
                        FieldKind::Array,
                        true,
                        None,
                        &[],
                        "Payload fields to retain, e.g. device.id or items[0].value.",
                    ),
                    SchemaSpec::Array {
                        item: Box::new(SchemaSpec::JsonValue),
                    },
                    None,
                )],
            ),
        ],
    }
}

fn action_variant(
    tag_value: &'static str,
    label: &'static str,
    fields: Vec<FieldSpec>,
) -> TaggedVariantSpec {
    TaggedVariantSpec {
        tag_value: tag_value.to_string(),
        label: label.to_string(),
        fields,
    }
}

fn tcp_fields() -> Vec<FieldSpec> {
    vec![
        field(
            "mode",
            "Mode",
            FieldKind::Enum,
            false,
            Some("client"),
            &["client", "server"],
            "TCP connection mode. Server mode accepts one client connection at a time.",
        ),
        field(
            "host",
            "Host",
            FieldKind::String,
            false,
            Some("localhost"),
            &[],
            "TCP host or bind address.",
        ),
        field(
            "port",
            "Port",
            FieldKind::Integer,
            false,
            Some("8080"),
            &[],
            "TCP port.",
        ),
        field(
            "reconnect",
            "Reconnect",
            FieldKind::Boolean,
            false,
            Some("true"),
            &[],
            "Reconnect when the TCP connection drops.",
        ),
        field(
            "reconnect_interval_ms",
            "Reconnect interval",
            FieldKind::Integer,
            false,
            Some("5000"),
            &[],
            "Delay between reconnect attempts in milliseconds.",
        ),
        field(
            "max_frame_bytes",
            "Max frame bytes",
            FieldKind::Integer,
            false,
            Some(DEFAULT_MAX_FRAME_BYTES_STR),
            &[],
            "Maximum accepted length-prefixed TCP frame size in bytes.",
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptors_cover_default_processors() {
        let names = processor_descriptors()
            .into_iter()
            .map(|descriptor| descriptor.type_name)
            .collect::<Vec<_>>();

        assert_eq!(
            names,
            vec![
                "console",
                "file",
                "fusion",
                "mqtt_pub",
                "mqtt_sub",
                "reduce",
                "rule",
                "simulated",
                "tcp_input",
                "tcp_output",
                "watchdog",
                "window",
            ]
        );
    }

    #[test]
    fn watchdog_descriptor_reuses_the_key_value_and_list_editors() {
        let descriptors = processor_descriptors();
        let watchdog = descriptors
            .iter()
            .find(|descriptor| descriptor.type_name == "watchdog")
            .expect("watchdog descriptor exists");
        let field = |key: &str| {
            watchdog
                .fields
                .iter()
                .find(|field| field.key == key)
                .unwrap_or_else(|| panic!("{key} field exists"))
        };

        assert!(field("timeout_ms").required);
        assert_eq!(field("timeouts").renderer.as_deref(), Some("key_value"));
        assert_eq!(field("group_by").renderer.as_deref(), Some("string_array"));
        assert_eq!(
            field("expected_keys").renderer.as_deref(),
            Some("string_array")
        );
        assert_eq!(
            field("emit_recovered").default_value.as_deref(),
            Some("false")
        );
    }

    #[test]
    fn rule_descriptor_marks_rules_as_required_array() {
        let descriptors = processor_descriptors();
        let rule = descriptors
            .iter()
            .find(|descriptor| descriptor.type_name == "rule")
            .expect("rule descriptor exists");
        let rules = rule
            .fields
            .iter()
            .find(|field| field.key == "rules")
            .expect("rules field exists");
        let error_strategy = rule
            .fields
            .iter()
            .find(|field| field.key == "error_strategy")
            .expect("error strategy field exists");

        assert_eq!(rules.kind, FieldKind::Array);
        assert!(rules.required);
        assert_eq!(rules.status, FieldStatus::Stable);
        assert_eq!(error_strategy.kind, FieldKind::Enum);
        assert_eq!(error_strategy.status, FieldStatus::Stable);
        assert!(error_strategy.status_note.is_none());
        assert_eq!(
            error_strategy.options,
            vec!["continue", "skip", "abort", "use_default"]
        );
    }

    #[test]
    fn rule_descriptor_exposes_nested_rule_schema() {
        let descriptors = processor_descriptors();
        let rule = descriptors
            .iter()
            .find(|descriptor| descriptor.type_name == "rule")
            .expect("rule descriptor exists");
        let rules = rule
            .fields
            .iter()
            .find(|field| field.key == "rules")
            .expect("rules field exists");

        assert_eq!(rules.renderer.as_deref(), Some("rule_builder"));

        let Some(SchemaSpec::Array { item }) = &rules.schema else {
            panic!("rules has an array schema");
        };
        let SchemaSpec::Object { fields } = item.as_ref() else {
            panic!("rule items are objects");
        };

        assert!(fields.iter().any(|field| field.key == "condition"));
        assert!(fields.iter().any(|field| field.key == "actions"));
        assert!(fields.iter().any(|field| field.key == "else_actions"));

        let condition = fields
            .iter()
            .find(|field| field.key == "condition")
            .expect("condition field exists");
        let Some(SchemaSpec::Object {
            fields: condition_fields,
        }) = &condition.schema
        else {
            panic!("condition has an object schema");
        };
        let operation = condition_fields
            .iter()
            .find(|field| field.key == "operation")
            .expect("operation field exists");
        let field_path = condition_fields
            .iter()
            .find(|field| field.key == "field_path")
            .expect("field_path field exists");
        let value = condition_fields
            .iter()
            .find(|field| field.key == "value")
            .expect("value field exists");

        assert!(operation.options.contains(&"always".to_string()));
        assert!(operation.options.contains(&"never".to_string()));
        assert!(!field_path.required);
        assert!(!value.required);

        let actions = fields
            .iter()
            .find(|field| field.key == "actions")
            .expect("actions field exists");
        let Some(SchemaSpec::Array { item }) = &actions.schema else {
            panic!("actions has an array schema");
        };
        let SchemaSpec::TaggedUnion { tag, variants } = item.as_ref() else {
            panic!("actions are a tagged union");
        };

        assert_eq!(tag, "type");
        assert!(
            variants
                .iter()
                .any(|variant| variant.tag_value == "set_field")
        );
        assert!(
            variants
                .iter()
                .any(|variant| variant.tag_value == "drop_message")
        );
        assert!(
            variants
                .iter()
                .any(|variant| variant.tag_value == "keep_only_fields")
        );
    }

    #[test]
    fn descriptors_are_sorted_by_processor_type() {
        let names = processor_descriptors()
            .into_iter()
            .map(|descriptor| descriptor.type_name)
            .collect::<Vec<_>>();
        let mut sorted_names = names.clone();

        sorted_names.sort();

        assert_eq!(names, sorted_names);
    }

    #[test]
    fn mqtt_sub_topics_use_string_array_renderer() {
        let descriptors = processor_descriptors();
        let mqtt_sub = descriptors
            .iter()
            .find(|descriptor| descriptor.type_name == "mqtt_sub")
            .expect("mqtt_sub descriptor exists");
        let topics = mqtt_sub
            .fields
            .iter()
            .find(|field| field.key == "topics")
            .expect("topics field exists");

        assert_eq!(topics.kind, FieldKind::Array);
        assert_eq!(topics.renderer.as_deref(), Some("string_array"));
        assert!(matches!(topics.schema, Some(SchemaSpec::Array { .. })));
    }

    #[test]
    fn window_descriptor_marks_fields_that_depend_on_kind_and_emit() {
        let descriptors = processor_descriptors();
        let window = descriptors
            .iter()
            .find(|descriptor| descriptor.type_name == "window")
            .expect("window descriptor exists");
        let field = |key: &str| {
            window
                .fields
                .iter()
                .find(|field| field.key == key)
                .unwrap_or_else(|| panic!("{key} field exists"))
        };
        let condition = |field: &str, values: &[&str]| FieldCondition {
            field: field.to_string(),
            values: values.iter().map(|value| value.to_string()).collect(),
        };

        assert!(field("kind").applies_when.is_empty());
        assert_eq!(
            field("size_ms").applies_when,
            vec![condition("kind", &["tumbling", "sliding"])]
        );
        assert_eq!(
            field("slide_ms").applies_when,
            vec![
                condition("kind", &["sliding"]),
                condition("emit", &["on_close"])
            ]
        );
        assert_eq!(
            field("size_n").applies_when,
            vec![condition("kind", &["last_n"])]
        );
        assert!(field("size_n").required);

        let aggregates = field("aggregates");
        assert_eq!(aggregates.renderer.as_deref(), Some("object_list"));
        let Some(SchemaSpec::Array { item }) = &aggregates.schema else {
            panic!("aggregates is an array");
        };
        let SchemaSpec::Object { fields } = item.as_ref() else {
            panic!("aggregates are objects");
        };
        let keys: Vec<&str> = fields.iter().map(|field| field.key.as_str()).collect();
        assert_eq!(keys, vec!["function", "field", "as"]);
        // An aggregate's field applies to every function but count, and is then required.
        let aggregate_field = &fields[1];
        assert!(aggregate_field.required);
        assert_eq!(aggregate_field.applies_when.len(), 1);
        assert_eq!(aggregate_field.applies_when[0].field, "function");
        assert!(
            !aggregate_field.applies_when[0]
                .values
                .contains(&"count".to_string())
        );
        assert!(
            aggregate_field.applies_when[0]
                .values
                .contains(&"sum".to_string())
        );
    }

    #[test]
    fn reduce_descriptor_uses_the_condition_and_key_value_renderers() {
        let descriptors = processor_descriptors();
        let reduce = descriptors
            .iter()
            .find(|descriptor| descriptor.type_name == "reduce")
            .expect("reduce descriptor exists");
        let field = |key: &str| {
            reduce
                .fields
                .iter()
                .find(|field| field.key == key)
                .unwrap_or_else(|| panic!("{key} field exists"))
        };

        assert_eq!(field("initial").renderer.as_deref(), Some("key_value"));
        assert!(field("initial").required);
        assert_eq!(field("reset_when").renderer.as_deref(), Some("condition"));
        assert_eq!(field("reset_when").schema, Some(condition_schema()));
        assert_eq!(field("group_by").renderer.as_deref(), Some("string_array"));

        let update = field("update");
        assert_eq!(update.renderer.as_deref(), Some("object_list"));
        let Some(SchemaSpec::Array { item }) = &update.schema else {
            panic!("update is an array");
        };
        let SchemaSpec::Object { fields } = item.as_ref() else {
            panic!("updates are objects");
        };
        let keys: Vec<&str> = fields.iter().map(|field| field.key.as_str()).collect();
        assert_eq!(keys, vec!["field", "expression", "when"]);
        assert_eq!(fields[2].renderer.as_deref(), Some("condition"));
        assert!(!fields[2].required);
    }

    #[test]
    fn fusion_descriptor_exposes_aggregator_defaults() {
        let descriptors = processor_descriptors();
        let fusion = descriptors
            .iter()
            .find(|descriptor| descriptor.type_name == "fusion")
            .expect("fusion descriptor exists");

        let field_names = fusion
            .fields
            .iter()
            .map(|field| field.key.as_str())
            .collect::<Vec<_>>();

        assert_eq!(fusion.category, ProcessorCategory::Aggregator);
        assert_eq!(
            field_names,
            vec!["mode", "conflict_strategy", "join_window_ms"]
        );
        assert_eq!(
            fusion.fields[0].default_value.as_deref(),
            Some("merge_objects")
        );
        assert_eq!(
            fusion.fields[0].options,
            vec!["merge_objects", "nest_by_input", "latest_by_input"]
        );
        assert_eq!(fusion.fields[1].default_value.as_deref(), Some("prefix"));
        assert_eq!(fusion.fields[2].default_value.as_deref(), Some("25"));
    }

    #[test]
    fn tcp_descriptors_expose_max_frame_default() {
        let descriptors = processor_descriptors();

        for type_name in ["tcp_input", "tcp_output"] {
            let descriptor = descriptors
                .iter()
                .find(|descriptor| descriptor.type_name == type_name)
                .expect("tcp descriptor exists");
            let max_frame = descriptor
                .fields
                .iter()
                .find(|field| field.key == "max_frame_bytes")
                .expect("max_frame_bytes field exists");

            assert_eq!(max_frame.kind, FieldKind::Integer);
            assert_eq!(
                max_frame.default_value.as_deref(),
                Some(DEFAULT_MAX_FRAME_BYTES_STR)
            );
        }
    }
}
