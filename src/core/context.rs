use super::channel::{PubSubChannel, PublishError, Subscriber};
use super::message::Message;
use super::runtime_event::{RuntimeEvent, RuntimeEventKind};
use super::runtime_observer::{SharedRuntimeObserver, noop_runtime_observer};

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;

pub struct ProcessingContext {
    pub stage_name: String,
    processor_type: String,
    observer: SharedRuntimeObserver,
    pub inputs: HashMap<String, ObservedSubscriber>,
    pub output: Option<OutputInfo>,
    pub metadata: HashMap<String, String>,
}

pub struct OutputInfo {
    pub channel: Arc<dyn PubSubChannel<Message>>,
    pub name: String,
}

impl ProcessingContext {
    pub fn new(stage_name: String) -> Self {
        Self {
            stage_name,
            processor_type: String::new(),
            observer: noop_runtime_observer(),
            inputs: HashMap::new(),
            output: None,
            metadata: HashMap::new(),
        }
    }

    pub fn set_runtime_metadata(
        &mut self,
        processor_type: String,
        observer: SharedRuntimeObserver,
    ) {
        self.processor_type = processor_type;
        self.observer = observer;
    }

    /// Publishes a snapshot of the processor's state for the GUI (a `processor_view` runtime
    /// event, e.g. the live shopfloor). Only seen when runtime events are enabled.
    pub fn emit_view(&self, view: serde_json::Value) {
        self.observer.emit(
            RuntimeEvent::new(RuntimeEventKind::ProcessorView)
                .stage(self.stage_name.clone())
                .processor_type(self.processor_type.clone())
                .data(view),
        );
    }

    pub fn attach_output(&mut self, name: String, channel: Arc<dyn PubSubChannel<Message>>) {
        let channel = Arc::new(ObservedOutputChannel {
            inner: channel,
            stage_name: self.stage_name.clone(),
            processor_type: self.processor_type.clone(),
            channel_name: name.clone(),
            observer: self.observer.clone(),
        });
        self.output = Some(OutputInfo { channel, name });
    }

    pub fn add_input(&mut self, name: String, subscriber: Subscriber<Message>) {
        self.inputs.insert(
            name.clone(),
            ObservedSubscriber {
                inner: subscriber,
                stage_name: self.stage_name.clone(),
                processor_type: self.processor_type.clone(),
                channel_name: name,
                observer: self.observer.clone(),
            },
        );
    }
}

pub struct ObservedSubscriber {
    inner: Subscriber<Message>,
    stage_name: String,
    processor_type: String,
    channel_name: String,
    observer: SharedRuntimeObserver,
}

impl ObservedSubscriber {
    pub fn new(channel_name: String, subscriber: Subscriber<Message>) -> Self {
        Self {
            inner: subscriber,
            stage_name: String::new(),
            processor_type: String::new(),
            channel_name,
            observer: noop_runtime_observer(),
        }
    }

    pub async fn recv(&mut self) -> Option<Message> {
        let message = self.inner.recv().await;
        self.emit_received(&message);
        message
    }

    pub async fn try_recv(&mut self) -> Option<Message> {
        let message = self.inner.try_recv().await;
        self.emit_received(&message);
        message
    }

    fn emit_received(&self, message: &Option<Message>) {
        if let Some(message) = message {
            self.observer.emit(
                RuntimeEvent::new(RuntimeEventKind::MessageReceived)
                    .stage(self.stage_name.clone())
                    .processor_type(self.processor_type.clone())
                    .channel(self.channel_name.clone())
                    .text(format!("{}:{}", message.source, message.topic)),
            );
        }
    }
}

struct ObservedOutputChannel {
    inner: Arc<dyn PubSubChannel<Message>>,
    stage_name: String,
    processor_type: String,
    channel_name: String,
    observer: SharedRuntimeObserver,
}

#[async_trait]
impl PubSubChannel<Message> for ObservedOutputChannel {
    async fn publish(&self, message: Message) -> Result<(), PublishError<Message>> {
        let event_text = format!("{}:{}", message.source, message.topic);
        self.inner.publish(message).await?;
        self.observer.emit(
            RuntimeEvent::new(RuntimeEventKind::MessageEmitted)
                .stage(self.stage_name.clone())
                .processor_type(self.processor_type.clone())
                .channel(self.channel_name.clone())
                .text(event_text),
        );
        Ok(())
    }

    fn subscribe(&self) -> Subscriber<Message> {
        self.inner.subscribe()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::channel::MpscChannel;
    use crate::core::runtime_event::RuntimeEventKind;
    use crate::core::runtime_observer::tests::RecordingRuntimeObserver;
    use serde_json::json;
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn observed_output_channel_emits_message_event_after_publish() {
        let observer = Arc::new(RecordingRuntimeObserver::default());
        let channel = Arc::new(MpscChannel::new(4));
        let mut context = ProcessingContext::new("producer".to_string());
        context.set_runtime_metadata("simulated".to_string(), observer.clone());
        context.attach_output("raw".to_string(), channel);

        context
            .output
            .as_ref()
            .expect("output attached")
            .channel
            .publish(Message::new("producer", "raw", json!({ "value": 1 })))
            .await
            .expect("publish succeeds");

        let events = observer.events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, RuntimeEventKind::MessageEmitted);
        assert_eq!(events[0].stage_id.as_deref(), Some("producer"));
        assert_eq!(events[0].processor_type.as_deref(), Some("simulated"));
        assert_eq!(events[0].channel_name.as_deref(), Some("raw"));
    }

    #[tokio::test]
    async fn observed_subscriber_emits_message_event_after_receive() {
        let observer = Arc::new(RecordingRuntimeObserver::default());
        let (sender, receiver) = mpsc::channel(4);
        let mut context = ProcessingContext::new("consumer".to_string());
        context.set_runtime_metadata("console".to_string(), observer.clone());
        context.add_input("raw".to_string(), Subscriber::Mpsc(receiver));

        sender
            .send(Message::new("producer", "raw", json!({ "value": 1 })))
            .await
            .expect("send succeeds");

        let input = context.inputs.get_mut("raw").expect("input attached");
        let message = input.try_recv().await.expect("message received");

        assert_eq!(message.topic, "raw");
        let events = observer.events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, RuntimeEventKind::MessageReceived);
        assert_eq!(events[0].stage_id.as_deref(), Some("consumer"));
        assert_eq!(events[0].processor_type.as_deref(), Some("console"));
        assert_eq!(events[0].channel_name.as_deref(), Some("raw"));
    }
}
