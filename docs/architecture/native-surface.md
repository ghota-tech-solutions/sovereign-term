# Native Surface

The first native surface is modeled before choosing a renderer. `sovereign-ui` owns the renderer-agnostic state a GPUI, winit, wgpu, or other Rust shell can consume.

## Initial Scope

- workspace tabs with active selection
- terminal panes with split direction and active pane
- bounded terminal block cards derived from command blocks
- docked agent panel with Chat, Plan, Tools, and Memory tabs
- removable agent context chips with resolvable payload IDs
- auditable agent context manifest with per-chip source and privacy flags
- privacy footer state
- command palette actions and keybinding metadata

The app still opens directly into the terminal workspace. There is no landing page or marketing shell.

## Renderer Boundary

`sovereign-ui` must not depend on a windowing framework. It serializes plain state so future renderers can be compared without rewriting product behavior.

The surface also avoids serializing raw terminal event internals. `BlockTimeline` remains the local command history model, while `BlockCardView` is the bounded UI projection used by renderers and demos.

Agent context chips carry a structured manifest alongside their visual labels. The manifest classifies terminal block previews, Git diff metadata, filesystem snapshots, filesystem read previews, code graph queries, and plugin-provided context. It also carries privacy flags for terminal output previews, filesystem content reads, patch contents, and remote network usage so the agent panel can show what will be attached before a prompt is sent.

```text
BlockTimeline -> BlockCardView -> WorkspaceSurface -> renderer adapter
                             |
                             +-> AgentPanelState
                             +-> PrivacyFooterState
                             +-> CommandPaletteState
```

## Privacy Contract

The privacy footer is part of the surface state, not a decoration. It carries:

- provider scope: local, private network, or remote
- provider name
- exact network destination
- telemetry state
- cloud handoff state
- counts of plugins with terminal or filesystem access

Renderer implementations should keep this state visible whenever the agent panel is enabled.

## Future Scope

- PTY-backed pane lifecycle
- renderer adapter traits
- command palette dispatch
- split resizing
- persisted workspace layouts
- accessibility focus model
