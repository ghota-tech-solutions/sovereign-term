use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandBlock {
    pub id: String,
    pub cwd: String,
    pub command: String,
    pub status: BlockStatus,
    pub exit_code: Option<i32>,
    pub output_preview: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum BlockStatus {
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TerminalEvent {
    CommandStarted(CommandBlock),
    OutputChunk { block_id: String, bytes: Vec<u8> },
    CommandFinished { block_id: String, exit_code: i32 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalSnapshot {
    pub active_cwd: String,
    pub shell: String,
    pub blocks: Vec<CommandBlock>,
    pub selected_text: Option<String>,
}

impl TerminalSnapshot {
    pub fn agent_context(&self) -> String {
        let mut context = format!("shell: {}\ncwd: {}\n", self.shell, self.active_cwd);

        if let Some(selected_text) = &self.selected_text {
            context.push_str("\nselected terminal text:\n");
            context.push_str(selected_text);
            context.push('\n');
        }

        if let Some(last_block) = self.blocks.last() {
            context.push_str("\nlast command:\n");
            context.push_str(&last_block.command);
            context.push_str("\nstatus: ");
            context.push_str(match last_block.status {
                BlockStatus::Running => "running",
                BlockStatus::Succeeded => "succeeded",
                BlockStatus::Failed => "failed",
                BlockStatus::Cancelled => "cancelled",
            });
            context.push_str("\noutput preview:\n");
            context.push_str(&last_block.output_preview);
            context.push('\n');
        }

        context
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_snapshot_builds_agent_context() {
        let snapshot = TerminalSnapshot {
            active_cwd: "/tmp/project".to_string(),
            shell: "zsh".to_string(),
            blocks: vec![CommandBlock {
                id: "1".to_string(),
                cwd: "/tmp/project".to_string(),
                command: "cargo test".to_string(),
                status: BlockStatus::Failed,
                exit_code: Some(101),
                output_preview: "one test failed".to_string(),
            }],
            selected_text: Some("error[E0308]".to_string()),
        };

        let context = snapshot.agent_context();

        assert!(context.contains("cargo test"));
        assert!(context.contains("error[E0308]"));
        assert!(context.contains("one test failed"));
    }
}
