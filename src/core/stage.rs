use super::channel::PubSubChannel;
use super::channel::Subscriber;
use super::commands::{COMMAND_CHANNEL_CAPACITY, CommandReceiver, CommandSender};
use super::context::ProcessingContext;
use super::message::Message;
use super::runtime_event::{RuntimeEvent, RuntimeEventKind};
use super::runtime_observer::{SharedRuntimeObserver, noop_runtime_observer};

use crate::config::StageConfig;
use crate::processors::processor::Processor;

use anyhow::{Context, Result};
use std::sync::Arc;

/// Creates a new stage with the given name and configuration.
///
/// # Arguments
/// * `name` - The name of the stage.
/// * `config` - The configuration for the stage.
///
/// # Returns
/// A `Box<Stage>` if the stage and its processor were created successfully.
///
pub fn create_stage(name: &str, config: StageConfig) -> Result<Box<Stage>> {
    // Uncomment if the stage name is used as processor type
    // if let Ok(processor) = crate::processors::create_processor(name, config) {

    let processor_type = config.r#type.clone();
    let processor =
        crate::processors::create_processor(&processor_type, config).with_context(|| {
            format!(
                "failed to create processor '{}' for stage '{}'",
                processor_type, name
            )
        })?;

    Ok(Box::new(Stage::new(
        name.to_string(),
        processor_type,
        processor,
        None,
    )))
}

#[derive(Debug, Clone)]
pub enum ControlMessage {
    Terminate,
}

pub struct Stage {
    name: String,
    processor_type: String,
    processor: Box<dyn Processor>,
    context: ProcessingContext,
    control_channel: Option<tokio::sync::broadcast::Receiver<ControlMessage>>,
    commands: Option<CommandReceiver>,
    observer: SharedRuntimeObserver,
}

impl Stage {
    pub fn new(
        name: String,
        processor_type: String,
        processor: Box<dyn Processor>,
        control_channel: Option<tokio::sync::broadcast::Receiver<ControlMessage>>,
    ) -> Self {
        let mut context = ProcessingContext::new(name.clone());
        context.set_runtime_metadata(processor_type.clone(), noop_runtime_observer());

        Self {
            name: name.clone(),
            processor_type,
            processor,
            context,
            control_channel: control_channel,
            commands: None,
            observer: noop_runtime_observer(),
        }
    }

    /// Opens this stage's command channel if its processor accepts commands.
    pub fn open_command_channel(&mut self) -> Option<CommandSender> {
        if !self.processor.accepts_commands() {
            return None;
        }
        let (sender, receiver) = tokio::sync::mpsc::channel(COMMAND_CHANNEL_CAPACITY);
        self.commands = Some(receiver);
        Some(sender)
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn attach_control_channel(
        &mut self,
        control_channel: tokio::sync::broadcast::Receiver<ControlMessage>,
    ) {
        self.control_channel = Some(control_channel);
    }

    pub fn set_observer(&mut self, observer: SharedRuntimeObserver) {
        self.observer = observer.clone();
        self.context
            .set_runtime_metadata(self.processor_type.clone(), observer);
    }

    pub async fn add_input(&mut self, name: &str, input: Subscriber<Message>) {
        self.context.add_input(name.to_string(), input);
    }

    pub async fn add_output(&mut self, name: &str, output: Arc<dyn PubSubChannel<Message>>) {
        self.context.attach_output(name.to_string(), output);
    }

    pub async fn init(&mut self) -> anyhow::Result<()> {
        self.processor.init().await
    }

    pub async fn run(&mut self) -> anyhow::Result<()> {
        tracing::info!("Stage '{}' is running", self.name);
        self.observer.emit(
            RuntimeEvent::new(RuntimeEventKind::StageRunning)
                .stage(self.name.clone())
                .processor_type(self.processor_type.clone()),
        );

        loop {
            let command = tokio::select! {
                // Handle control messages
                Some(message) = async {
                    if let Some(control_channel) = &mut self.control_channel {
                        control_channel.recv().await.ok()
                    } else {
                        None
                    }
                } => {
                    match message {
                        ControlMessage::Terminate => {
                            tracing::info!("Stage '{}' received terminate signal", self.name);
                            break;
                        }
                    }
                }

                // Commands for this stage (only if its processor accepts them)
                Some(command) = async {
                    match &mut self.commands {
                        Some(commands) => commands.recv().await,
                        None => None,
                    }
                } => Some(command),

                // Process messages
                result = self.processor.process(&mut self.context) => {
                    // Handle the result of the processor
                    if let Err(e) = result {
                        tracing::error!("Error in processor for stage '{}': {}", self.name, e);
                        self.observer.emit(
                            RuntimeEvent::new(RuntimeEventKind::ProcessorError)
                                .stage(self.name.clone())
                                .processor_type(self.processor_type.clone())
                                .text(e.to_string()),
                        );
                        return Err(e);
                    }
                    None
                }
            };

            // Handled outside `select!` so `process` is no longer borrowed.
            if let Some(command) = command
                && let Err(error) = self
                    .processor
                    .handle_command(command, &mut self.context)
                    .await
            {
                tracing::warn!("Stage '{}' rejected a command: {:#}", self.name, error);
                self.observer.emit(
                    RuntimeEvent::new(RuntimeEventKind::CommandRejected)
                        .stage(self.name.clone())
                        .processor_type(self.processor_type.clone())
                        .text(format!("{error:#}")),
                );
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::runtime_observer::tests::RecordingRuntimeObserver;
    use async_trait::async_trait;
    use std::sync::Mutex;
    use std::time::Duration;

    /// Counts `process` calls and records commands; rejects `{"bad": true}`.
    struct Interactive {
        accepts: bool,
        processed: Arc<Mutex<u32>>,
        commands: Arc<Mutex<Vec<serde_json::Value>>>,
    }

    #[async_trait]
    impl Processor for Interactive {
        async fn init(&mut self) -> anyhow::Result<()> {
            Ok(())
        }

        async fn process(&mut self, _context: &mut ProcessingContext) -> anyhow::Result<()> {
            tokio::time::sleep(Duration::from_millis(5)).await;
            *self.processed.lock().unwrap() += 1;
            Ok(())
        }

        fn accepts_commands(&self) -> bool {
            self.accepts
        }

        async fn handle_command(
            &mut self,
            command: serde_json::Value,
            _context: &mut ProcessingContext,
        ) -> anyhow::Result<()> {
            if command == serde_json::json!({"bad": true}) {
                anyhow::bail!("bad command");
            }
            self.commands.lock().unwrap().push(command);
            Ok(())
        }
    }

    type Recorded = (Arc<Mutex<u32>>, Arc<Mutex<Vec<serde_json::Value>>>);

    fn stage(accepts: bool) -> (Stage, Recorded) {
        let processed = Arc::new(Mutex::new(0));
        let commands = Arc::new(Mutex::new(Vec::new()));
        let processor = Interactive {
            accepts,
            processed: processed.clone(),
            commands: commands.clone(),
        };
        let stage = Stage::new("floor".into(), "interactive".into(), Box::new(processor), None);
        (stage, (processed, commands))
    }

    #[tokio::test]
    async fn only_stages_whose_processor_accepts_commands_get_a_channel() {
        let (mut plain, _) = stage(false);
        assert!(plain.open_command_channel().is_none());
        let (mut interactive, _) = stage(true);
        assert!(interactive.open_command_channel().is_some());
    }

    #[tokio::test]
    async fn commands_reach_the_processor_and_rejections_do_not_stop_the_stage() {
        let (mut stage, (processed, commands)) = stage(true);
        let observer = Arc::new(RecordingRuntimeObserver::default());
        stage.set_observer(observer.clone());
        let sender = stage.open_command_channel().unwrap();
        let (control, receiver) = tokio::sync::broadcast::channel(4);
        stage.attach_control_channel(receiver);
        let running = tokio::spawn(async move { stage.run().await });

        sender.send(serde_json::json!({"move": 1})).await.unwrap();
        sender.send(serde_json::json!({"bad": true})).await.unwrap();
        sender.send(serde_json::json!({"move": 2})).await.unwrap();
        tokio::time::sleep(Duration::from_millis(60)).await;
        control.send(ControlMessage::Terminate).unwrap();
        running.await.unwrap().unwrap();

        assert_eq!(
            *commands.lock().unwrap(),
            vec![serde_json::json!({"move": 1}), serde_json::json!({"move": 2})]
        );
        assert!(*processed.lock().unwrap() > 1, "processing continued around the commands");
        let rejected: Vec<RuntimeEvent> = observer
            .events()
            .into_iter()
            .filter(|event| event.kind == RuntimeEventKind::CommandRejected)
            .collect();
        assert_eq!(rejected.len(), 1);
        assert_eq!(rejected[0].stage_id.as_deref(), Some("floor"));
        assert_eq!(rejected[0].text.as_deref(), Some("bad command"));
    }
}
