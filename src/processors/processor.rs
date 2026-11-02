use crate::core::context::ProcessingContext;

use async_trait::async_trait;

/// This trait defines the interace for a processor in a pipeline stage. It is responsible
/// for processing messages from input channels and sending them to output channels.
#[async_trait]
pub trait Processor: Send + Sync {
    /// Initializes the processor.
    /// This method is guaranteed to be called before any processing occurs.
    async fn init(&mut self) -> anyhow::Result<()>;

    /// Processes messages from the input channels and sends them to the output channel.
    ///
    /// # Arguments
    /// * `context` - A mutable reference to the processing context, which contains information
    ///   about the input and output channels.
    ///
    /// # Returns
    /// A result indicating success or failure of the processing.
    async fn process(&mut self, context: &mut ProcessingContext) -> anyhow::Result<()>;

    /// Whether this processor takes commands from outside the pipeline (see `core::commands`).
    /// Only stages whose processor returns `true` get a command channel.
    fn accepts_commands(&self) -> bool {
        false
    }

    /// Handles one command sent to this stage. Runs between `process` calls, which may be
    /// interrupted to deliver it. An error rejects the command; the stage keeps running.
    async fn handle_command(
        &mut self,
        _command: serde_json::Value,
        _context: &mut ProcessingContext,
    ) -> anyhow::Result<()> {
        Err(anyhow::anyhow!("this stage does not accept commands"))
    }
}
