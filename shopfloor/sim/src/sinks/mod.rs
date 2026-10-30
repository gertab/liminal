//! Event sinks: where the event stream goes besides the UI.
//!
//! Each sink runs on its own OS thread, reading batches from the runtime's broadcast channel, so a
//! slow disk or terminal never stalls the simulation. A sink that falls too far behind skips
//! batches (with a warning) rather than slowing the engine down.

mod jsonl;
mod stdout;

pub use jsonl::JsonlSink;
pub use stdout::StdoutSink;

use crate::event::SimEvent;
use crate::runtime::EventBatch;
use crate::scenario::{SinkDef, SinkKind};
use tokio::sync::broadcast;
use tokio::sync::broadcast::error::RecvError;

pub trait EventSink: Send {
    fn write_batch(&mut self, events: &[SimEvent]) -> std::io::Result<()>;

    fn close(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub fn build_sink(def: &SinkDef) -> anyhow::Result<Box<dyn EventSink>> {
    Ok(match &def.kind {
        SinkKind::Stdout => Box::new(StdoutSink),
        SinkKind::Jsonl { path, append } => Box::new(JsonlSink::open(path, *append)?),
    })
}

/// One JSON object per line, as written by every sink.
pub fn to_json_line(event: &SimEvent) -> String {
    serde_json::to_string(event).expect("events always serialise")
}

pub(crate) struct SinkThread {
    pub name: String,
    pub join: std::thread::JoinHandle<()>,
}

pub(crate) fn spawn_sink(
    name: String,
    mut sink: Box<dyn EventSink>,
    mut events: broadcast::Receiver<EventBatch>,
) -> SinkThread {
    let thread_name = name.clone();
    let join = std::thread::Builder::new()
        .name(format!("sink-{name}"))
        .spawn(move || {
            loop {
                match events.blocking_recv() {
                    Ok(batch) => {
                        if let Err(error) = sink.write_batch(&batch) {
                            tracing::error!("sink '{thread_name}': write failed, stopping: {error}");
                            break;
                        }
                    }
                    Err(RecvError::Lagged(skipped)) => {
                        tracing::warn!("sink '{thread_name}' fell behind and skipped {skipped} batches");
                    }
                    Err(RecvError::Closed) => break,
                }
            }
            if let Err(error) = sink.close() {
                tracing::error!("sink '{thread_name}': close failed: {error}");
            }
        })
        .expect("failed to spawn sink thread");
    SinkThread { name, join }
}
