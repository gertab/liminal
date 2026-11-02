//! Commands sent to running stages from outside the pipeline (e.g. the GUI driving a simulated
//! input). Each command is one JSON line:
//!
//! ```text
//! {"stage": "floor", "command": { ... }}
//! ```
//!
//! Only stages whose processor accepts commands (`Processor::accepts_commands`) get a command
//! channel; the command payload is passed to that processor's `handle_command` as-is.

use serde::Deserialize;
use std::collections::HashMap;
use tokio::io::{AsyncBufRead, AsyncBufReadExt};
use tokio::sync::mpsc;

pub type CommandSender = mpsc::Sender<serde_json::Value>;
pub type CommandReceiver = mpsc::Receiver<serde_json::Value>;

/// Commands waiting per stage before senders have to wait.
pub const COMMAND_CHANNEL_CAPACITY: usize = 64;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StageCommand {
    pub stage: String,
    pub command: serde_json::Value,
}

/// Parses one command line (`{"stage": "...", "command": {...}}`).
pub fn parse_command_line(line: &str) -> Result<StageCommand, String> {
    serde_json::from_str(line).map_err(|error| format!("invalid command line: {error}"))
}

/// Delivers commands to the stages that accept them.
#[derive(Debug, Clone, Default)]
pub struct CommandRouter {
    senders: HashMap<String, CommandSender>,
}

impl CommandRouter {
    pub fn insert(&mut self, stage: String, sender: CommandSender) {
        self.senders.insert(stage, sender);
    }

    /// Names of the stages that accept commands.
    pub fn stages(&self) -> Vec<String> {
        let mut stages: Vec<String> = self.senders.keys().cloned().collect();
        stages.sort();
        stages
    }

    pub async fn route(&self, command: StageCommand) -> Result<(), String> {
        let sender = self.senders.get(&command.stage).ok_or_else(|| {
            let known = self.stages();
            format!(
                "stage '{}' does not accept commands (stages that do: {})",
                command.stage,
                if known.is_empty() {
                    "none".to_string()
                } else {
                    known.join(", ")
                }
            )
        })?;
        sender
            .send(command.command)
            .await
            .map_err(|_| format!("stage '{}' has stopped", command.stage))
    }
}

/// Reads command lines until EOF and routes each one. Problems are logged, never fatal.
pub async fn read_commands<R: AsyncBufRead + Unpin>(reader: R, router: CommandRouter) {
    let mut lines = reader.lines();
    loop {
        match lines.next_line().await {
            Ok(Some(line)) if line.trim().is_empty() => {}
            Ok(Some(line)) => {
                let result = match parse_command_line(&line) {
                    Ok(command) => router.route(command).await,
                    Err(error) => Err(error),
                };
                if let Err(error) = result {
                    tracing::warn!("command ignored: {error}");
                }
            }
            Ok(None) => break,
            Err(error) => {
                tracing::warn!("stopped reading commands: {error}");
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_command_lines() {
        assert_eq!(
            parse_command_line(r#"{"stage":"floor","command":{"control":"pause"}}"#).unwrap(),
            StageCommand {
                stage: "floor".into(),
                command: json!({"control": "pause"}),
            }
        );
        assert!(parse_command_line("not json").unwrap_err().contains("invalid command line"));
        assert!(parse_command_line(r#"{"stage":"floor"}"#).is_err());
        assert!(parse_command_line(r#"{"stage":"floor","command":1,"extra":2}"#).is_err());
    }

    #[tokio::test]
    async fn routes_to_the_named_stage_only() {
        let (sender, mut receiver) = mpsc::channel(4);
        let mut router = CommandRouter::default();
        router.insert("floor".into(), sender);

        router
            .route(StageCommand {
                stage: "floor".into(),
                command: json!({"control": "reset"}),
            })
            .await
            .unwrap();
        assert_eq!(receiver.recv().await, Some(json!({"control": "reset"})));

        let error = router
            .route(StageCommand {
                stage: "rules".into(),
                command: json!({}),
            })
            .await
            .unwrap_err();
        assert_eq!(error, "stage 'rules' does not accept commands (stages that do: floor)");
    }

    #[tokio::test]
    async fn reads_lines_until_eof_and_skips_bad_ones() {
        let (sender, mut receiver) = mpsc::channel(4);
        let mut router = CommandRouter::default();
        router.insert("floor".into(), sender);
        let input = b"garbage\n\n{\"stage\":\"nope\",\"command\":1}\n{\"stage\":\"floor\",\"command\":2}\n";
        read_commands(&input[..], router).await;
        assert_eq!(receiver.recv().await, Some(json!(2)));
        assert!(receiver.try_recv().is_err(), "only the valid line was delivered");
    }
}
