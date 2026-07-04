# Privacy Model

Sovereign Term treats privacy as a product invariant.

## Default State

- Telemetry is disabled.
- Cloud handoff is disabled.
- The default model provider is local oMLX on `127.0.0.1`.
- The CLI prints the model network destination before a request.
- Public internet model endpoints are blocked unless their provider sets `allow_remote = true`.
- API keys are read from environment variables or local config and are not printed.

Run `sovereign-term offline check` to audit the active config before invoking an agent. The check fails when telemetry or cloud handoff are enabled, when the default provider points at public internet, or when any public provider is explicitly allowed for remote access.

## Agent Context Contract

The agent panel must make attached context explicit before a prompt is sent. `sovereign-ui` serializes a context manifest for every attached chip, including the source type and privacy flags for terminal output previews, filesystem content reads, patch contents, and remote network usage.

Filesystem snapshots and Git diff summaries are metadata context by default. Filesystem read previews are explicit content reads and are marked as such in the manifest. Git diff summaries must keep `patch_contents_included = false` until a future patch-preview gate is added.

## Local Provider Contract

A local provider is expected to be reachable on loopback or a private network address. Sovereign Term allows loopback and private network endpoints for providers with `allow_remote = false`; public internet hosts require explicit opt-in.

Example:

```toml
[providers.omlx]
display_name = "oMLX Local"
endpoint = "http://127.0.0.1:8000/v1/chat/completions"
model = "ornith-local-agent"
api_key_env = "OMLX_API_KEY"
request_timeout_secs = 120
allow_remote = false
```

## Remote Provider Contract

Remote providers should require `allow_remote = true` and UI warnings before use. Remote calls must show their destination clearly.

## Plugin Contract

Plugins declare capabilities in their manifest. The host should deny capabilities that were not declared and approved.

## Future Hardening

- OS keychain integration
- per-provider network audit log
- offline mode
- signed plugin manifests
- sandboxed WASM plugin runtime
