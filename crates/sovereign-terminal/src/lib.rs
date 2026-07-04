use std::error::Error;
use std::fmt;

use serde::{Deserialize, Serialize};

const DEFAULT_OUTPUT_PREVIEW_CHARS: usize = 4_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandBlock {
    pub id: String,
    pub cwd: String,
    pub command: String,
    pub status: BlockStatus,
    pub exit_code: Option<i32>,
    #[serde(default)]
    pub output_preview: String,
    #[serde(default)]
    pub started_at_ms: u128,
    #[serde(default)]
    pub finished_at_ms: Option<u128>,
    #[serde(default)]
    pub output: Vec<OutputChunk>,
}

impl CommandBlock {
    pub fn new(
        id: impl Into<String>,
        cwd: impl Into<String>,
        command: impl Into<String>,
        started_at_ms: u128,
    ) -> Self {
        Self {
            id: id.into(),
            cwd: cwd.into(),
            command: command.into(),
            status: BlockStatus::Running,
            exit_code: None,
            output_preview: String::new(),
            started_at_ms,
            finished_at_ms: None,
            output: Vec::new(),
        }
    }

    pub fn duration_ms(&self) -> Option<u128> {
        self.finished_at_ms
            .map(|finished_at_ms| finished_at_ms.saturating_sub(self.started_at_ms))
    }

    fn append_chunk(
        &mut self,
        chunk: OutputChunk,
        max_preview_chars: usize,
    ) -> Result<(), TimelineError> {
        if self.status != BlockStatus::Running {
            return Err(TimelineError::BlockAlreadyClosed(self.id.clone()));
        }

        self.output_preview =
            append_to_output_preview(&self.output_preview, &chunk.text, max_preview_chars);
        self.output.push(chunk);
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum BlockStatus {
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

impl BlockStatus {
    pub fn label(self) -> &'static str {
        match self {
            BlockStatus::Running => "running",
            BlockStatus::Succeeded => "succeeded",
            BlockStatus::Failed => "failed",
            BlockStatus::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum OutputStream {
    Stdout,
    Stderr,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OutputChunk {
    pub stream: OutputStream,
    pub text: String,
    pub byte_len: usize,
    pub received_at_ms: u128,
}

impl OutputChunk {
    pub fn from_bytes(stream: OutputStream, bytes: &[u8], received_at_ms: u128) -> Self {
        Self {
            stream,
            text: String::from_utf8_lossy(bytes).to_string(),
            byte_len: bytes.len(),
            received_at_ms,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TerminalEvent {
    CommandStarted(CommandBlock),
    OutputChunk {
        block_id: String,
        stream: OutputStream,
        bytes: Vec<u8>,
    },
    CommandFinished {
        block_id: String,
        exit_code: i32,
    },
    CommandCancelled {
        block_id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockTimeline {
    blocks: Vec<CommandBlock>,
    #[serde(default = "default_preview_chars", skip_serializing)]
    max_preview_chars: usize,
}

impl BlockTimeline {
    pub fn new() -> Self {
        Self {
            blocks: Vec::new(),
            max_preview_chars: DEFAULT_OUTPUT_PREVIEW_CHARS,
        }
    }

    pub fn with_preview_limit(max_preview_chars: usize) -> Self {
        Self {
            blocks: Vec::new(),
            max_preview_chars,
        }
    }

    pub fn blocks(&self) -> &[CommandBlock] {
        &self.blocks
    }

    pub fn start_command(
        &mut self,
        id: impl Into<String>,
        cwd: impl Into<String>,
        command: impl Into<String>,
        started_at_ms: u128,
    ) -> Result<&CommandBlock, TimelineError> {
        let id = id.into();
        if self.blocks.iter().any(|block| block.id == id) {
            return Err(TimelineError::DuplicateBlockId(id));
        }

        self.blocks
            .push(CommandBlock::new(id, cwd, command, started_at_ms));
        Ok(self.blocks.last().expect("block was just pushed"))
    }

    pub fn append_output_bytes(
        &mut self,
        block_id: &str,
        stream: OutputStream,
        bytes: &[u8],
        received_at_ms: u128,
    ) -> Result<(), TimelineError> {
        let max_preview_chars = self.max_preview_chars;
        let block = self.find_block_mut(block_id)?;
        block.append_chunk(
            OutputChunk::from_bytes(stream, bytes, received_at_ms),
            max_preview_chars,
        )
    }

    pub fn finish_command(
        &mut self,
        block_id: &str,
        exit_code: i32,
        finished_at_ms: u128,
    ) -> Result<(), TimelineError> {
        let block = self.find_block_mut(block_id)?;
        if block.status != BlockStatus::Running {
            return Err(TimelineError::BlockAlreadyClosed(block.id.clone()));
        }

        block.status = if exit_code == 0 {
            BlockStatus::Succeeded
        } else {
            BlockStatus::Failed
        };
        block.exit_code = Some(exit_code);
        block.finished_at_ms = Some(finished_at_ms);
        Ok(())
    }

    pub fn cancel_command(
        &mut self,
        block_id: &str,
        finished_at_ms: u128,
    ) -> Result<(), TimelineError> {
        let block = self.find_block_mut(block_id)?;
        if block.status != BlockStatus::Running {
            return Err(TimelineError::BlockAlreadyClosed(block.id.clone()));
        }

        block.status = BlockStatus::Cancelled;
        block.finished_at_ms = Some(finished_at_ms);
        Ok(())
    }

    pub fn search(&self, query: &str) -> Vec<SearchMatch<'_>> {
        let needle = query.trim().to_lowercase();
        if needle.is_empty() {
            return Vec::new();
        }

        self.blocks
            .iter()
            .filter_map(|block| {
                let matched_command = block.command.to_lowercase().contains(&needle);
                let matched_output = block
                    .output
                    .iter()
                    .any(|chunk| chunk.text.to_lowercase().contains(&needle));

                if matched_command || matched_output {
                    Some(SearchMatch {
                        block,
                        matched_command,
                        matched_output,
                    })
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn agent_context_for_blocks<'a>(
        &self,
        block_ids: impl IntoIterator<Item = &'a str>,
    ) -> String {
        self.agent_context_bundle_for_blocks(block_ids)
            .to_prompt_context()
    }

    pub fn agent_context_bundle_for_blocks<'a>(
        &self,
        block_ids: impl IntoIterator<Item = &'a str>,
    ) -> AgentContextBundle {
        let block_ids = block_ids.into_iter().collect::<Vec<_>>();
        AgentContextBundle::from_blocks(
            self.blocks
                .iter()
                .filter(|block| block_ids.iter().any(|block_id| *block_id == block.id)),
        )
    }

    pub fn to_snapshot(
        &self,
        active_cwd: impl Into<String>,
        shell: impl Into<String>,
        selected_text: Option<String>,
    ) -> TerminalSnapshot {
        TerminalSnapshot {
            active_cwd: active_cwd.into(),
            shell: shell.into(),
            blocks: self
                .blocks
                .iter()
                .map(AgentBlockContext::from_block)
                .collect(),
            selected_text,
        }
    }

    fn find_block_mut(&mut self, block_id: &str) -> Result<&mut CommandBlock, TimelineError> {
        self.blocks
            .iter_mut()
            .find(|block| block.id == block_id)
            .ok_or_else(|| TimelineError::BlockNotFound(block_id.to_string()))
    }
}

impl Default for BlockTimeline {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SearchMatch<'a> {
    pub block: &'a CommandBlock,
    pub matched_command: bool,
    pub matched_output: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimelineError {
    DuplicateBlockId(String),
    BlockNotFound(String),
    BlockAlreadyClosed(String),
}

impl fmt::Display for TimelineError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TimelineError::DuplicateBlockId(block_id) => {
                write!(formatter, "block '{block_id}' already exists")
            }
            TimelineError::BlockNotFound(block_id) => {
                write!(formatter, "block '{block_id}' was not found")
            }
            TimelineError::BlockAlreadyClosed(block_id) => {
                write!(formatter, "block '{block_id}' is already closed")
            }
        }
    }
}

impl Error for TimelineError {}

fn default_preview_chars() -> usize {
    DEFAULT_OUTPUT_PREVIEW_CHARS
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalSnapshot {
    pub active_cwd: String,
    pub shell: String,
    pub blocks: Vec<AgentBlockContext>,
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
            context.push_str(last_block.status.label());
            context.push_str("\noutput preview:\n");
            context.push_str(&last_block.output_preview);
            context.push('\n');
        }

        context
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentContextBundle {
    pub selected_blocks: Vec<AgentBlockContext>,
}

impl AgentContextBundle {
    pub fn from_blocks<'a>(blocks: impl IntoIterator<Item = &'a CommandBlock>) -> Self {
        Self {
            selected_blocks: blocks
                .into_iter()
                .map(AgentBlockContext::from_block)
                .collect(),
        }
    }

    pub fn to_prompt_context(&self) -> String {
        let mut context = String::from("selected command blocks:\n");

        for block in &self.selected_blocks {
            context.push_str("\n--- block ");
            context.push_str(&block.id);
            context.push_str(" ---\n");
            context.push_str("cwd: ");
            context.push_str(&block.cwd);
            context.push_str("\ncommand: ");
            context.push_str(&block.command);
            context.push_str("\nstatus: ");
            context.push_str(block.status.label());

            if let Some(exit_code) = block.exit_code {
                context.push_str("\nexit code: ");
                context.push_str(&exit_code.to_string());
            }

            if let Some(duration_ms) = block.duration_ms {
                context.push_str("\nduration ms: ");
                context.push_str(&duration_ms.to_string());
            }

            context.push_str("\noutput preview:\n");
            context.push_str(&block.output_preview);
            context.push('\n');
        }

        context
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentBlockContext {
    pub id: String,
    pub cwd: String,
    pub command: String,
    pub status: BlockStatus,
    pub exit_code: Option<i32>,
    #[serde(default)]
    pub duration_ms: Option<u128>,
    #[serde(default)]
    pub output_preview: String,
}

impl AgentBlockContext {
    fn from_block(block: &CommandBlock) -> Self {
        Self {
            id: block.id.clone(),
            cwd: block.cwd.clone(),
            command: block.command.clone(),
            status: block.status,
            exit_code: block.exit_code,
            duration_ms: block.duration_ms(),
            output_preview: block.output_preview.clone(),
        }
    }
}

fn append_to_output_preview(current: &str, new_text: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }

    let mut preview = String::with_capacity(current.len().saturating_add(new_text.len()));
    preview.push_str(current);
    preview.push_str(new_text);

    let preview_chars = preview.chars().count();
    if preview_chars <= max_chars {
        return preview;
    }

    preview
        .chars()
        .rev()
        .take(max_chars)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_snapshot_builds_agent_context() {
        let snapshot = TerminalSnapshot {
            active_cwd: "/tmp/project".to_string(),
            shell: "zsh".to_string(),
            blocks: vec![AgentBlockContext {
                id: "1".to_string(),
                cwd: "/tmp/project".to_string(),
                command: "cargo test".to_string(),
                status: BlockStatus::Failed,
                exit_code: Some(101),
                duration_ms: Some(10),
                output_preview: "one test failed".to_string(),
            }],
            selected_text: Some("error[E0308]".to_string()),
        };

        let context = snapshot.agent_context();

        assert!(context.contains("cargo test"));
        assert!(context.contains("error[E0308]"));
        assert!(context.contains("one test failed"));
    }

    #[test]
    fn block_timeline_tracks_command_lifecycle() {
        let mut timeline = BlockTimeline::new();

        timeline
            .start_command("block-1", "/tmp/project", "cargo test", 1_000)
            .expect("start command");
        timeline
            .append_output_bytes("block-1", OutputStream::Stdout, b"running 4 tests\n", 1_050)
            .expect("append stdout");
        timeline
            .append_output_bytes("block-1", OutputStream::Stderr, b"failure detail\n", 1_100)
            .expect("append stderr");
        timeline
            .finish_command("block-1", 101, 1_250)
            .expect("finish command");

        let block = &timeline.blocks()[0];
        assert_eq!(block.status, BlockStatus::Failed);
        assert_eq!(block.exit_code, Some(101));
        assert_eq!(block.duration_ms(), Some(250));
        assert!(block.output_preview.contains("running 4 tests"));
        assert!(block.output_preview.contains("failure detail"));
        assert_eq!(block.output[1].stream, OutputStream::Stderr);
    }

    #[test]
    fn block_timeline_searches_command_and_output_locally() {
        let mut timeline = BlockTimeline::new();
        timeline
            .start_command("block-1", "/tmp/project", "git status", 0)
            .expect("start command");
        timeline
            .append_output_bytes(
                "block-1",
                OutputStream::Stdout,
                b"modified: src/main.rs\n",
                1,
            )
            .expect("append output");

        let command_matches = timeline.search("git");
        assert_eq!(command_matches.len(), 1);
        assert!(command_matches[0].matched_command);

        let output_matches = timeline.search("modified");
        assert_eq!(output_matches.len(), 1);
        assert!(output_matches[0].matched_output);
    }

    #[test]
    fn block_timeline_builds_agent_context_for_selected_blocks() {
        let mut timeline = BlockTimeline::new();
        timeline
            .start_command("block-1", "/tmp/project", "cargo build", 10)
            .expect("start command");
        timeline
            .append_output_bytes(
                "block-1",
                OutputStream::Stderr,
                b"error: missing type\n",
                20,
            )
            .expect("append output");
        timeline
            .finish_command("block-1", 101, 30)
            .expect("finish command");

        let context = timeline.agent_context_for_blocks(["block-1"]);

        assert!(context.contains("selected command blocks"));
        assert!(context.contains("cargo build"));
        assert!(context.contains("status: failed"));
        assert!(context.contains("error: missing type"));
        assert!(context.contains("duration ms: 20"));
    }

    #[test]
    fn block_timeline_keeps_preview_bounded() {
        let mut timeline = BlockTimeline::with_preview_limit(6);
        timeline
            .start_command("block-1", "/tmp/project", "printf", 0)
            .expect("start command");
        timeline
            .append_output_bytes("block-1", OutputStream::Stdout, b"hello world", 1)
            .expect("append output");

        assert_eq!(timeline.blocks()[0].output_preview, " world");
    }

    #[test]
    fn block_timeline_rejects_duplicate_block_ids() {
        let mut timeline = BlockTimeline::new();
        timeline
            .start_command("block-1", "/tmp/project", "echo one", 0)
            .expect("start command");

        let error = timeline
            .start_command("block-1", "/tmp/project", "echo two", 1)
            .expect_err("duplicate id");

        assert_eq!(
            error,
            TimelineError::DuplicateBlockId("block-1".to_string())
        );
    }

    #[test]
    fn agent_context_bundle_never_serializes_full_output_chunks() {
        let mut timeline = BlockTimeline::with_preview_limit(6);
        timeline
            .start_command("block-1", "/tmp/project", "cat secret.txt", 0)
            .expect("start command");
        timeline
            .append_output_bytes(
                "block-1",
                OutputStream::Stdout,
                b"sensitive-secret-prefix-public",
                1,
            )
            .expect("append output");

        let bundle = timeline.agent_context_bundle_for_blocks(["block-1"]);
        let serialized = serde_json::to_string(&bundle).expect("serialize bundle");

        assert!(serialized.contains("public"));
        assert!(!serialized.contains("sensitive-secret-prefix"));
        assert!(!serialized.contains("byte_len"));
        assert!(!serialized.contains("received_at_ms"));
    }

    #[test]
    fn terminal_snapshot_serialization_is_agent_safe() {
        let mut timeline = BlockTimeline::with_preview_limit(6);
        timeline
            .start_command("block-1", "/tmp/project", "cat secret.txt", 0)
            .expect("start command");
        timeline
            .append_output_bytes(
                "block-1",
                OutputStream::Stdout,
                b"sensitive-secret-prefix-public",
                1,
            )
            .expect("append output");

        let snapshot = timeline.to_snapshot("/tmp/project", "zsh", None);
        let serialized = serde_json::to_string(&snapshot).expect("serialize snapshot");

        assert!(serialized.contains("public"));
        assert!(!serialized.contains("sensitive-secret-prefix"));
        assert!(!serialized.contains("output\":["));
    }

    #[test]
    fn command_block_deserializes_previous_shape() {
        let raw = r#"{
            "id": "old-1",
            "cwd": "/tmp/project",
            "command": "cargo test",
            "status": "failed",
            "exit_code": 101,
            "output_preview": "old failure"
        }"#;

        let block: CommandBlock = serde_json::from_str(raw).expect("deserialize old block");

        assert_eq!(block.id, "old-1");
        assert_eq!(block.started_at_ms, 0);
        assert_eq!(block.finished_at_ms, None);
        assert!(block.output.is_empty());
    }

    #[test]
    fn terminal_snapshot_deserializes_previous_shape() {
        let raw = r#"{
            "active_cwd": "/tmp/project",
            "shell": "zsh",
            "blocks": [{
                "id": "old-1",
                "cwd": "/tmp/project",
                "command": "cargo test",
                "status": "failed",
                "exit_code": 101,
                "output_preview": "old failure"
            }],
            "selected_text": null
        }"#;

        let snapshot: TerminalSnapshot =
            serde_json::from_str(raw).expect("deserialize old snapshot");

        assert_eq!(snapshot.blocks[0].id, "old-1");
        assert_eq!(snapshot.blocks[0].duration_ms, None);
        assert_eq!(snapshot.blocks[0].output_preview, "old failure");
    }

    #[test]
    fn block_timeline_rejects_output_after_finish() {
        let mut timeline = BlockTimeline::new();
        timeline
            .start_command("block-1", "/tmp/project", "cargo test", 0)
            .expect("start command");
        timeline
            .finish_command("block-1", 0, 10)
            .expect("finish command");

        let error = timeline
            .append_output_bytes("block-1", OutputStream::Stdout, b"late output", 11)
            .expect_err("late output");

        assert_eq!(
            error,
            TimelineError::BlockAlreadyClosed("block-1".to_string())
        );
    }

    #[test]
    fn block_timeline_rejects_repeated_finish() {
        let mut timeline = BlockTimeline::new();
        timeline
            .start_command("block-1", "/tmp/project", "cargo test", 0)
            .expect("start command");
        timeline
            .finish_command("block-1", 0, 10)
            .expect("finish command");

        let error = timeline
            .finish_command("block-1", 101, 11)
            .expect_err("repeated finish");

        assert_eq!(
            error,
            TimelineError::BlockAlreadyClosed("block-1".to_string())
        );
        assert_eq!(timeline.blocks()[0].status, BlockStatus::Succeeded);
        assert_eq!(timeline.blocks()[0].exit_code, Some(0));
    }
}
