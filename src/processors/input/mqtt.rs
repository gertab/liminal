use crate::config::{
    FieldConfig, ProcessorConfig, StageConfig, defaulted_param, extract_field_params,
};
use crate::core::context::ProcessingContext;
use crate::core::timing_mixin::{TimingMixin, WithTimingMixin};
use crate::processors::Processor;
use crate::processors::common::MqttConnectionConfig;

use async_trait::async_trait;
use rumqttc::{AsyncClient, Event, Packet};
use serde_json::Value;
use tokio::sync::Mutex;
use tokio::time::Duration;

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};

#[derive(Debug, Clone)]
pub struct MqttInputConfig {
    pub connection: MqttConnectionConfig,
    pub topics: Vec<String>,
    pub field: FieldConfig,
    pub timing: Option<crate::config::TimingConfig>,
}

impl ProcessorConfig for MqttInputConfig {
    fn from_stage_config(config: &StageConfig) -> anyhow::Result<Self> {
        let connection = MqttConnectionConfig::from_parameters(&config.parameters, "liminal")?;
        let topics: Vec<String> =
            defaulted_param(&config.parameters, "topics", vec!["#".to_string()])?;

        // |KB|Todo: Field configuration will be removed. Any payload parameter renaming should be handled by
        // a separate rename processor. Will be changing this to None in the future.
        let field_config = extract_field_params(&config.parameters);

        // Extract timing configuration
        let timing_config = config.timing.clone();

        Ok(Self {
            connection,
            topics,
            field: field_config,
            timing: timing_config,
        })
    }

    fn validate(&self) -> anyhow::Result<()> {
        self.connection.validate()?;
        if self.topics.is_empty() {
            return Err(anyhow::anyhow!("At least one topic must be specified"));
        }
        Ok(())
    }
}

pub struct MqttInputProcessor {
    name: String,
    config: MqttInputConfig,
    timing: TimingMixin,
    client: Option<AsyncClient>,
    event_loop: Option<Mutex<rumqttc::EventLoop>>,
}

impl MqttInputProcessor {
    pub fn new(name: &str, config: StageConfig) -> anyhow::Result<Box<dyn Processor>> {
        let processor_config = MqttInputConfig::from_stage_config(&config)?;
        processor_config.validate()?;

        // Create timing mixin from processor configuration
        let timing = TimingMixin::new(processor_config.timing.as_ref());

        Ok(Box::new(Self {
            name: name.to_string(),
            config: processor_config,
            timing,
            client: None,
            event_loop: None,
        }))
    }
}

#[async_trait]
impl Processor for MqttInputProcessor {
    async fn init(&mut self) -> anyhow::Result<()> {
        let mqttoptions = self.config.connection.create_mqtt_options("liminal")?;
        let (client, mut eventloop) = AsyncClient::new(mqttoptions, 10);

        self.config
            .connection
            .wait_for_connection_ack(&mut eventloop)
            .await?;

        for topic in &self.config.topics {
            client
                .subscribe(topic, self.config.connection.qos())
                .await
                .map_err(|e| anyhow::anyhow!("Failed to subscribe to topic '{}': {}", topic, e))?;
            tracing::info!(
                "Subscribed to MQTT topic: {} (QoS: {})",
                topic,
                self.config.connection.qos
            );
        }
        self.config
            .connection
            .wait_for_subscription_acks(&mut eventloop, self.config.topics.len())
            .await?;

        self.client = Some(client);
        self.event_loop = Some(Mutex::new(eventloop));

        tracing::info!("Field configuration: {:?}", self.config.field);
        tracing::info!(
            "MQTT subscriber '{}' initialised with timing semantics",
            self.name
        );

        Ok(())
    }

    async fn process(&mut self, context: &mut ProcessingContext) -> anyhow::Result<()> {
        if let Some(ref event_loop_mutex) = self.event_loop {
            // |KB| Changing logic to poll under the lock but then drop it before
            // any downstram awaits, to avoid convoying stages.
            let (maybe_topic, maybe_payload_bytes) = {
                let mut eventloop = event_loop_mutex.lock().await;

                tokio::select! {
                    event_result = eventloop.poll() => {
                        match event_result {
                            Ok(Event::Incoming(Packet::Publish(publish))) => {
                                (Some(publish.topic.clone()), Some(publish.payload.to_vec()))
                            }
                            Ok(_) => (None, None),
                            Err(e) => {
                                tracing::error!("MQTT connection error: {}", e);
                                (None, None)
                            }
                        }
                    }
                    _ = tokio::time::sleep(Duration::from_millis(100)) => (None, None),
                }
            };

            // Process downstream messages, if any
            if let (Some(topic), Some(payload_bytes)) = (maybe_topic, maybe_payload_bytes) {
                let payload = match serde_json::from_slice::<Value>(&payload_bytes) {
                    Ok(json_value) => json_value,
                    Err(_) => match std::str::from_utf8(&payload_bytes) {
                        Ok(s) => Value::String(s.to_owned()),
                        Err(_) => Value::String(BASE64.encode(&payload_bytes)),
                    },
                };

                tracing::debug!("MQTT '{}' payload: {},", topic, payload);

                if let Some(output_info) = &context.output {
                    // Generate sequence ID and create message with timing semantics
                    let sequence_id = self.timing.next_sequence_id();
                    let message = self
                        .timing
                        .create_message_with_event_time_extraction(
                            &self.name,
                            &output_info.name,
                            payload,
                            std::time::SystemTime::now(),
                        )
                        .with_sequence_id(sequence_id);

                    if let Err(e) = output_info.channel.publish(message).await {
                        tracing::warn!("Downstream publish failed: {:?}", e);
                    } else {
                        tracing::debug!("Received MQTT message from topic: '{}'", topic);
                    }
                }
            }
        }

        Ok(())
    }
}

impl WithTimingMixin for MqttInputProcessor {
    fn timing_mixin(&self) -> &TimingMixin {
        &self.timing
    }

    fn timing_mixin_mut(&mut self) -> &mut TimingMixin {
        &mut self.timing
    }
}
