# Architecture Overview

Sovereign Term is split into replaceable subsystems so the product can evolve without turning into one large terminal monolith.

## Crates

- `sovereign-core`: configuration, privacy defaults, local paths, provider resolution.
- `sovereign-agent`: OpenAI-compatible model client and future local agent runtime.
- `sovereign-terminal`: terminal snapshots, command blocks, PTY-facing domain types.
- `sovereign-plugin`: plugin manifests, activation events, capability declarations.
- `sovereign-term`: developer CLI and future app entrypoint.

See [block-engine.md](block-engine.md) for the local command-block data model.

## Runtime Boundaries

```text
terminal output -> command block model -> agent context builder
user prompt -> provider resolver -> local model endpoint
plugin manifest -> permission gate -> runtime activation
```

The model provider never receives terminal context unless the user invokes an agent action that requires it. Remote providers must be configured explicitly with `allow_remote = true`.

## Terminal Engine Direction

The current crate only defines domain types. The GUI milestone will evaluate:

- `alacritty_terminal` for PTY, VTE, and grid state
- `gpui-terminal` for a GPUI-native embedded terminal path
- `winit`/`wgpu` if we need tighter renderer control
- `libghostty-vt` when its C/Zig API is mature enough for stable embedding

## Agent Runtime Direction

The agent runtime should remain provider-agnostic:

- planner
- tool registry
- permission gate
- model provider
- network audit log
- transcript storage

Tool calls should be explicit data structures, not ad hoc shell strings.

## Plugin Direction

Plugins start with process and WASM entry kinds. Long term, plugins should run in constrained sandboxes with a typed host API.

Initial capability set:

- `read-terminal`
- `write-terminal`
- `read-workspace`
- `write-workspace`
- `shell`
- `network`
- `model`

The default policy should deny broad capabilities unless the user enables the plugin.
