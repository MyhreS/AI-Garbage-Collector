---
name: aigc
description: Inspect and safely clean up the storage coding agents leave behind (git worktrees, dependency folders, Python environments, build output, package caches) with the local AI Garbage Collector CLI `aigc`. Use when disk is filling up, when asked what agent worktrees or caches exist, before or after long builds, or when a task's scratch output should be protected or released.
license: MIT
---

# AI Garbage Collector (`aigc`)

`aigc` is a local CLI and hourly background collector for macOS, Windows and Ubuntu. It inventories agent worktrees, dependencies, build output and caches, and removes only resources that are idle and verified safe. No cloud, account or model is involved.

## Before you start

1. Check that it is installed: `aigc --version`. If it is missing, tell the user and link the install instructions at https://github.com/MyhreS/AI-Garbage-Collector#install. Do not run an installer without the user's go-ahead.
2. Check the background service: `aigc service status`.

## Inspect

```sh
aigc status --json            # inventory, sizes, activity, eligibility; read warnings, complete, status, reason
aigc status worktrees --refresh
aigc plan --json              # what the next pass would do, and why
aigc inspect ID               # one resource: ownership, consumers, process evidence, blockers
aigc history                  # last 500 outcomes: removed, maintained, no_op, skipped
```

Report reclaimed space from `history`, never by adding category sizes together: categories overlap and nested sizes double count.

## Clean

```sh
aigc clean --dry-run          # always preview first and show the user the result
aigc clean                    # only acts on eligible resources and rechecks each one
```

Run `aigc clean` only when the user asked for cleanup. Eligible linked worktrees with uncommitted, untracked or ignored files are force-removed **without recovery** by default; call that out in the preview summary.

## Protect work

```sh
aigc run -- cargo build                 # protect everything while a command runs
aigc run --resource PATH --owner TASK -- COMMAND
aigc pin /absolute/path/to/current-task # keep work between sessions
aigc unpin /absolute/path/to/current-task
aigc pause 2h                           # pause collection
aigc resume
```

Use `register PATH --kind scratch --owner TASK --purpose TEXT` only for generated scratch output the user agreed can be disposed of. Never register a checkout.

## Configure

Change policy with `aigc config set` (for example `aigc config set min-free-space 25GB`) and show it with `aigc config show`. Never edit `state.json` to fake old observations, and never weaken protection to reach a free-space target.

## Rules

- Treat uncertain ownership or activity as protected, never as permission to delete.
- Do not copy aigc's deletion logic into shell commands; use the CLI.
- A locked, active, unknown or pinned resource is not disposable just because a task finished.
- Simulators, runtimes, emulators and SDK packages are never touched; report-only categories are not automatically cleanable.
- Crashed wrappers leave reservations: check `aigc leases` and run `aigc release-lease` only after verifying the work has ended.
