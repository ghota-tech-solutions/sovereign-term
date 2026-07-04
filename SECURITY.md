# Security Policy

Sovereign Term is early software. Do not run it against sensitive systems without reviewing the code and configuration.

## Reporting

Please report security issues privately to the maintainers before opening a public issue. If private reporting is not yet configured on GitHub, contact the repository owner directly.

## Security Goals

- No telemetry by default.
- Local inference works without public tunnels or hosted routing.
- Network destinations are visible before model calls.
- Secrets are never logged intentionally.
- Shell and filesystem tools require explicit permission gates.
- Plugins declare capabilities before activation.

## Non-Goals For Early Milestones

- The first CLI milestone is not a sandbox.
- Process plugins are not isolated yet.
- GUI terminal rendering is not implemented yet.
- Local model quality and safety depend on the configured provider.
