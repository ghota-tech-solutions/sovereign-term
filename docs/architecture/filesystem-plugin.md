# Filesystem Plugin

The filesystem plugin begins with metadata-only workspace snapshots. It should make local project structure available to the UI and agent without silently reading file contents.

## Initial Scope

- snapshot an explicit workspace root
- collect path, entry kind, byte length, modified timestamp, and readonly bit
- avoid file contents
- preview a named file only through an explicit, byte-bounded read command
- ignore common high-volume or sensitive directories by default
- avoid following symlinks outside the explicit workspace root
- expose a CLI smoke-test command

## Privacy Contract

`sovereign-fs` treats file contents as a separate permission tier. Metadata snapshots set `file_contents_read = false` and must not include file body text in serialized output.

Read previews are explicit content reads. They require the caller to provide a workspace root and file path, reject paths outside the canonical root, reject directories and symlinks, apply the same hidden and ignored-name defaults, cap bytes read by policy, and set `file_contents_read = true`.

Default ignored names include:

- `.git`
- `.codebase-memory`
- `target`
- `node_modules`
- `.next`
- `.turbo`
- `.cache`

File edits, deletes, moves, and writes require future explicit gates and should produce auditable action records.

Symlinks are reported as metadata. Even when symlink traversal is enabled by a future caller, traversal must remain inside the canonical workspace root and the canonical target path must satisfy the same hidden and ignored-name policy as a normal path.

Snapshot traversal is bounded by `max_entries` before directory contents are queued so large directories cannot silently expand the snapshot beyond the caller's budget.

## Future Scope

- write/edit permission prompts
- stable directory-handle traversal for adversarial concurrent mutation cases
- safe backups before destructive writes
- path allowlists per workspace
- plugin-contributed file actions
