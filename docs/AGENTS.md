# Agent integration

Use the installed `aigc` executable. Do not copy its deletion logic into shell scripts.

1. Run `aigc status --json`. Inspect `warnings`, `complete`, `status`, and `reason`.
2. Run builds or foreground sessions through `aigc run -- <command>`. Pins are appropriate for work that must survive between sessions.
3. Register a worktree or virtual device only when its owner has authorized it as disposable. `manage` is an authorization, not an activity probe.
4. Run `aigc plan --json` to review current decisions. `clean` only acts on eligible resources and rechecks them.
5. Report actual outcomes from `history`; never add overlapping category sizes into a claimed reclaimed total.

Use `config set` to change policy on the user's behalf. Do not edit `state.json` to manufacture old observations. Do not disable protection to force a free-space target.

A resource protected as application-managed needs its owning app's archive operation. A locked, active, modified, unknown or pinned resource is not disposable merely because a task completed.

Examples:

```sh
aigc status worktrees --json
aigc config set min-free-space 25GB
aigc pin /absolute/path/to/current-task
aigc run -- cargo build
aigc unpin /absolute/path/to/current-task
```

Global caches, shared SDKs and simulator runtimes are report-only in v0.1. Do not describe these as automatically collected.
