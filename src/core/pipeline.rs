use super::registry::ChannelRegistry;
use super::runtime_event::{RuntimeEvent, RuntimeEventKind};
use super::runtime_observer::{SharedRuntimeObserver, noop_runtime_observer};
use super::commands::CommandRouter;
use super::stage::{ControlMessage, Stage, create_stage};
use crate::config::{Config, StageConfig};
use crate::core::channel::PubSubChannel;
use crate::core::message::Message;

use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Represents a pipeline consisting of multiple stages.
#[allow(dead_code)]
struct Pipeline {
    name: String,
    description: String,
    stage_names: Vec<String>,
}

/// Manages the creation and connection of stages and pipelines.
pub struct PipelineManager {
    config: Config,
    stages: HashMap<String, Arc<Mutex<Box<Stage>>>>,
    pipelines: HashMap<String, Pipeline>,
    channel_registry: ChannelRegistry<Message>,
    control_channel: Option<Arc<tokio::sync::broadcast::Sender<ControlMessage>>>,
    stage_handles: HashMap<String, tokio::task::JoinHandle<()>>,
    observer: SharedRuntimeObserver,
    commands: CommandRouter,
}

impl PipelineManager {
    /// Create a new `PipelineManager` with the given configuration.
    ///
    /// # Arguments
    ///
    /// * `config` - The configuration for the pipeline manager.
    ///
    /// # Returns
    ///
    /// A new instance of `PipelineManager`.
    pub fn new(config: Config) -> Self {
        Self {
            config,
            stages: HashMap::new(),
            pipelines: HashMap::new(),
            channel_registry: ChannelRegistry::new(),
            control_channel: None,
            stage_handles: HashMap::new(),
            observer: noop_runtime_observer(),
            commands: CommandRouter::default(),
        }
    }

    /// Attach an observer that receives structured runtime events.
    pub fn with_runtime_observer(mut self, observer: SharedRuntimeObserver) -> Self {
        self.observer = observer;
        self
    }

    /// Get all stage configurations from the config.
    fn get_all_stage_configs(&self) -> Vec<(String, StageConfig)> {
        let mut all_stages = Vec::new();

        for (stage_name, stage_config) in &self.config.inputs {
            all_stages.push((stage_name.clone(), stage_config.clone()));
        }

        for (_, pipeline_config) in &self.config.pipelines {
            for (stage_name, stage_config) in &pipeline_config.stages {
                all_stages.push((stage_name.clone(), stage_config.clone()));
            }
        }

        for (stage_name, stage_config) in &self.config.outputs {
            all_stages.push((stage_name.clone(), stage_config.clone()));
        }

        all_stages
    }

    /// Check if all inputs for a stage are available in the channel registry.
    fn are_all_inputs_available(
        channel_registry: &ChannelRegistry<Message>,
        stage_config: &StageConfig,
    ) -> Result<bool> {
        Ok(stage_config
            .inputs
            .as_ref()
            .map(|inputs| {
                inputs
                    .iter()
                    .all(|input| channel_registry.get(input).is_some())
            })
            .unwrap_or(true))
    }

    /// Map inputs from the stage configuration to the channel registry.
    async fn map_inputs(
        channel_registry: &mut ChannelRegistry<Message>,
        stage: &Arc<Mutex<Box<Stage>>>,
        stage_config: &StageConfig,
    ) -> Result<()> {
        if let Some(inputs) = &stage_config.inputs {
            for input_name in inputs {
                if let Some(channel) = channel_registry.get(input_name) {
                    let subscriber = channel.subscribe();
                    stage.lock().await.add_input(input_name, subscriber).await;
                } else {
                    return Err(anyhow::anyhow!("Input channel '{}' not found", input_name));
                }
            }
        }

        Ok(())
    }

    /// Create an output channel for the stage if specified in the configuration.
    async fn create_output(
        channel_registry: &mut ChannelRegistry<Message>,
        stage: &Arc<Mutex<Box<Stage>>>,
        stage_config: &StageConfig,
    ) -> Result<()> {
        if let Some(output_name) = &stage_config.output {
            let channel_config = stage_config.channel.clone().unwrap_or_default();
            let channel = channel_registry.get_or_create(
                output_name,
                channel_config.r#type.clone(),
                channel_config.capacity,
            );

            stage
                .lock()
                .await
                .add_output(&output_name, channel.clone())
                .await;
        }

        Ok(())
    }

    /// Try to connect a stage by mapping its inputs and creating its output.
    async fn try_connect_stage(
        &mut self,
        stage_name: &str,
        stage_config: &StageConfig,
    ) -> Result<()> {
        let stage = self
            .stages
            .get_mut(stage_name)
            .ok_or_else(|| anyhow::anyhow!("Stage not found: '{}'", stage_name))?;

        if !Self::are_all_inputs_available(&self.channel_registry, stage_config)? {
            return Err(anyhow::anyhow!(
                "Inputs not available for stage: {}",
                stage_name
            ));
        }

        Self::map_inputs(&mut self.channel_registry, stage, stage_config).await?;

        Self::create_output(&mut self.channel_registry, stage, stage_config).await?;

        Ok(())
    }

    /// Resolve deferred stages by checking if their dependencies are met.
    async fn resolve_deferred_stages(
        &mut self,
        mut deferred_stages: Vec<(String, StageConfig)>,
    ) -> Result<()> {
        let mut progress_made = true;

        while !deferred_stages.is_empty() && progress_made {
            progress_made = false;

            let mut new_deferred_stages = Vec::new();

            for (stage_name, stage_config) in deferred_stages.into_iter() {
                if self
                    .try_connect_stage(&stage_name, &stage_config)
                    .await
                    .is_err()
                {
                    new_deferred_stages.push((stage_name, stage_config));
                } else {
                    progress_made = true;
                }
            }

            deferred_stages = new_deferred_stages;
        }

        if !deferred_stages.is_empty() {
            return Err(anyhow::anyhow!(
                "Unmet or circular dependencies detected in stages: {:?}",
                deferred_stages
                    .iter()
                    .map(|(name, _)| name)
                    .collect::<Vec<_>>()
            ));
        }

        Ok(())
    }

    /// Connect all stages by resolving their dependencies.
    pub async fn connect_stages(mut self) -> Result<Self> {
        let all_stages = self.get_all_stage_configs();
        let mut deferred_stages = Vec::new();

        for (stage_name, stage_config) in all_stages {
            if let Err(_) = self.try_connect_stage(&stage_name, &stage_config).await {
                deferred_stages.push((stage_name, stage_config));
            }
        }

        self.resolve_deferred_stages(deferred_stages).await?;

        Ok(self)
    }

    /// Create stages based on the provided stage configurations.
    fn create_stages(
        stage_configs: &HashMap<String, StageConfig>,
    ) -> Result<HashMap<String, Arc<Mutex<Box<Stage>>>>> {
        let mut stages: HashMap<String, Arc<Mutex<Box<Stage>>>> = HashMap::new();

        for (stage_name, stage_config) in stage_configs {
            tracing::debug!(stage = %stage_name, config = ?stage_config, "Creating stage");

            // Use the type as name of the stage
            // if let Some(stage) = create_stage(&stage_config.r#type, stage_config.clone()) {

            let stage = create_stage(&stage_name, stage_config.clone()).map_err(|error| {
                anyhow::anyhow!("Failed to create stage '{}': {}", stage_name, error)
            })?;
            stages.insert(stage_name.clone(), Arc::new(Mutex::new(stage)));
        }

        Ok(stages)
    }

    /// Create pipelines and their stages based on the provided pipeline configurations.
    fn create_pipelines(
        &mut self,
    ) -> Result<(
        HashMap<String, Arc<Mutex<Box<Stage>>>>,
        HashMap<String, Pipeline>,
    )> {
        let mut pipelines = HashMap::new();
        let mut stages = HashMap::new();

        for (pipeline_name, pipeline_config) in &self.config.pipelines {
            let created_stages = Self::create_stages(&pipeline_config.stages)?;

            stages.extend(created_stages);

            let pipeline = Pipeline {
                name: pipeline_name.clone(),
                description: pipeline_config.description.clone(),
                stage_names: pipeline_config.stages.keys().cloned().collect(),
            };

            pipelines.insert(pipeline_name.clone(), pipeline);
        }

        Ok((stages, pipelines))
    }

    /// Build all stages and pipelines based on the provided configuration.
    pub fn build_all(mut self) -> Result<Self> {
        // Create input stages
        let input_stages = Self::create_stages(&self.config.inputs)?;
        self.stages.extend(input_stages);

        // Create output stages
        let output_stages = Self::create_stages(&self.config.outputs)?;
        self.stages.extend(output_stages);

        // Create pipelines and pipeline stages
        let (pipeline_stages, pipelines) = self.create_pipelines()?;
        self.stages.extend(pipeline_stages);
        self.pipelines.extend(pipelines);

        // Create the control channel
        let (control_channel, _) = tokio::sync::broadcast::channel::<ControlMessage>(128);
        self.control_channel = Some(Arc::new(control_channel));

        for stage in self.stages.values() {
            if let Ok(mut stage) = stage.try_lock() {
                stage.set_observer(self.observer.clone());
            }
        }

        Ok(self)
    }

    /// Start all stages in the pipeline.
    pub async fn start_all(mut self) -> Result<Self> {
        tracing::info!("Starting all stages");
        let all_stages = self.get_all_stage_configs();
        self.observer.emit(
            RuntimeEvent::new(RuntimeEventKind::PipelineStarting)
                .text(format!("{} stages", all_stages.len())),
        );

        for (stage_name, stage_config) in all_stages {
            if let Some(stage) = self.stages.get_mut(&stage_name) {
                // Setup stage and wire control channel
                {
                    let stage_clone = Arc::clone(stage);
                    let mut stage = stage_clone.lock().await;
                    stage.set_observer(self.observer.clone());

                    // Attach the control channel if available
                    if let Some(control_channel) = &self.control_channel {
                        stage.attach_control_channel(control_channel.subscribe());
                    }

                    // Stages that accept commands get their own command channel
                    if let Some(sender) = stage.open_command_channel() {
                        self.commands.insert(stage_name.clone(), sender);
                    }

                    self.observer.emit(
                        RuntimeEvent::new(RuntimeEventKind::StageStarting)
                            .stage(stage_name.clone())
                            .processor_type(stage_config.r#type.clone()),
                    );

                    // Initialise stage (and processor)
                    if let Err(error) = stage.init().await {
                        self.observer.emit(
                            RuntimeEvent::new(RuntimeEventKind::ProcessorError)
                                .stage(stage_name.clone())
                                .processor_type(stage_config.r#type.clone())
                                .text(error.to_string()),
                        );
                        return Err(error);
                    }
                }

                // Run the stage
                {
                    let stage_clone = Arc::clone(stage);
                    let stage_name_clone = stage_name.clone();
                    let processor_type = stage_config.r#type.clone();
                    let observer = self.observer.clone();

                    // Spawn a new task to run the stage
                    let handle = tokio::spawn(async move {
                        let mut stage_lock = stage_clone.lock().await;
                        if let Err(e) = stage_lock.run().await {
                            tracing::error!("Error running stage [{}]: {}", stage_name_clone, e);
                        }
                        observer.emit(
                            RuntimeEvent::new(RuntimeEventKind::StageStopped)
                                .stage(stage_name_clone)
                                .processor_type(processor_type),
                        );
                    });

                    self.stage_handles.insert(stage_name, handle);
                }
            }
        }

        self.observer.emit(
            RuntimeEvent::new(RuntimeEventKind::PipelineStarted)
                .text(format!("{} stages running", self.stage_handles.len())),
        );

        // futures::future::pending().await;
        Ok(self)
    }

    /// Routes commands to the running stages that accept them (call after `start_all`).
    pub fn command_router(&self) -> CommandRouter {
        self.commands.clone()
    }

    /// Wait for all stages to complete and handle termination signals.
    pub async fn wait_for_all(self) -> Result<()> {
        let observer = self.observer.clone();
        let control_channel_clone = self.control_channel.clone();

        // Listen for Ctrl+C signal
        tokio::spawn(async move {
            if let Err(e) = tokio::signal::ctrl_c().await {
                tracing::error!("Failed to listen for Ctrl + C: {}", e);
            }

            tracing::info!("Received Ctrl+C -> shutting down.");
            let _ = control_channel_clone
                .as_ref()
                .unwrap()
                .send(ControlMessage::Terminate);
        });

        let handles: Vec<_> = self.stage_handles.into_values().collect();

        // Wait for all stage handles to complete
        futures::future::join_all(handles).await;

        observer.emit(RuntimeEvent::new(RuntimeEventKind::PipelineStopped));

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::runtime_observer::tests::RecordingRuntimeObserver;

    #[tokio::test]
    async fn emits_pipeline_lifecycle_events() {
        let observer = Arc::new(RecordingRuntimeObserver::default());

        PipelineManager::new(Config::default())
            .with_runtime_observer(observer.clone())
            .build_all()
            .expect("empty pipeline builds")
            .start_all()
            .await
            .expect("empty pipeline starts")
            .wait_for_all()
            .await
            .expect("empty pipeline stops");

        let kinds: Vec<_> = observer
            .events()
            .into_iter()
            .map(|event| event.kind)
            .collect();

        assert_eq!(
            kinds,
            vec![
                RuntimeEventKind::PipelineStarting,
                RuntimeEventKind::PipelineStarted,
                RuntimeEventKind::PipelineStopped,
            ]
        );
    }
}
