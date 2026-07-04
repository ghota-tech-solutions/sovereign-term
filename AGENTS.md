# Agent Guidelines

Sovereign Term is a local-first AI terminal. Preserve that contract in every change.

## Principles

- Local inference must work without cloud routing.
- Telemetry stays off by default.
- Any remote network destination must be visible to the user.
- Shell, filesystem, git, and model tools must pass through explicit permission boundaries.
- Plugins declare capabilities before activation.
- Keep the app modular: terminal, agent, plugin, and UI concerns should remain separable.

## Coding Notes

- Prefer small crates with clear ownership.
- Keep platform-specific code behind interfaces.
- Do not add hosted services as required infrastructure.
- Do not store secrets in project files.
- Tests should cover privacy and permission behavior before UI polish.
