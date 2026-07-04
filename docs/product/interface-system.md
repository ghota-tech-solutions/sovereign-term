# Interface System

Sovereign Term aims for the ergonomics of modern block-based terminals while keeping a distinct visual identity and a stronger local-first contract.

## Product Feel

The interface should feel:

- quiet and dense
- keyboard-first
- fast enough to be trusted as the daily terminal
- explicit about privacy state
- friendly to agents without making the agent the whole product

The design can learn from Warp-style terminal blocks, command palettes, side panels, and rich command output, but it must not copy Warp branding, assets, icons, copy, color palette, or proprietary interaction details.

## First Screen

The app opens directly into the terminal workspace.

```text
+---------------------------------------------------------------------+
| Workspace: project-name                         Local: oMLX Ornith  |
+-----------------------------+---------------------------------------+
| terminal blocks             | Agent                                 |
|                             |                                       |
| > cargo test                | Context                               |
|   running 4 tests...        | [selected block] [git diff] [files]   |
|   ok                        |                                       |
|                             | Ask                                   |
| > git status                | +-----------------------------------+ |
|   modified: src/main.rs     | | Explain this failure...          | |
|                             | +-----------------------------------+ |
| > _                         |                                       |
+-----------------------------+---------------------------------------+
| Privacy: local-only | Network: 127.0.0.1:8000 | Plugins: 2 active   |
+---------------------------------------------------------------------+
```

No landing page. No marketing shell. The user's terminal is the product.

## Layout Regions

### Workspace Header

Purpose:

- current workspace/project
- current profile
- local or remote model state
- command palette entry point

Expected controls:

- profile selector
- model selector
- plugin status
- privacy state

### Terminal Block Stream

Each shell command is represented as a block.

Block anatomy:

- prompt row
- command text
- status chip
- duration
- cwd marker when it differs from the workspace root
- stdout/stderr body
- folded output state
- quick actions

Block actions:

- copy command
- copy output
- rerun
- explain
- fix with agent
- create issue/task from output

Block states:

- running
- succeeded
- failed
- cancelled
- backgrounded
- permission-required

### Agent Panel

The agent panel is docked, not modal-first. It can be hidden completely.

Expected tabs:

- Chat
- Plan
- Tools
- Memory

Agent context chips:

- selected block
- last command
- git diff
- file selection
- code graph
- plugin-provided context

Every context chip should be removable before sending.

### Privacy Footer

The privacy footer is always visible when the agent is enabled.

It shows:

- provider kind: local, private-network, remote
- exact network destination
- whether telemetry is disabled
- whether cloud handoff is disabled
- whether a plugin can access terminal or files

Examples:

- `Local-only | oMLX | 127.0.0.1:8000 | telemetry off`
- `Remote provider | api.example.com | review context before send`

## Visual Tokens

Color should carry state, not decoration.

```text
surface.base       #0e1116
surface.raised     #151a21
surface.hover      #1d242d
text.primary       #e7edf3
text.secondary     #95a3b4
accent.local       #3ddc97
accent.remote      #ffb454
accent.danger      #ff6b6b
accent.agent       #67b7ff
border.subtle      #27313c
```

Rounded corners should be restrained. Cards are for repeated items, modals, and framed tools only.

## Interaction Model

Keyboard-first commands:

- `Cmd+K`: command palette
- `Cmd+L`: focus terminal input
- `Cmd+I`: focus agent prompt
- `Cmd+Shift+M`: model selector
- `Cmd+Shift+P`: plugin permissions
- `Esc`: close overlays

Mouse interactions:

- click block to select
- drag to select terminal text
- hover block actions
- resize agent panel
- drag tabs and panes

## Permission Prompts

Permission prompts must be specific and interruptible.

Good:

```text
Git Helper wants to read git status in /Users/me/project.
[Allow once] [Always allow for this workspace] [Deny]
```

Bad:

```text
Plugin wants workspace access.
```

## Plugin Surfaces

Plugins can contribute:

- command palette actions
- context chips
- agent tools
- block quick actions
- side panel views
- status footer items

Plugins cannot silently:

- send terminal output over the network
- read file contents
- write files
- execute shell commands
- call a model

Those actions require declared capabilities and user approval.

## Offline Behavior

When offline:

- terminal works normally
- local model providers continue to work
- plugin actions that do not require network continue to work
- remote model providers show disabled state
- no UI nags the user to sign in

## Accessibility

Minimum expectations:

- all actions are keyboard reachable
- state is not color-only
- focus rings are visible
- terminal font sizing is independent from UI font sizing
- reduced motion mode disables non-essential animation
