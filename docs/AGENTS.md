# Agent integration

Use the installed `aigc` executable. Do not copy its deletion logic into shell scripts.

1. Run `aigc status --json`. Inspect `warnings`, `complete`, `status`, and `reason`.
2. Run builds or foreground sessions through `aigc run -- <command>`. Pins are appropriate for work that must survive between sessions.
3. Register virtual devices only when their owner has authorized disposal. Regular linked worktrees are automatically eligible after seven days since their latest file write, HEAD commit or detected use.
4. Run `aigc plan --json` to review current decisions. `clean` only acts on eligible resources and rechecks them.
5. Report actual outcomes from `history`; never add overlapping category sizes into a claimed reclaimed total.

Use `config set` to change policy on the user's behalf. Do not edit `state.json` to manufacture old observations. Do not disable protection to force a free-space target.

Codex-managed linked worktrees follow the normal seven-day policy. Cleanup does not archive their chats or create Codex recovery snapshots. A locked, active, unknown or pinned resource is not disposable merely because a task completed. Eligible regular worktrees with modified, untracked or ignored files are force-removed with no recovery of those files. Pin or reserve work that must remain available.

Examples:

```sh
aigc status worktrees --json
aigc config set min-free-space 25GB
aigc pin /absolute/path/to/current-task
aigc run -- cargo build
aigc unpin /absolute/path/to/current-task
```

Version 0.4 reports schema 2. Read [adapter scope](ADAPTERS.md): some package caches, SDK packages and runtime disks support explicit disposal, while shared/uncertain resources remain protected. Never treat report-only storage as automatically cleanable.

Use `inspect ID` for native usage, ownership, consumers and process evidence. `own ID --owner TASK` records ownership only; `require ID --project PROJECT` protects future requirements. `duplicates` is advisory and never merges environments.

For generated scratch/build output use `register PATH --kind scratch --owner TASK --purpose TEXT --retain-days 30` only when the user authorized disposal. Do not register a checkout as scratch. Use `run --resource PATH --owner TASK -- COMMAND` for scoped reservations; omit resources to protect everything. Inherited background group members keep the wrapper alive. Escaped daemons need pins. Crashed wrappers leave reservations: inspect `leases` and release only after verifying the work has ended.

History distinguishes `removed`, `maintained`, `no_op` and `skipped`; native reclaimed bytes may be unknown. A successful maintenance command does not imply any bytes were freed. Never sum nested/category sizes.
