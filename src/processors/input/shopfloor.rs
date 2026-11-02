//! `shopfloor` input: runs a shopfloor simulation (the `shopfloor-sim` engine in `shopfloor/sim`)
//! inside the pipeline.
//!
//! - Every simulation event (positions, moves, actions, sensor readings, scenes…) is published as
//!   one message whose payload is the event's JSON (`kind`, `entity`, …; see shopfloor/README.md).
//! - A snapshot of the world is published for the GUI as a `processor_view` runtime event.
//! - The stage accepts commands (`{"command": {...}}` / `{"control": ...}`), so the GUI can drive
//!   the mobile manipulators, sensors and scenes while the pipeline runs.
//!
//! ```toml
//! [inputs.floor]
//! type = "shopfloor"
//! output = "shopfloor_events"
//! parameters = { scenario = "config/shopfloor/sample-floor.toml", scene = "production" }
//! ```

use crate::config::{
    ProcessorConfig, StageConfig, TimingConfig, defaulted_param, optional_param, required_param,
};
use crate::core::context::ProcessingContext;
use crate::core::timing_mixin::{TimingMixin, WithTimingMixin};
use crate::processors::Processor;

use anyhow::{Context, anyhow};
use async_trait::async_trait;
use shopfloor_sim::runtime::now_ms;
use shopfloor_sim::{Control, Engine, Origin, Request, Scenario, SimEvent, load_scenario};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, UNIX_EPOCH};
use tokio::time::{Interval, MissedTickBehavior};

#[derive(Debug, Clone)]
pub struct ShopfloorInputConfig {
    /// Scenario file, relative to the working directory like other Liminal paths.
    pub scenario: PathBuf,
    /// Scene to play as soon as the pipeline starts.
    pub scene: Option<String>,
    /// How often the world is published for the GUI.
    pub view_interval_ms: u64,
    pub timing: Option<TimingConfig>,
}

impl ProcessorConfig for ShopfloorInputConfig {
    fn from_stage_config(config: &StageConfig) -> anyhow::Result<Self> {
        let scenario: String = required_param(&config.parameters, "scenario")?;
        Ok(Self {
            scenario: PathBuf::from(scenario),
            scene: optional_param(&config.parameters, "scene")?
                .filter(|scene: &String| !scene.trim().is_empty()),
            view_interval_ms: defaulted_param(&config.parameters, "view_interval_ms", 100)?,
            timing: config.timing.clone(),
        })
    }

    fn validate(&self) -> anyhow::Result<()> {
        if self.view_interval_ms == 0 {
            return Err(anyhow!("view_interval_ms must be greater than 0"));
        }
        Ok(())
    }
}

pub struct ShopfloorInputProcessor {
    name: String,
    config: ShopfloorInputConfig,
    /// Loaded in `init` (like other processors, file access happens there, in the pipeline's
    /// working directory; the GUI's preflight only constructs processors).
    scenario: Option<Arc<Scenario>>,
    engine: Option<Engine>,
    ticker: Option<Interval>,
    /// Events not yet published. `process` publishes the front one and only removes it once the
    /// publish completed, so an interrupted `process` (e.g. by a command) loses nothing.
    outbox: VecDeque<SimEvent>,
    last_view: Option<Instant>,
    timing: TimingMixin,
}

impl ShopfloorInputProcessor {
    pub fn new(name: &str, config: StageConfig) -> anyhow::Result<Box<dyn Processor>> {
        let config = ShopfloorInputConfig::from_stage_config(&config)?;
        config.validate()?;
        let timing = TimingMixin::new(config.timing.as_ref());
        Ok(Box::new(Self {
            name: name.to_string(),
            config,
            scenario: None,
            engine: None,
            ticker: None,
            outbox: VecDeque::new(),
            last_view: None,
            timing,
        }))
    }

    fn engine(&mut self) -> anyhow::Result<&mut Engine> {
        self.engine
            .as_mut()
            .ok_or_else(|| anyhow!("{}: simulation not initialised", self.name))
    }

    fn emit_view(&mut self, context: &ProcessingContext) {
        if let Some(engine) = &self.engine {
            context.emit_view(serde_json::json!({ "state": engine.state() }));
            self.last_view = Some(Instant::now());
        }
    }

    async fn publish(&mut self, context: &mut ProcessingContext, event: &SimEvent) {
        let Some(output) = &context.output else {
            return;
        };
        let payload = match serde_json::to_value(event) {
            Ok(payload) => payload,
            Err(error) => {
                tracing::error!("{}: could not serialise event: {error}", self.name);
                return;
            }
        };
        let event_time = UNIX_EPOCH + Duration::from_millis(event.ts);
        let message = self.timing.create_message_with_event_time_extraction(
            &self.name,
            &output.name,
            payload,
            event_time,
        );
        if let Err(error) = output.channel.publish(message).await {
            // E.g. nobody subscribed to a broadcast channel; events are not kept for later.
            tracing::debug!("{}: downstream publish failed: {error:?}", self.name);
        }
    }
}

#[async_trait]
impl Processor for ShopfloorInputProcessor {
    async fn init(&mut self) -> anyhow::Result<()> {
        let scenario = Arc::new(load_scenario(&self.config.scenario)?);
        if let Some(scene) = &self.config.scene
            && scenario.scene_index(scene).is_none()
        {
            let known: Vec<&str> = scenario.scenes.iter().map(|s| s.id.as_str()).collect();
            return Err(anyhow!(
                "unknown scene '{scene}' in {} (scenes: {})",
                self.config.scenario.display(),
                if known.is_empty() { "none".to_string() } else { known.join(", ") }
            ));
        }
        if !scenario.output.sinks.is_empty() {
            tracing::info!(
                "{}: ignoring the scenario's [output.sinks]; events go into the pipeline",
                self.name
            );
        }

        let mut engine = Engine::new(Arc::clone(&scenario));
        let ts = now_ms();
        self.outbox.extend(engine.start(ts));
        if let Some(scene) = &self.config.scene {
            self.outbox
                .extend(engine.control(Control::PlayScene(scene.clone()), ts)?);
        }
        let mut ticker = tokio::time::interval(Duration::from_millis(scenario.sim.tick_ms));
        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
        self.ticker = Some(ticker);
        self.engine = Some(engine);
        tracing::info!(
            "{}: shopfloor '{}' from {}{}",
            self.name,
            scenario.name,
            self.config.scenario.display(),
            self.config
                .scene
                .as_ref()
                .map(|scene| format!(", playing scene '{scene}'"))
                .unwrap_or_default()
        );
        self.scenario = Some(scenario);
        Ok(())
    }

    async fn process(&mut self, context: &mut ProcessingContext) -> anyhow::Result<()> {
        if let Some(event) = self.outbox.front().cloned() {
            self.publish(context, &event).await;
            self.outbox.pop_front();
            return Ok(());
        }

        // Cancel-safe: an interrupted tick just happens on the next call.
        self.ticker
            .as_mut()
            .ok_or_else(|| anyhow!("{}: simulation not initialised", self.name))?
            .tick()
            .await;
        let events = self.engine()?.step(now_ms());
        self.outbox.extend(events);

        let view_due = self.last_view.is_none_or(|last| {
            last.elapsed() >= Duration::from_millis(self.config.view_interval_ms)
        });
        if view_due {
            self.emit_view(context);
        }
        Ok(())
    }

    fn accepts_commands(&self) -> bool {
        true
    }

    async fn handle_command(
        &mut self,
        command: serde_json::Value,
        context: &mut ProcessingContext,
    ) -> anyhow::Result<()> {
        let request: Request = serde_json::from_value(command)
            .context("expected {\"command\": {...}} or {\"control\": ...}")?;
        let events = self.engine()?.request(request, Origin::Ui, now_ms())?;
        self.outbox.extend(events);
        // Show the effect right away rather than at the next view.
        self.emit_view(context);
        Ok(())
    }
}

impl WithTimingMixin for ShopfloorInputProcessor {
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
    use crate::config::types::ChannelType;
    use crate::core::channel::{Channel, PubSubChannel, Subscriber};
    use crate::core::message::Message;
    use crate::core::runtime_event::RuntimeEventKind;
    use crate::core::runtime_observer::tests::RecordingRuntimeObserver;
    use serde_json::{Value, json};
    use std::collections::HashMap;

    const SCENARIO: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/config/shopfloor/sample-floor.toml");

    fn stage_config(scene: Option<&str>) -> StageConfig {
        let mut parameters = HashMap::new();
        parameters.insert("scenario".to_string(), json!(SCENARIO));
        if let Some(scene) = scene {
            parameters.insert("scene".to_string(), json!(scene));
        }
        StageConfig {
            r#type: "shopfloor".to_string(),
            inputs: None,
            output: Some("shopfloor_events".to_string()),
            concurrency: None,
            channel: None,
            timing: None,
            parameters: Some(parameters),
        }
    }

    struct Harness {
        processor: Box<dyn Processor>,
        context: ProcessingContext,
        output: Subscriber<Message>,
        observer: Arc<RecordingRuntimeObserver>,
    }

    async fn harness(scene: Option<&str>) -> Harness {
        let mut processor = ShopfloorInputProcessor::new("floor", stage_config(scene)).unwrap();
        processor.init().await.unwrap();
        let observer = Arc::new(RecordingRuntimeObserver::default());
        let mut context = ProcessingContext::new("floor".to_string());
        context.set_runtime_metadata("shopfloor".to_string(), observer.clone());
        let channel = Arc::new(Channel::new(ChannelType::Broadcast, 4096));
        let output = channel.subscribe();
        context.attach_output("shopfloor_events".to_string(), channel);
        Harness {
            processor,
            context,
            output,
            observer,
        }
    }

    impl Harness {
        async fn run(&mut self, calls: usize) {
            for _ in 0..calls {
                self.processor.process(&mut self.context).await.unwrap();
            }
        }

        async fn payloads(&mut self) -> Vec<Value> {
            let mut payloads = Vec::new();
            while let Some(message) = self.output.try_recv().await {
                payloads.push(message.payload);
            }
            payloads
        }
    }

    fn kinds(payloads: &[Value]) -> Vec<&str> {
        payloads.iter().map(|p| p["kind"].as_str().unwrap()).collect()
    }

    #[tokio::test]
    async fn publishes_simulation_events_and_autoplays_the_scene() {
        let mut harness = harness(Some("production")).await;
        harness.run(5).await;
        let payloads = harness.payloads().await;
        assert_eq!(&kinds(&payloads)[..3], &["sim", "scene", "move_command"]);
        assert_eq!(payloads[1]["scene"], "production");
        assert_eq!(payloads[2]["origin"], "scene");
        assert_eq!(payloads[2]["v"], 1);
    }

    #[tokio::test]
    async fn publishes_a_view_of_the_world_for_the_gui() {
        let mut harness = harness(None).await;
        harness.run(3).await;
        let views: Vec<_> = harness
            .observer
            .events()
            .into_iter()
            .filter(|event| event.kind == RuntimeEventKind::ProcessorView)
            .collect();
        assert!(!views.is_empty());
        let view = views[0].data.as_ref().unwrap();
        assert_eq!(views[0].stage_id.as_deref(), Some("floor"));
        assert_eq!(view["state"]["mobile_manipulators"][0]["id"], "mm1");
    }

    #[tokio::test]
    async fn commands_drive_the_simulation() {
        let mut harness = harness(None).await;
        harness
            .processor
            .handle_command(
                json!({"command": {"type": "move_to", "mm": "mm1", "target": {"machine": "machine3"}}}),
                &mut harness.context,
            )
            .await
            .unwrap();
        harness
            .processor
            .handle_command(json!({"control": "pause"}), &mut harness.context)
            .await
            .unwrap();
        harness.run(4).await;
        let payloads = harness.payloads().await;
        let moved = payloads
            .iter()
            .find(|p| p["kind"] == "move_command")
            .expect("a move_command event");
        assert_eq!(moved["origin"], "ui");
        assert_eq!(moved["to"]["at"], "machine3");
        assert!(payloads.iter().any(|p| p["kind"] == "sim" && p["state"] == "paused"));

        let error = harness
            .processor
            .handle_command(
                json!({"command": {"type": "perform", "mm": "mm2", "action": "pick"}}),
                &mut harness.context,
            )
            .await
            .unwrap_err();
        assert!(error.to_string().contains("mm2 cannot perform 'pick'"), "{error}");
        let error = harness
            .processor
            .handle_command(json!({"teleport": {}}), &mut harness.context)
            .await
            .unwrap_err();
        assert!(format!("{error:#}").contains("expected"), "{error:#}");
    }

    #[tokio::test]
    async fn rejects_unknown_scenes_and_missing_scenarios_at_init() {
        // Constructing never touches the file system (the GUI's preflight runs elsewhere).
        let mut processor = ShopfloorInputProcessor::new("floor", stage_config(Some("nope"))).unwrap();
        let error = processor.init().await.unwrap_err();
        assert!(error.to_string().contains("unknown scene 'nope'"), "{error}");

        let mut config = stage_config(None);
        config.parameters = Some(HashMap::from([(
            "scenario".to_string(),
            json!("does/not/exist.toml"),
        )]));
        let mut processor = ShopfloorInputProcessor::new("floor", config).unwrap();
        let error = processor.init().await.unwrap_err();
        assert!(format!("{error:#}").contains("does/not/exist.toml"), "{error:#}");

        let mut config = stage_config(None);
        config.parameters = None;
        assert!(ShopfloorInputProcessor::new("floor", config).is_err(), "scenario is required");
    }
}
