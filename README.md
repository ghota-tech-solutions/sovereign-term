# Sovereign Term

Sovereign Term is a local-first, agentic terminal for developers who want the power of modern AI workflows without hidden server-side routing.

The product goal is simple: if you configure local inference, your terminal context, prompts, code, and command output stay on your machine.

## Why This Exists

Modern AI terminals are polished, but many route custom model requests through hosted agent infrastructure. That is convenient for cloud handoff, but it is the wrong default for sensitive codebases, offline work, local model labs, and teams that need clear data boundaries.

Sovereign Term is designed around the opposite contract:

- local inference first
- remote providers only when explicitly configured
- no telemetry by default
- visible network destinations
- plugin permissions that are declared up front
- shell and filesystem actions gated by policy

## Current Status

This repository is intentionally early. The first milestone is a compilable Rust runtime that proves the local-first contract:

- workspace architecture split into `core`, `agent`, `terminal`, and `plugin` crates
- OpenAI-compatible chat client for oMLX, Ollama, LM Studio, vLLM, and similar servers
- default oMLX provider targeting `http://127.0.0.1:8000/v1/chat/completions`
- privacy defaults with telemetry and cloud handoff disabled
- plugin manifest model with explicit capabilities
- CLI commands for config, provider inspection, local chat, and plugin validation

The graphical terminal UI comes next.

## Quick Start

```sh
cargo run -- doctor
cargo run -- init-config
export OMLX_API_KEY="your-local-omlx-key"
cargo run -- chat --prompt "Say hello from local inference"
```

By default, the local provider uses:

- endpoint: `http://127.0.0.1:8000/v1/chat/completions`
- model: `ornith-local-agent`
- code model: `code-local-agent`
- API key env var: `OMLX_API_KEY`

## Product Direction

Sovereign Term is inspired by the ergonomics of modern block-based terminals:

- command blocks with status, timing, and searchable output
- an AI side panel that can see selected terminal context
- profile-aware model routing
- project memory through MCP servers
- agent tools for shell, git, filesystem, and code graph analysis
- extension points for plugins and local workflow automation

It is not a clone. The differentiator is the trust boundary: local-first is a product feature, not a hidden implementation detail.

## Architecture

```text
Sovereign Term
  app shell
    tabs, panes, blocks, command palette, AI panel
  terminal engine
    PTY, VTE parser, command blocks, renderer adapter
  agent runtime
    local planner, model provider, tool permission gates
  model providers
    OpenAI-compatible local endpoints, optional cloud providers
  plugin host
    manifest, permissions, activation events, sandbox strategy
  privacy layer
    network audit log, local config, local secrets, telemetry off by default
```

See [docs/architecture/overview.md](docs/architecture/overview.md).
See [docs/product/privacy-model.md](docs/product/privacy-model.md) for the privacy contract.

## Plugin Philosophy

Plugins should make workflows richer without silently expanding the trust boundary. Every plugin declares:

- how it starts
- when it activates
- what it can read or write
- whether it can use network, shell, workspace, terminal, or model capabilities

See [examples/plugins/git-helper.toml](examples/plugins/git-helper.toml).

## Roadmap

See [docs/product/roadmap.md](docs/product/roadmap.md).

## License

Apache-2.0. See [LICENSE](LICENSE).
