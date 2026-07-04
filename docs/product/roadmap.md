# Roadmap

## Milestone 0: Local Runtime

- Rust workspace and CLI
- local oMLX/OpenAI-compatible chat path
- local config generation
- plugin manifest validation
- privacy defaults and network destination logging

## Milestone 1: Terminal Surface

- native app shell
- PTY-backed terminal
- tabs and panes
- block-based command history
- searchable output
- command status and timing

## Milestone 2: Agent Panel

- AI panel attached to terminal context
- selected-text actions
- explain command output
- propose shell commands with confirmation
- local transcript storage
- provider/model switcher

## Milestone 3: Tooling

- shell tool with confirmation policies
- filesystem read/write tools
- git diff/status tools
- MCP client support
- codebase-memory integration

## Milestone 4: Plugin Platform

- plugin registry folder
- manifest permissions UI
- process plugin host
- WASM plugin host
- signed plugin support

## Milestone 5: Privacy Hardening

- network audit view
- local secret store integration
- offline mode
- remote provider warnings
- reproducible builds
