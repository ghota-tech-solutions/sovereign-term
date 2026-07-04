# Git Plugin

The Git plugin starts as local repository introspection. It must not contact remote hosts unless the user explicitly invokes a remote action.

## Initial Scope

- discover the repository root
- read current branch and HEAD
- summarize staged, unstaged, untracked, and conflicting paths
- summarize staged and unstaged diff metadata without patch contents
- serialize a local snapshot for UI/plugin use

## Privacy Contract

`sovereign-git` reads `.git`, the index, and the worktree. It does not fetch, push, pull, or call GitHub. The `remote_network_used` field is part of the public snapshot so UI and tests can enforce the local-only invariant.

The first snapshot implementation intentionally avoids Git status paths that can execute configured clean/smudge filters. Unstaged detection is metadata-based, so it may be conservative compared with a full content diff. Rich diffs and content-aware checks require a separate explicit permission gate.

`sovereign-git` diff summaries expose path-level `--numstat` metadata only. They disable external diff, textconv, repo fsmonitor hooks, and configured filter commands before running the local diff engine. They set `patch_contents_included = false` and are intended for UI badges and agent context chips before the user explicitly asks for a patch preview.

## Future Scope

- safe commit drafting
- branch switch/create flows
- patch previews behind explicit content-read permission
- PR helpers gated behind explicit remote permissions
- workspace-specific approval rules for write operations
