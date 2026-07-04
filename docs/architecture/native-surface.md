# Native Surface

The first native surface is modeled before choosing a renderer. `sovereign-ui` owns the renderer-agnostic state a GPUI, winit, wgpu, or other Rust shell can consume.

## Initial Scope

- workspace tabs with active selection
- terminal panes with split direction and active pane
- bounded terminal block cards derived from command blocks
- docked agent panel with Chat, Plan, Tools, and Memory tabs
- removable agent context chips with resolvable payload IDs
- privacy footer state
- command palette actions and keybinding metadata

The app still opens directly into the terminal workspace. There is no landing page or marketing shell.

## Renderer Boundary

`sovereign-ui` must not depend on a windowing framework. It serializes plain state so future renderers can be compared without rewriting product behavior.

The surface also avoids serializing raw terminal event internals. `BlockTimeline` remains the local command history model, while `BlockCardView` is the bounded UI projection used by renderers and demos.

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
