use std::collections::HashSet;
use std::error::Error;
use std::fmt;

use serde::{Deserialize, Serialize};
use sovereign_terminal::{BlockStatus, BlockTimeline, CommandBlock, OutputStream};

const DEFAULT_AGENT_PANEL_WIDTH_FRACTION: f32 = 0.34;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceSurface {
    pub workspace_name: String,
    pub active_tab_id: String,
    pub tabs: Vec<WorkspaceTab>,
    pub agent_panel: AgentPanelState,
    pub privacy_footer: PrivacyFooterState,
    #[serde(default)]
    pub command_palette: CommandPaletteState,
}

impl WorkspaceSurface {
    pub fn new(
        workspace_name: impl Into<String>,
        first_tab: WorkspaceTab,
        privacy_footer: PrivacyFooterState,
    ) -> Self {
        let active_tab_id = first_tab.id.clone();
        Self {
            workspace_name: workspace_name.into(),
            active_tab_id,
            tabs: vec![first_tab],
            agent_panel: AgentPanelState::default(),
            privacy_footer,
            command_palette: CommandPaletteState::default(),
        }
    }

    pub fn demo_local() -> Self {
        let mut timeline = BlockTimeline::new();
        timeline
            .start_command("demo-cargo-test", "/Users/me/project", "cargo test", 1_000)
            .expect("demo block id is unique");
        timeline
            .append_output_bytes(
                "demo-cargo-test",
                OutputStream::Stdout,
                b"running 4 tests\nall tests passed\n",
                1_100,
            )
            .expect("demo block is running");
        timeline
            .finish_command("demo-cargo-test", 0, 1_350)
            .expect("demo block can finish");

        let first_tab = WorkspaceTab::new(
            "tab-main",
            "project",
            TerminalPane::from_timeline("pane-main", "/Users/me/project", "zsh", &timeline),
        );
        let mut surface = Self::new(
            "project",
            first_tab,
            PrivacyFooterState {
                provider_scope: ProviderScope::Local,
                provider_name: "oMLX Ornith".to_string(),
                network_destination: "127.0.0.1:8000".to_string(),
                telemetry_enabled: false,
                cloud_handoff_enabled: false,
                plugins_with_terminal_access: 0,
                plugins_with_filesystem_access: 0,
            },
        );
        surface.agent_panel.context_chips = vec![
            AgentContextChip::new(
                "chip-selected-block",
                "selected block",
                ContextChipPayload::SelectedBlock {
                    pane_id: "pane-main".to_string(),
                    block_id: "demo-cargo-test".to_string(),
                },
            ),
            AgentContextChip::new(
                "chip-git-diff",
                "git diff",
                ContextChipPayload::GitDiff {
                    snapshot_id: "git-snapshot-current".to_string(),
                },
            ),
            AgentContextChip::new(
                "chip-files",
                "filesystem snapshot",
                ContextChipPayload::FilesystemSelection {
                    snapshot_id: "fs-snapshot-current".to_string(),
                    root: "/Users/me/project".to_string(),
                    paths: vec!["src/main.rs".to_string()],
                },
            ),
        ];
        surface
            .active_tab_mut()
            .expect("active tab")
            .select_block("pane-main", "demo-cargo-test")
            .expect("demo block exists");
        surface
    }

    pub fn active_tab(&self) -> Option<&WorkspaceTab> {
        self.tabs.iter().find(|tab| tab.id == self.active_tab_id)
    }

    pub fn active_tab_mut(&mut self) -> Option<&mut WorkspaceTab> {
        self.tabs
            .iter_mut()
            .find(|tab| tab.id == self.active_tab_id)
    }

    pub fn add_tab(&mut self, tab: WorkspaceTab) -> Result<(), SurfaceError> {
        if self.tabs.iter().any(|existing| existing.id == tab.id) {
            return Err(SurfaceError::DuplicateTabId(tab.id));
        }
        self.tabs.push(tab);
        Ok(())
    }

    pub fn select_tab(&mut self, tab_id: &str) -> Result<(), SurfaceError> {
        if !self.tabs.iter().any(|tab| tab.id == tab_id) {
            return Err(SurfaceError::TabNotFound(tab_id.to_string()));
        }
        self.active_tab_id = tab_id.to_string();
        Ok(())
    }

    pub fn show_agent_panel(&mut self) {
        self.agent_panel.visible = true;
    }

    pub fn hide_agent_panel(&mut self) {
        self.agent_panel.visible = false;
    }

    pub fn validate(&self) -> Result<(), SurfaceError> {
        if self.tabs.is_empty() {
            return Err(SurfaceError::EmptyTabSet);
        }

        let mut tab_ids = HashSet::new();
        for tab in &self.tabs {
            if !tab_ids.insert(tab.id.as_str()) {
                return Err(SurfaceError::DuplicateTabId(tab.id.clone()));
            }
            tab.validate()?;
        }

        if !self.tabs.iter().any(|tab| tab.id == self.active_tab_id) {
            return Err(SurfaceError::TabNotFound(self.active_tab_id.clone()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceTab {
    pub id: String,
    pub title: String,
    pub active_pane_id: String,
    pub split: PaneSplit,
    pub panes: Vec<TerminalPane>,
}

impl WorkspaceTab {
    pub fn new(id: impl Into<String>, title: impl Into<String>, first_pane: TerminalPane) -> Self {
        let active_pane_id = first_pane.id.clone();
        Self {
            id: id.into(),
            title: title.into(),
            active_pane_id,
            split: PaneSplit::Single,
            panes: vec![first_pane],
        }
    }

    pub fn active_pane(&self) -> Option<&TerminalPane> {
        self.panes
            .iter()
            .find(|pane| pane.id == self.active_pane_id)
    }

    pub fn add_pane(&mut self, pane: TerminalPane, split: PaneSplit) -> Result<(), SurfaceError> {
        if self.panes.iter().any(|existing| existing.id == pane.id) {
            return Err(SurfaceError::DuplicatePaneId(pane.id));
        }
        self.panes.push(pane);
        self.split = split;
        Ok(())
    }

    pub fn select_pane(&mut self, pane_id: &str) -> Result<(), SurfaceError> {
        if !self.panes.iter().any(|pane| pane.id == pane_id) {
            return Err(SurfaceError::PaneNotFound(pane_id.to_string()));
        }
        self.active_pane_id = pane_id.to_string();
        Ok(())
    }

    pub fn select_block(&mut self, pane_id: &str, block_id: &str) -> Result<(), SurfaceError> {
        let pane = self
            .panes
            .iter_mut()
            .find(|pane| pane.id == pane_id)
            .ok_or_else(|| SurfaceError::PaneNotFound(pane_id.to_string()))?;
        pane.select_block(block_id)
    }

    pub fn validate(&self) -> Result<(), SurfaceError> {
        if self.panes.is_empty() {
            return Err(SurfaceError::EmptyPaneSet(self.id.clone()));
        }

        let mut pane_ids = HashSet::new();
        for pane in &self.panes {
            if !pane_ids.insert(pane.id.as_str()) {
                return Err(SurfaceError::DuplicatePaneId(pane.id.clone()));
            }
            pane.validate()?;
        }

        if !self.panes.iter().any(|pane| pane.id == self.active_pane_id) {
            return Err(SurfaceError::PaneNotFound(self.active_pane_id.clone()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalPane {
    pub id: String,
    pub cwd: String,
    pub shell: String,
    #[serde(default)]
    pub current_input: String,
    #[serde(default)]
    pub focus: PaneFocus,
    #[serde(default)]
    pub selected_block_id: Option<String>,
    #[serde(default)]
    pub blocks: Vec<BlockCardView>,
    #[serde(default)]
    pub quick_actions: Vec<BlockQuickAction>,
}

impl TerminalPane {
    pub fn new(
        id: impl Into<String>,
        cwd: impl Into<String>,
        shell: impl Into<String>,
        blocks: Vec<BlockCardView>,
    ) -> Self {
        Self {
            id: id.into(),
            cwd: cwd.into(),
            shell: shell.into(),
            current_input: String::new(),
            focus: PaneFocus::Prompt,
            selected_block_id: None,
            blocks,
            quick_actions: BlockQuickAction::defaults(),
        }
    }

    pub fn from_timeline(
        id: impl Into<String>,
        cwd: impl Into<String>,
        shell: impl Into<String>,
        timeline: &BlockTimeline,
    ) -> Self {
        Self::new(
            id,
            cwd,
            shell,
            timeline
                .blocks()
                .iter()
                .map(BlockCardView::from_block)
                .collect(),
        )
    }

    pub fn select_block(&mut self, block_id: &str) -> Result<(), SurfaceError> {
        if !self.blocks.iter().any(|block| block.id == block_id) {
            return Err(SurfaceError::BlockNotFound(block_id.to_string()));
        }
        self.selected_block_id = Some(block_id.to_string());
        self.focus = PaneFocus::Block;
        Ok(())
    }

    pub fn validate(&self) -> Result<(), SurfaceError> {
        let mut block_ids = HashSet::new();
        for block in &self.blocks {
            if !block_ids.insert(block.id.as_str()) {
                return Err(SurfaceError::DuplicateBlockId(block.id.clone()));
            }
        }

        if let Some(selected_block_id) = &self.selected_block_id
            && !self
                .blocks
                .iter()
                .any(|block| &block.id == selected_block_id)
        {
            return Err(SurfaceError::BlockNotFound(selected_block_id.clone()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum PaneFocus {
    #[default]
    Prompt,
    Block,
    OutputSelection,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BlockCardView {
    pub id: String,
    pub cwd: String,
    pub command: String,
    pub status: BlockStatus,
    pub exit_code: Option<i32>,
    pub duration_ms: Option<u128>,
    pub output_preview: String,
    pub folded: bool,
    pub actions: Vec<BlockQuickAction>,
}

impl BlockCardView {
    pub fn from_block(block: &CommandBlock) -> Self {
        Self {
            id: block.id.clone(),
            cwd: block.cwd.clone(),
            command: block.command.clone(),
            status: block.status,
            exit_code: block.exit_code,
            duration_ms: block.duration_ms(),
            output_preview: block.output_preview.clone(),
            folded: false,
            actions: BlockQuickAction::defaults(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum BlockQuickAction {
    CopyCommand,
    CopyOutputPreview,
    Rerun,
    Explain,
    FixWithAgent,
    CreateIssue,
}

impl BlockQuickAction {
    pub fn defaults() -> Vec<Self> {
        vec![
            Self::CopyCommand,
            Self::CopyOutputPreview,
            Self::Rerun,
            Self::Explain,
            Self::FixWithAgent,
            Self::CreateIssue,
        ]
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum PaneSplit {
    Single,
    Horizontal,
    Vertical,
    Grid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentPanelState {
    pub visible: bool,
    pub active_tab: AgentPanelTab,
    pub width_fraction: f32,
    pub context_chips: Vec<AgentContextChip>,
}

impl AgentPanelState {
    pub fn select_tab(&mut self, tab: AgentPanelTab) {
        self.active_tab = tab;
    }

    pub fn add_context_chip(&mut self, chip: AgentContextChip) -> Result<(), SurfaceError> {
        if self
            .context_chips
            .iter()
            .any(|existing| existing.id == chip.id)
        {
            return Err(SurfaceError::DuplicateContextChipId(chip.id));
        }
        self.context_chips.push(chip);
        Ok(())
    }

    pub fn remove_context_chip(&mut self, chip_id: &str) -> Result<AgentContextChip, SurfaceError> {
        let index = self
            .context_chips
            .iter()
            .position(|chip| chip.id == chip_id)
            .ok_or_else(|| SurfaceError::ContextChipNotFound(chip_id.to_string()))?;
        Ok(self.context_chips.remove(index))
    }
}

impl Default for AgentPanelState {
    fn default() -> Self {
        Self {
            visible: true,
            active_tab: AgentPanelTab::Chat,
            width_fraction: DEFAULT_AGENT_PANEL_WIDTH_FRACTION,
            context_chips: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum AgentPanelTab {
    Chat,
    Plan,
    Tools,
    Memory,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentContextChip {
    pub id: String,
    pub label: String,
    pub payload: ContextChipPayload,
    pub removable: bool,
}

impl AgentContextChip {
    pub fn new(
        id: impl Into<String>,
        label: impl Into<String>,
        payload: ContextChipPayload,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            payload,
            removable: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ContextChipPayload {
    SelectedBlock {
        pane_id: String,
        block_id: String,
    },
    LastCommand {
        pane_id: String,
        block_id: String,
    },
    GitDiff {
        snapshot_id: String,
    },
    FilesystemSelection {
        snapshot_id: String,
        root: String,
        paths: Vec<String>,
    },
    CodeGraph {
        query: String,
    },
    PluginProvided {
        plugin_id: String,
        payload_id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PrivacyFooterState {
    pub provider_scope: ProviderScope,
    pub provider_name: String,
    pub network_destination: String,
    pub telemetry_enabled: bool,
    pub cloud_handoff_enabled: bool,
    pub plugins_with_terminal_access: usize,
    pub plugins_with_filesystem_access: usize,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderScope {
    Local,
    PrivateNetwork,
    Remote,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandPaletteState {
    pub actions: Vec<CommandAction>,
}

impl Default for CommandPaletteState {
    fn default() -> Self {
        Self {
            actions: vec![
                CommandAction::new(
                    "workspace.open-command-palette",
                    "Open command palette",
                    CommandActionScope::Workspace,
                    Some("Cmd+K"),
                ),
                CommandAction::new(
                    "terminal.focus-input",
                    "Focus terminal input",
                    CommandActionScope::Terminal,
                    Some("Cmd+L"),
                ),
                CommandAction::new(
                    "agent.focus-prompt",
                    "Focus agent prompt",
                    CommandActionScope::Agent,
                    Some("Cmd+I"),
                ),
                CommandAction::new(
                    "agent.select-model",
                    "Select model",
                    CommandActionScope::Agent,
                    Some("Cmd+Shift+M"),
                ),
                CommandAction::new(
                    "plugin.open-permissions",
                    "Open plugin permissions",
                    CommandActionScope::Plugin,
                    Some("Cmd+Shift+P"),
                ),
                CommandAction::new(
                    "workspace.close-overlay",
                    "Close overlay",
                    CommandActionScope::Workspace,
                    Some("Esc"),
                ),
                CommandAction::new(
                    "privacy.open-status",
                    "Open privacy status",
                    CommandActionScope::Privacy,
                    None,
                ),
            ],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommandAction {
    pub id: String,
    pub label: String,
    pub scope: CommandActionScope,
    pub keybinding: Option<String>,
}

impl CommandAction {
    pub fn new(
        id: impl Into<String>,
        label: impl Into<String>,
        scope: CommandActionScope,
        keybinding: Option<&str>,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            scope,
            keybinding: keybinding.map(str::to_string),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum CommandActionScope {
    Workspace,
    Terminal,
    Agent,
    Plugin,
    Privacy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SurfaceError {
    EmptyTabSet,
    EmptyPaneSet(String),
    DuplicateTabId(String),
    TabNotFound(String),
    DuplicatePaneId(String),
    PaneNotFound(String),
    DuplicateBlockId(String),
    BlockNotFound(String),
    DuplicateContextChipId(String),
    ContextChipNotFound(String),
}

impl fmt::Display for SurfaceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SurfaceError::EmptyTabSet => write!(formatter, "workspace surface has no tabs"),
            SurfaceError::EmptyPaneSet(id) => write!(formatter, "tab '{id}' has no panes"),
            SurfaceError::DuplicateTabId(id) => write!(formatter, "tab '{id}' already exists"),
            SurfaceError::TabNotFound(id) => write!(formatter, "tab '{id}' was not found"),
            SurfaceError::DuplicatePaneId(id) => write!(formatter, "pane '{id}' already exists"),
            SurfaceError::PaneNotFound(id) => write!(formatter, "pane '{id}' was not found"),
            SurfaceError::DuplicateBlockId(id) => write!(formatter, "block '{id}' already exists"),
            SurfaceError::BlockNotFound(id) => write!(formatter, "block '{id}' was not found"),
            SurfaceError::DuplicateContextChipId(id) => {
                write!(formatter, "context chip '{id}' already exists")
            }
            SurfaceError::ContextChipNotFound(id) => {
                write!(formatter, "context chip '{id}' was not found")
            }
        }
    }
}

impl Error for SurfaceError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_surface_opens_directly_into_terminal_workspace() {
        let surface = WorkspaceSurface::demo_local();

        assert_eq!(surface.workspace_name, "project");
        assert_eq!(surface.tabs.len(), 1);
        let pane = surface
            .active_tab()
            .expect("active tab")
            .active_pane()
            .expect("active pane");
        assert_eq!(pane.current_input, "");
        assert_eq!(pane.focus, PaneFocus::Block);
        assert_eq!(pane.selected_block_id.as_deref(), Some("demo-cargo-test"));
        assert!(surface.agent_panel.visible);
        assert_eq!(surface.agent_panel.active_tab, AgentPanelTab::Chat);
        assert_eq!(surface.privacy_footer.provider_scope, ProviderScope::Local);
        assert!(!surface.privacy_footer.telemetry_enabled);
        assert!(!surface.privacy_footer.cloud_handoff_enabled);
        surface.validate().expect("valid surface");
    }

    #[test]
    fn tabs_and_panes_have_stable_selection() {
        let mut surface = WorkspaceSurface::demo_local();
        let tab = WorkspaceTab::new(
            "tab-logs",
            "logs",
            TerminalPane::new("pane-logs", "/tmp", "zsh", Vec::new()),
        );
        surface.add_tab(tab).expect("add tab");
        surface.select_tab("tab-logs").expect("select tab");

        let active = surface.active_tab().expect("active tab");
        assert_eq!(active.title, "logs");

        let active = surface.active_tab_mut().expect("active tab");
        active
            .add_pane(
                TerminalPane::new("pane-tail", "/tmp", "zsh", Vec::new()),
                PaneSplit::Vertical,
            )
            .expect("add pane");
        active.select_pane("pane-tail").expect("select pane");

        let active = surface.active_tab().expect("active tab");
        assert_eq!(active.active_pane().expect("active pane").id, "pane-tail");
        assert_eq!(active.split, PaneSplit::Vertical);
    }

    #[test]
    fn duplicate_tab_and_pane_ids_are_rejected() {
        let mut surface = WorkspaceSurface::demo_local();
        let duplicate_tab = WorkspaceTab::new(
            "tab-main",
            "duplicate",
            TerminalPane::new("pane-other", "/tmp", "zsh", Vec::new()),
        );
        assert_eq!(
            surface.add_tab(duplicate_tab).expect_err("duplicate tab"),
            SurfaceError::DuplicateTabId("tab-main".to_string())
        );

        let active = surface.active_tab_mut().expect("active tab");
        assert_eq!(
            active
                .add_pane(
                    TerminalPane::new("pane-main", "/tmp", "zsh", Vec::new()),
                    PaneSplit::Horizontal,
                )
                .expect_err("duplicate pane"),
            SurfaceError::DuplicatePaneId("pane-main".to_string())
        );
    }

    #[test]
    fn agent_context_chips_are_resolvable_and_removable_by_user() {
        let mut surface = WorkspaceSurface::demo_local();
        let selected_block = surface
            .agent_panel
            .context_chips
            .iter()
            .find(|chip| chip.id == "chip-selected-block")
            .expect("selected block chip");

        assert_eq!(
            selected_block.payload,
            ContextChipPayload::SelectedBlock {
                pane_id: "pane-main".to_string(),
                block_id: "demo-cargo-test".to_string()
            }
        );

        let removed = surface
            .agent_panel
            .remove_context_chip("chip-git-diff")
            .expect("remove chip");

        assert_eq!(removed.label, "git diff");
        assert!(
            !surface
                .agent_panel
                .context_chips
                .iter()
                .any(|chip| chip.id == "chip-git-diff")
        );
    }

    #[test]
    fn command_palette_contains_keyboard_first_actions() {
        let surface = WorkspaceSurface::demo_local();

        for (id, keybinding) in [
            ("workspace.open-command-palette", "Cmd+K"),
            ("terminal.focus-input", "Cmd+L"),
            ("agent.focus-prompt", "Cmd+I"),
            ("agent.select-model", "Cmd+Shift+M"),
            ("plugin.open-permissions", "Cmd+Shift+P"),
            ("workspace.close-overlay", "Esc"),
        ] {
            assert!(surface.command_palette.actions.iter().any(|action| {
                action.id == id && action.keybinding.as_deref() == Some(keybinding)
            }));
        }
    }

    #[test]
    fn surface_serializes_bounded_block_cards_not_terminal_timeline_internals() {
        let surface = WorkspaceSurface::demo_local();
        let serialized = serde_json::to_string(&surface).expect("serialize");

        assert!(serialized.contains("\"agent_panel\""));
        assert!(serialized.contains("\"privacy_footer\""));
        assert!(serialized.contains("\"output_preview\""));
        assert!(!serialized.contains("\"output\""));
        assert!(!serialized.contains("\"byte_len\""));
        assert!(!serialized.contains("winit"));
        assert!(!serialized.contains("gpui"));
        assert!(!serialized.contains("wgpu"));
    }

    #[test]
    fn deserialized_surface_invariants_are_validated() {
        let raw = r#"{
            "workspace_name": "bad",
            "active_tab_id": "missing",
            "tabs": [],
            "agent_panel": {
                "visible": true,
                "active_tab": "chat",
                "width_fraction": 0.34,
                "context_chips": []
            },
            "privacy_footer": {
                "provider_scope": "local",
                "provider_name": "oMLX",
                "network_destination": "127.0.0.1:8000",
                "telemetry_enabled": false,
                "cloud_handoff_enabled": false,
                "plugins_with_terminal_access": 0,
                "plugins_with_filesystem_access": 0
            }
        }"#;

        let surface: WorkspaceSurface = serde_json::from_str(raw).expect("deserialize");
        assert_eq!(surface.command_palette.actions.len(), 7);
        assert_eq!(
            surface.validate().expect_err("invalid tab set"),
            SurfaceError::EmptyTabSet
        );
    }

    #[test]
    fn validate_rejects_selected_blocks_that_do_not_exist() {
        let mut surface = WorkspaceSurface::demo_local();
        let pane = &mut surface
            .active_tab_mut()
            .expect("active tab")
            .panes
            .first_mut()
            .expect("first pane");
        pane.selected_block_id = Some("missing-block".to_string());

        assert_eq!(
            surface.validate().expect_err("missing selected block"),
            SurfaceError::BlockNotFound("missing-block".to_string())
        );
    }
}
