use crate::config::{ProcessorConfig, StageConfig, defaulted_param, required_param};
use crate::core::input_poll::{DEFAULT_MAX_DRAIN_PER_INPUT, drain_bounded_each, idle_sleep};
use crate::core::timing_mixin::{TimingMixin, WithTimingMixin};
use crate::core::{context::ProcessingContext, message::Message};
use crate::processors::common::expression::{Expression, Variables};
use crate::processors::common::field_utils::FieldUtils;
use crate::processors::processor::Processor;

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use serde_json::{Number, Value};
use std::collections::HashMap;
use tracing::{debug, error, warn};

pub use crate::processors::common::condition_utils::Condition;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RuleConfig {
    pub rules: Vec<Rule>,
    #[serde(default = "default_error_strategy")]
    pub error_strategy: ErrorStrategy,
    #[serde(skip)]
    pub timing: Option<crate::config::TimingConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorStrategy {
    Continue,   // Log error and continue processing
    Skip,       // Skip this action, continue with others
    Abort,      // Stop processing this message
    UseDefault, // Use default/fallback values on error
}

fn default_error_strategy() -> ErrorStrategy {
    ErrorStrategy::Continue
}

impl ProcessorConfig for RuleConfig {
    fn from_stage_config(config: &StageConfig) -> Result<Self> {
        let rules: Vec<Rule> = required_param(&config.parameters, "rules")
            .map_err(|error| anyhow!("invalid rules parameter: {}", error))?;
        if rules.is_empty() {
            return Err(anyhow!("rule_transformer requires at least one rule"));
        }

        let error_strategy: ErrorStrategy = defaulted_param(
            &config.parameters,
            "error_strategy",
            default_error_strategy(),
        )
        .map_err(|error| anyhow!("invalid error_strategy parameter: {}", error))?;

        // Extract timing configuration
        let timing_config = config.timing.clone();

        Ok(Self {
            rules,
            error_strategy,
            timing: timing_config,
        })
    }
    fn validate(&self) -> Result<()> {
        if self.rules.is_empty() {
            return Err(anyhow!("At least one rule must be defined"));
        }

        for (i, rule) in self.rules.iter().enumerate() {
            rule.condition
                .validate()
                .map_err(|error| anyhow!("Rule {} has {}", i, error))?;

            if rule.actions.is_empty() {
                return Err(anyhow!("Rule {} has no actions", i));
            }

            // Validate actions
            for (j, action) in rule.actions.iter().enumerate() {
                RuleConfig::validate_action(action, &format!("Rule {}, Action {}", i, j))?;
            }

            // Validate else_actions
            for (j, action) in rule.else_actions.iter().enumerate() {
                RuleConfig::validate_action(action, &format!("Rule {}, Else Action {}", i, j))?;
            }
        }

        Ok(())
    }
}

impl RuleConfig {
    fn validate_action(action: &Action, context: &str) -> Result<()> {
        match action {
            Action::SetField { field_path, .. } => {
                FieldUtils::validate_field_path(field_path).map_err(|error| {
                    anyhow!("{}: SetField has invalid field_path: {}", context, error)
                })?;
            }
            Action::RemoveField { field_path } => {
                FieldUtils::validate_field_path(field_path).map_err(|error| {
                    anyhow!("{}: RemoveField has invalid field_path: {}", context, error)
                })?;
            }
            Action::CopyField {
                source_field,
                target_field,
            } => {
                FieldUtils::validate_field_path(source_field).map_err(|error| {
                    anyhow!("{}: CopyField has invalid source_field: {}", context, error)
                })?;
                FieldUtils::validate_field_path(target_field).map_err(|error| {
                    anyhow!("{}: CopyField has invalid target_field: {}", context, error)
                })?;
                if source_field == target_field {
                    return Err(anyhow!(
                        "{}: CopyField source and target are the same",
                        context
                    ));
                }
            }
            Action::RenameField {
                old_field,
                new_field,
            } => {
                FieldUtils::validate_field_path(old_field).map_err(|error| {
                    anyhow!("{}: RenameField has invalid old_field: {}", context, error)
                })?;
                FieldUtils::validate_field_path(new_field).map_err(|error| {
                    anyhow!("{}: RenameField has invalid new_field: {}", context, error)
                })?;
                if old_field == new_field {
                    return Err(anyhow!(
                        "{}: RenameField old and new field are the same",
                        context
                    ));
                }
            }
            Action::ComputeField {
                field_path,
                expression,
            } => {
                FieldUtils::validate_field_path(field_path).map_err(|error| {
                    anyhow!(
                        "{}: ComputeField has invalid field_path: {}",
                        context,
                        error
                    )
                })?;
                if expression.is_empty() {
                    return Err(anyhow!("{}: ComputeField has empty expression", context));
                }
                Expression::parse(expression)
                    .map_err(|error| anyhow!("{}: ComputeField has {}", context, error))?;
            }
            Action::KeepOnlyFields { field_paths } => {
                // Empty field_paths is valid - it means "keep no fields" (clear everything)
                for field_path in field_paths {
                    FieldUtils::validate_field_path(field_path).map_err(|error| {
                        anyhow!(
                            "{}: KeepOnlyFields contains invalid field_path: {}",
                            context,
                            error
                        )
                    })?;
                }
            }
            Action::DropMessage | Action::PassThrough => {
                // These actions have no parameters to validate
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Rule {
    pub condition: Condition,
    pub actions: Vec<Action>,
    #[serde(default)]
    pub else_actions: Vec<Action>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type")]
pub enum Action {
    #[serde(rename = "set_field")]
    SetField { field_path: String, value: Value },
    #[serde(rename = "remove_field")]
    RemoveField { field_path: String },
    #[serde(rename = "copy_field")]
    CopyField {
        source_field: String,
        target_field: String,
    },
    #[serde(rename = "rename_field")]
    RenameField {
        old_field: String,
        new_field: String,
    },
    #[serde(rename = "compute_field")]
    ComputeField {
        field_path: String,
        expression: String,
    },
    #[serde(rename = "drop_message")]
    DropMessage,
    #[serde(rename = "pass_through")]
    PassThrough,
    #[serde(rename = "keep_only_fields")]
    KeepOnlyFields { field_paths: Vec<String> },
}

#[derive(Debug, Clone, PartialEq, PartialOrd)]
enum ActionPriority {
    Reset = 1,     // KeepOnlyFields (destructive reset)
    Transform = 2, // SetField, CopyField, RenameField, RemoveField, ComputeField assignment
    Control = 3,   // DropMessage, PassThrough
}

impl Action {
    fn priority(&self) -> ActionPriority {
        match self {
            Action::KeepOnlyFields { .. } => ActionPriority::Reset,
            Action::ComputeField { .. } => ActionPriority::Transform, // Will be handled specially
            Action::DropMessage | Action::PassThrough => ActionPriority::Control,
            _ => ActionPriority::Transform,
        }
    }

    fn needs_pre_computation(&self) -> bool {
        matches!(self, Action::ComputeField { .. })
    }
}

pub struct RuleProcessor {
    name: String,
    config: RuleConfig,
    timing: TimingMixin,
    /// The `compute_field` expressions, parsed once, by their source.
    expressions: HashMap<String, Expression>,
}

impl RuleProcessor {
    pub fn new(name: &str, config: StageConfig) -> Result<Box<dyn Processor>> {
        let processor_config = RuleConfig::from_stage_config(&config)?;
        processor_config.validate()?;

        // Create timing mixin from processor configuration
        let timing = TimingMixin::new(processor_config.timing.as_ref());
        let expressions = parse_expressions(&processor_config.rules)?;

        Ok(Box::new(Self {
            name: name.to_string(),
            config: processor_config,
            timing,
            expressions,
        }))
    }

    fn execute_action(&self, payload: &mut Value, action: &Action) -> Result<()> {
        let result = match action {
            Action::SetField { field_path, value } => {
                debug!("Setting field '{}' to {:?}", field_path, value);
                FieldUtils::set_field_value(payload, field_path, value.clone())
            }
            Action::RemoveField { field_path } => {
                debug!("Removing field '{}'", field_path);
                FieldUtils::remove_field_value(payload, field_path)
            }
            Action::CopyField {
                source_field,
                target_field,
            } => {
                debug!("Copying field '{}' to '{}'", source_field, target_field);
                if let Some(source_value) = FieldUtils::extract_field_value(payload, source_field) {
                    FieldUtils::set_field_value(payload, target_field, source_value.clone())
                } else {
                    let err = anyhow!(
                        "Source field '{}' not found for copy operation",
                        source_field
                    );
                    return self.handle_action_error(err, action);
                }
            }
            Action::RenameField {
                old_field,
                new_field,
            } => {
                debug!("Renaming field '{}' to '{}'", old_field, new_field);
                if let Some(value) = FieldUtils::extract_field_value(payload, old_field) {
                    let value_clone = value.clone();
                    match FieldUtils::set_field_value(payload, new_field, value_clone) {
                        Ok(_) => FieldUtils::remove_field_value(payload, old_field),
                        Err(e) => Err(e),
                    }
                } else {
                    let err = anyhow!("Field '{}' not found for rename operation", old_field);
                    return self.handle_action_error(err, action);
                }
            }
            Action::ComputeField {
                field_path,
                expression,
            } => {
                debug!(
                    "Computing field '{}' with expression '{}'",
                    field_path, expression
                );
                match self.evaluate_expression(payload, expression) {
                    Ok(result) => FieldUtils::set_field_value(
                        payload,
                        field_path,
                        Value::Number(Number::from_f64(result).unwrap_or(Number::from(0))),
                    ),
                    Err(e) => {
                        return self.handle_action_error(e, action);
                    }
                }
            }
            Action::DropMessage => {
                debug!("Drop message action encountered");
                Ok(())
            }
            Action::PassThrough => {
                debug!("Pass through action - no modifications");
                Ok(())
            }
            Action::KeepOnlyFields { .. } => {
                debug!("KeepOnlyFields action - handled in execute_actions");
                Ok(())
            }
        };

        match result {
            Ok(_) => Ok(()),
            Err(e) => self.handle_action_error(e, action),
        }
    }

    fn handle_action_error(&self, error: anyhow::Error, action: &Action) -> Result<()> {
        match &self.config.error_strategy {
            ErrorStrategy::Continue => {
                error!("Action {:?} failed: {} (continuing)", action, error);
                Ok(())
            }
            ErrorStrategy::Skip => {
                warn!("Skipping action {:?} due to error: {}", action, error);
                Ok(())
            }
            ErrorStrategy::Abort => {
                error!("Aborting message processing due to action error: {}", error);
                Err(error)
            }
            ErrorStrategy::UseDefault => {
                warn!(
                    "Action {:?} failed: {} (using default behavior)",
                    action, error
                );
                Ok(())
            }
        }
    }

    fn keep_only_fields(&self, payload: &mut Value, field_paths: &[String]) -> Result<()> {
        // Extract all the values we want to keep first
        let mut kept_values = HashMap::new();

        for field_path in field_paths {
            if let Some(value) = FieldUtils::extract_field_value(payload, field_path) {
                kept_values.insert(field_path.clone(), value.clone());
            } else {
                warn!("Field '{}' not found while keeping fields", field_path);
            }
        }

        // Clear the payload and rebuild it with only the kept fields
        *payload = Value::Object(serde_json::Map::new());

        // Set each kept field back into the payload
        for (field_path, value) in kept_values {
            FieldUtils::set_field_value(payload, &field_path, value)?;
        }

        debug!("Kept {} fields: {:?}", field_paths.len(), field_paths);
        Ok(())
    }

    fn evaluate_expression(&self, payload: &Value, expression: &str) -> Result<f64> {
        let expression = self
            .expressions
            .get(expression)
            .ok_or_else(|| anyhow!("expression '{}' was not parsed", expression))?;
        let mut variables = Variables::new();
        variables.add_payload("", payload);
        expression.eval_number(&variables).inspect_err(|error| {
            error!("Failed to evaluate expression: {}", error);
        })
    }

    fn process_message(&self, mut message: Message) -> Result<Option<Message>> {
        let mut should_drop = false;

        for rule in &self.config.rules {
            let matched;
            let active_actions;

            if rule.condition.matches(&message.payload) {
                debug!("Rule condition matched for message from {}", message.source);
                matched = true;
                active_actions = &rule.actions;
            } else if !rule.else_actions.is_empty() {
                debug!(
                    "Rule condition not matched, executing else_actions for message from {}",
                    message.source
                );
                matched = false;
                active_actions = &rule.else_actions;
            } else {
                continue;
            }

            if let Err(e) = self.execute_actions(&mut message.payload, active_actions) {
                match &self.config.error_strategy {
                    ErrorStrategy::Abort => {
                        error!("Aborting current message after action failure: {}", e);
                        return Ok(None);
                    }
                    _ => {
                        error!("Failed to execute rule actions: {}", e);
                    }
                }
            }

            if active_actions
                .iter()
                .any(|action| matches!(action, Action::DropMessage))
            {
                should_drop = true;
                debug!(
                    "Rule {} branch requested message drop",
                    if matched { "action" } else { "else_action" }
                );
            }

            if should_drop {
                break;
            }
        }

        if should_drop {
            debug!("Message dropped due to drop_message action");
            return Ok(None);
        }

        // Update the source to indicate transformation
        message.source = self.name.clone();
        Ok(Some(message))
    }

    fn execute_actions(&self, payload: &mut Value, actions: &[Action]) -> Result<()> {
        // Pre-computation phase: evaluate all compute_field expressions before any destructive operations
        let mut computed_values = HashMap::new();
        for action in actions {
            if action.needs_pre_computation() {
                if let Action::ComputeField {
                    field_path,
                    expression,
                } = action
                {
                    debug!(
                        "Pre-computing field '{}' with expression '{}'",
                        field_path, expression
                    );
                    match self.evaluate_expression(payload, expression) {
                        Ok(result) => {
                            computed_values.insert(field_path.clone(), result);
                            debug!("Pre-computed '{}' = {}", field_path, result);
                        }
                        Err(e) => {
                            let error = anyhow!(
                                "Failed to pre-compute field '{}' with expression '{}': {}",
                                field_path,
                                expression,
                                e
                            );

                            self.handle_action_error(error, action)?;

                            if matches!(&self.config.error_strategy, ErrorStrategy::UseDefault) {
                                computed_values.insert(field_path.clone(), 0.0);
                            }
                        }
                    }
                }
            }
        }

        // Sort actions by priority: KeepOnlyFields first, then transform actions, then control actions
        let mut sorted_actions: Vec<_> = actions.iter().collect();
        sorted_actions.sort_by(|a, b| {
            a.priority()
                .partial_cmp(&b.priority())
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Execute actions in priority order
        for action in sorted_actions {
            match action {
                Action::ComputeField { field_path, .. } => {
                    // Use pre-computed value instead of re-evaluating
                    if let Some(computed_value) = computed_values.get(field_path) {
                        debug!(
                            "Setting pre-computed field '{}' to {}",
                            field_path, computed_value
                        );
                        FieldUtils::set_field_value(
                            payload,
                            field_path,
                            Value::Number(
                                Number::from_f64(*computed_value).unwrap_or(Number::from(0)),
                            ),
                        )?;
                    } else {
                        debug!(
                            "Skipping compute_field '{}' after pre-compute failure",
                            field_path
                        );
                    }
                }
                Action::KeepOnlyFields { field_paths } => {
                    debug!(
                        "Applying destructive reset - keeping only fields: {:?}",
                        field_paths
                    );
                    self.keep_only_fields(payload, field_paths)?;
                }
                _ => {
                    // Handle all other actions normally
                    self.execute_action(payload, action)?;
                }
            }
        }

        Ok(())
    }
}

#[async_trait::async_trait]
impl Processor for RuleProcessor {
    async fn init(&mut self) -> Result<()> {
        tracing::info!(
            "Rule processor '{}' initialised with timing semantics and {} rules",
            self.name,
            self.config.rules.len()
        );
        Ok(())
    }

    async fn process(&mut self, context: &mut ProcessingContext) -> Result<()> {
        let input_messages =
            drain_bounded_each(&mut context.inputs, DEFAULT_MAX_DRAIN_PER_INPUT).await;

        for input_message in &input_messages {
            if self.timing.should_drop_message(&input_message.message) {
                tracing::debug!(
                    "Message from '{}' was dropped by timing constraints",
                    input_message.input_name
                );
                continue;
            }

            match self.process_message(input_message.message.clone()) {
                Ok(Some(transformed_message)) => {
                    if let Some(output_info) = &context.output {
                        // Preserve timing information when forwarding
                        let output_message = Message {
                            source: transformed_message.source,
                            topic: output_info.name.clone(),
                            payload: transformed_message.payload,
                            timestamp: transformed_message.timestamp,
                            timing: transformed_message.timing,
                        };

                        // Update watermark using timing mixin
                        let output_message = self.timing.update_message_watermark(output_message);

                        if let Err(e) = output_info.channel.publish(output_message).await {
                            tracing::warn!("Failed to publish transformed message: {:?}", e);
                        } else {
                            tracing::debug!(
                                "Message from '{}' transformed and forwarded",
                                input_message.input_name
                            );
                        }
                    }
                }
                Ok(None) => {
                    tracing::debug!(
                        "Message from '{}' was dropped by rule processor",
                        input_message.input_name
                    );
                }
                Err(e) => {
                    error!("Failed to transform message: {}", e);
                }
            }
        }

        if input_messages.is_empty() {
            idle_sleep().await;
        }

        Ok(())
    }
}

/// Parses every `compute_field` expression of the rules, keyed by its source.
fn parse_expressions(rules: &[Rule]) -> Result<HashMap<String, Expression>> {
    let mut expressions = HashMap::new();
    let actions = rules
        .iter()
        .flat_map(|rule| rule.actions.iter().chain(&rule.else_actions));
    for action in actions {
        if let Action::ComputeField { expression, .. } = action
            && !expressions.contains_key(expression)
        {
            expressions.insert(expression.clone(), Expression::parse(expression)?);
        }
    }
    Ok(expressions)
}

impl WithTimingMixin for RuleProcessor {
    fn timing_mixin(&self) -> &TimingMixin {
        &self.timing
    }

    fn timing_mixin_mut(&mut self) -> &mut TimingMixin {
        &mut self.timing
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn stage_config(parameters: Option<HashMap<String, Value>>) -> StageConfig {
        StageConfig {
            r#type: "rule".to_string(),
            inputs: Some(vec!["raw".to_string()]),
            output: Some("filtered".to_string()),
            concurrency: None,
            channel: None,
            timing: None,
            parameters,
        }
    }

    fn minimal_rule() -> Value {
        json!({
            "condition": {
                "field_path": "value",
                "operation": ">",
                "value": 10
            },
            "actions": [
                { "type": "pass_through" }
            ],
            "else_actions": []
        })
    }

    fn processor_with(rules: Vec<Rule>, error_strategy: ErrorStrategy) -> RuleProcessor {
        RuleProcessor {
            name: "rule_test".to_string(),
            expressions: parse_expressions(&rules).expect("test expressions parse"),
            config: RuleConfig {
                rules,
                error_strategy,
                timing: None,
            },
            timing: TimingMixin::new(None),
        }
    }

    fn condition(operation: &str) -> Condition {
        Condition {
            field_path: String::new(),
            operation: operation.to_string(),
            value: Value::Null,
        }
    }

    #[test]
    fn rule_config_requires_rules_parameter() {
        let error = RuleConfig::from_stage_config(&stage_config(Some(HashMap::new())))
            .expect_err("missing rules is rejected");

        assert!(error.to_string().contains("rules"));
    }

    #[test]
    fn rule_config_rejects_invalid_error_strategy() {
        let config = stage_config(Some(HashMap::from([
            ("rules".to_string(), json!([minimal_rule()])),
            ("error_strategy".to_string(), json!("shrug")),
        ])));

        let error =
            RuleConfig::from_stage_config(&config).expect_err("invalid error strategy is rejected");

        assert!(error.to_string().contains("error_strategy"));
    }

    #[test]
    fn always_condition_runs_actions_without_field_path() {
        let processor = processor_with(
            vec![Rule {
                condition: condition("always"),
                actions: vec![Action::SetField {
                    field_path: "status".to_string(),
                    value: json!("seen"),
                }],
                else_actions: vec![],
            }],
            ErrorStrategy::Abort,
        );

        let output = processor
            .process_message(Message::new("source", "raw", json!({})))
            .expect("message processes")
            .expect("message is forwarded");

        assert_eq!(output.payload["status"], json!("seen"));
    }

    #[test]
    fn never_condition_runs_else_actions_without_field_path() {
        let processor = processor_with(
            vec![Rule {
                condition: condition("never"),
                actions: vec![Action::SetField {
                    field_path: "status".to_string(),
                    value: json!("action"),
                }],
                else_actions: vec![Action::SetField {
                    field_path: "status".to_string(),
                    value: json!("else"),
                }],
            }],
            ErrorStrategy::Abort,
        );

        let output = processor
            .process_message(Message::new("source", "raw", json!({})))
            .expect("message processes")
            .expect("message is forwarded");

        assert_eq!(output.payload["status"], json!("else"));
    }

    #[test]
    fn ordinary_condition_requires_field_path() {
        let config = RuleConfig {
            rules: vec![Rule {
                condition: Condition {
                    field_path: String::new(),
                    operation: "equals".to_string(),
                    value: json!(true),
                },
                actions: vec![Action::PassThrough],
                else_actions: vec![],
            }],
            error_strategy: ErrorStrategy::Continue,
            timing: None,
        };

        let error = config
            .validate()
            .expect_err("missing field path is rejected");

        assert!(error.to_string().contains("field_path"));
    }

    #[test]
    fn abort_strategy_drops_current_message_on_action_error() {
        let processor = processor_with(
            vec![Rule {
                condition: condition("always"),
                actions: vec![Action::CopyField {
                    source_field: "missing".to_string(),
                    target_field: "copied".to_string(),
                }],
                else_actions: vec![],
            }],
            ErrorStrategy::Abort,
        );

        let output = processor
            .process_message(Message::new("source", "raw", json!({"kept": true})))
            .expect("abort is contained to the current message");

        assert!(output.is_none());
    }

    #[test]
    fn continue_strategy_keeps_message_after_action_error() {
        let processor = processor_with(
            vec![Rule {
                condition: condition("always"),
                actions: vec![Action::CopyField {
                    source_field: "missing".to_string(),
                    target_field: "copied".to_string(),
                }],
                else_actions: vec![],
            }],
            ErrorStrategy::Continue,
        );

        let output = processor
            .process_message(Message::new("source", "raw", json!({"kept": true})))
            .expect("message processes")
            .expect("message is forwarded");

        assert_eq!(output.payload["kept"], json!(true));
        assert!(output.payload.get("copied").is_none());
    }

    #[test]
    fn use_default_strategy_sets_failed_compute_to_zero() {
        let processor = processor_with(
            vec![Rule {
                condition: condition("always"),
                actions: vec![Action::ComputeField {
                    field_path: "computed".to_string(),
                    expression: "missing + 1".to_string(),
                }],
                else_actions: vec![],
            }],
            ErrorStrategy::UseDefault,
        );

        let output = processor
            .process_message(Message::new("source", "raw", json!({})))
            .expect("message processes")
            .expect("message is forwarded");

        assert_eq!(output.payload["computed"], json!(0.0));
    }

    #[test]
    fn rule_condition_can_read_array_path() {
        let processor = processor_with(
            vec![Rule {
                condition: Condition {
                    field_path: "readings[1].kind".to_string(),
                    operation: "equals".to_string(),
                    value: json!("target"),
                },
                actions: vec![Action::SetField {
                    field_path: "matched".to_string(),
                    value: json!(true),
                }],
                else_actions: vec![],
            }],
            ErrorStrategy::Abort,
        );

        let output = processor
            .process_message(Message::new(
                "source",
                "raw",
                json!({
                    "readings": [
                        { "kind": "other" },
                        { "kind": "target" }
                    ]
                }),
            ))
            .expect("message processes")
            .expect("message is forwarded");

        assert_eq!(output.payload["matched"], json!(true));
    }

    #[test]
    fn rule_actions_can_mutate_array_paths() {
        let processor = processor_with(
            vec![Rule {
                condition: condition("always"),
                actions: vec![
                    Action::SetField {
                        field_path: "readings[0].status".to_string(),
                        value: json!("ok"),
                    },
                    Action::CopyField {
                        source_field: "readings[0].status".to_string(),
                        target_field: "summary.first_status".to_string(),
                    },
                    Action::RemoveField {
                        field_path: "readings[1].noise".to_string(),
                    },
                ],
                else_actions: vec![],
            }],
            ErrorStrategy::Abort,
        );

        let output = processor
            .process_message(Message::new(
                "source",
                "raw",
                json!({
                    "readings": [
                        {},
                        { "noise": true, "value": 5 }
                    ]
                }),
            ))
            .expect("message processes")
            .expect("message is forwarded");

        assert_eq!(output.payload["readings"][0]["status"], json!("ok"));
        assert_eq!(output.payload["summary"]["first_status"], json!("ok"));
        assert!(output.payload["readings"][1].get("noise").is_none());
    }

    #[test]
    fn keep_only_fields_can_preserve_array_paths() {
        let processor = processor_with(
            vec![Rule {
                condition: condition("always"),
                actions: vec![Action::KeepOnlyFields {
                    field_paths: vec!["readings[1].value".to_string()],
                }],
                else_actions: vec![],
            }],
            ErrorStrategy::Abort,
        );

        let output = processor
            .process_message(Message::new(
                "source",
                "raw",
                json!({
                    "readings": [
                        { "value": 1 },
                        { "value": 2 }
                    ],
                    "ignored": true
                }),
            ))
            .expect("message processes")
            .expect("message is forwarded");

        assert_eq!(output.payload["readings"][1]["value"], json!(2));
        assert!(output.payload.get("ignored").is_none());
    }

    #[test]
    fn rule_validation_rejects_malformed_field_path() {
        let config = RuleConfig {
            rules: vec![Rule {
                condition: Condition {
                    field_path: "readings[abc]".to_string(),
                    operation: "equals".to_string(),
                    value: json!(true),
                },
                actions: vec![Action::PassThrough],
                else_actions: vec![],
            }],
            error_strategy: ErrorStrategy::Continue,
            timing: None,
        };

        let error = config.validate().expect_err("malformed path is rejected");

        assert!(error.to_string().contains("field_path"));
    }

    #[test]
    fn expression_context_exposes_array_paths() {
        let processor = processor_with(
            vec![Rule {
                condition: condition("always"),
                actions: vec![Action::ComputeField {
                    field_path: "next".to_string(),
                    expression: "readings[0].value + 1".to_string(),
                }],
                else_actions: vec![],
            }],
            ErrorStrategy::Abort,
        );

        let output = processor
            .process_message(Message::new(
                "source",
                "raw",
                json!({ "readings": [{ "value": 4 }] }),
            ))
            .expect("message processes")
            .expect("message is forwarded");

        assert_eq!(output.payload["next"], json!(5.0));
    }

    #[test]
    fn compute_field_functions_work_bare_and_namespaced() {
        let processor = processor_with(
            vec![Rule {
                condition: condition("always"),
                actions: vec![Action::ComputeField {
                    field_path: "result".to_string(),
                    expression: "floor(x) + math::sqrt(x) + log * 1.0".to_string(),
                }],
                else_actions: vec![],
            }],
            ErrorStrategy::Abort,
        );

        let output = processor
            .process_message(Message::new(
                "source",
                "raw",
                json!({ "x": 16.5, "log": 2.0 }),
            ))
            .expect("message processes")
            .expect("message is forwarded");

        let expected = 16.0 + 16.5f64.sqrt() + 2.0;
        assert_eq!(output.payload["result"], json!(expected));
    }

    #[test]
    fn rule_validation_rejects_expression_syntax_errors() {
        let config = RuleConfig {
            rules: vec![Rule {
                condition: condition("always"),
                actions: vec![Action::ComputeField {
                    field_path: "broken".to_string(),
                    expression: "(value + 1.0".to_string(),
                }],
                else_actions: vec![],
            }],
            error_strategy: ErrorStrategy::Continue,
            timing: None,
        };

        let error = config.validate().expect_err("syntax error is rejected");

        assert!(error.to_string().contains("invalid expression"), "{error}");
    }
}
