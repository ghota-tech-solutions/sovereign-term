# Privacy Model

Sovereign Term treats privacy as a product invariant.

## Default State

- Telemetry is disabled.
- Cloud handoff is disabled.
- The default model provider is local oMLX on `127.0.0.1`.
- The CLI prints the model network destination before a request.
- API keys are read from environment variables or local config and are not printed.

## Local Provider Contract

A local provider is expected to be reachable on loopback or a private host. Sovereign Term does not reject loopback endpoints; they are the preferred path.

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
