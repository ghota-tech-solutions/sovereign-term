# Contributing

Thanks for helping build Sovereign Term.

## Development

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Product Bar

Changes should preserve the local-first promise. If a feature sends context, prompts, command output, files, or secrets to a remote service, the user must opt in explicitly and the destination must be auditable.

## Pull Requests

Please include:

- what changed
- why it matters
- how privacy and permissions are affected
- how it was tested

## Plugin Contributions

Plugins must include a manifest and a clear permissions rationale. Avoid broad capabilities when narrower ones are enough.
