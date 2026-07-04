# Block Engine

The block engine turns shell commands into first-class local data. This is the core primitive behind a block-based terminal surface, local search, and agent context selection.

## Goals

- Track command lifecycle from start to finish.
- Preserve stdout and stderr as separate streams.
- Store timing and exit status.
- Keep a bounded output preview for UI and agent context.
- Search blocks locally.
- Build explicit agent context only from selected blocks.

## Domain Model

```text
BlockTimeline
  CommandBlock
    id
    cwd
    command
    status
    started_at_ms
    finished_at_ms
    exit_code
    output_preview
    OutputChunk[]
```

`OutputChunk` stores:

- stream: `stdout`, `stderr`, or `system`
- text decoded lossily from terminal bytes
- original byte length
- received timestamp

## Privacy Contract

The block archive is local application state. Creating, searching, and rendering blocks must not send command output to a model or remote service.

Agent context is opt-in:

- selected terminal text
- selected command blocks
- selected files or plugin context

The agent panel should show these context chips before a prompt is sent.

`BlockTimeline` and `CommandBlock` represent the local archive and may retain full output chunks on disk. Agent-facing APIs must use `AgentContextBundle` / `AgentBlockContext`, which contain bounded output previews and metadata only. Serializing a terminal snapshot for an agent must not serialize full `OutputChunk` data.

## UI Implications

The GUI can render from `BlockTimeline` without needing to parse raw scrollback repeatedly. It can also offer block actions like:

- explain output
- rerun
- copy command
- copy output
- create issue from failure

Those actions should route through permission gates when they call a model, shell, filesystem, network, or plugin tool.
