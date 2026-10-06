# AI Garbage Collector

**Clean up the storage AI coding agents leave behind.**

`aigc` is a local CLI and hourly garbage collector for abandoned worktrees, dependencies, build output and virtual devices. Keep using the storage you have—no cloud, subscription or AI model required.

> **Default cleanup:** all implemented cleanup categories are enabled. Worktrees idle for 7 days can be deleted immediately after installation, including uncommitted and ignored files, **without recovery**. Detected activity, open PRs and aigc pins protect them. Failed PR lookups do **not** block deletion by default.

## Install

Supports **macOS 13+** (Apple Silicon/Intel), **Windows 10/11** (x86_64) and **Ubuntu 22.04+** (x86_64). Git is required.

Installers start collection immediately, then hourly and at login. No `aigc start` command is needed. Downloads use SHA-256 verification; binaries are currently unsigned. Re-run the installer to update.

### macOS / Ubuntu

Ubuntu prerequisites: `sudo apt install git curl lsof`. A systemd user session is required.

```sh
curl -fsSL https://raw.githubusercontent.com/MyhreS/AI-Garbage-Collector/main/scripts/install.sh | sh
```

### Homebrew (macOS)

```sh
curl -fsSL https://raw.githubusercontent.com/MyhreS/AI-Garbage-Collector/main/Brewfile | brew bundle --file=-
```

### Windows terminal install (PowerShell)

Install [Git for Windows](https://git-scm.com/download/win), then:

```powershell
Invoke-WebRequest https://raw.githubusercontent.com/MyhreS/AI-Garbage-Collector/main/scripts/install.ps1 -OutFile "$env:TEMP\aigc-install.ps1"
& "$env:TEMP\aigc-install.ps1"
Remove-Item "$env:TEMP\aigc-install.ps1"
```

[Downloads](https://github.com/MyhreS/AI-Garbage-Collector/releases/latest) · [Install, update and uninstall details](docs/USAGE.md#install)

## Use

```sh
aigc status                     # Counts, sizes, activity and eligibility
aigc status worktrees --refresh # Fresh worktree inventory
aigc status agent-caches --refresh # Codex/Claude cache entries and protection reasons
aigc status --json              # Structured inventory and scan timings
aigc clean                      # Scan now and process all eligible resources
aigc clean --dry-run            # Preview cleanup
aigc pin /absolute/path         # Keep a resource
aigc pause 2h                   # Temporarily pause cleanup
aigc config show                # View settings
aigc service status            # Check the background scheduler
```

Cleanup passes have no item-count or byte cap. Skipped resources do not stop later candidates.

## Cleanup scope

| Resource | Behavior |
| --- | --- |
| Git worktrees, including Codex and Claude | Remove after 7 days since the latest file write, HEAD commit or detected use. Clean and dirty trees are removed without recovery archives; dirty trees use force removal. |
| Recognized dependencies and build output | Remove eligible `node_modules`, Rust/Swift/Next.js output and Xcode DerivedData after observed inactivity; protect tracked source. |
| Python/Poetry environments | Automatically remove after 7 days since the latest file write or detected use, including the first scan. Orphaned Poetry environments are included. |
| Devices, runtimes, SDKs | Automatically collect eligible resources after activity and dependency checks. Xcode/iOS support is macOS-only. |
| pip, pnpm and npm caches | Automatic native maintenance after inactivity and cooldown checks, regardless of cache size. |
| Codex and Claude caches | Remove individual regenerable entries after seven days of observation and no file change, access or detected use. Known CLI cache folders are discovered; custom cache roots require explicit configuration. |
| Other manager caches, browsers, Xcode archives and recovery bundles | Report only or protected. |

Custom scratch paths require registration for discovery, with no extra retention deadline. Other filesystem resources require **7 days of observed inactivity**. Storage figures are estimates. Docker is unsupported. The app does not deduplicate environments, archive agent chats, sweep personal files or manage remote machines.

Inventory-format changes do not restart the worktree or Python environment inactivity timer. Activity detection is best effort. Running agents elsewhere do not block worktree cleanup. **Windows** detects running environment interpreters but lacks general per-file activity attribution; `aigc run` reservations need explicit release. Codex chat pins are not read—use `aigc pin` to preserve a worktree.

## Documentation

- [User guide](docs/USAGE.md): configuration, policy, recovery and uninstall
- [Cleanup details](docs/ADAPTERS.md): supported resources and limitations
- [Agent integration](docs/AGENTS.md): JSON reports, ownership and reservations
- [Builds and releases](docs/RELEASING.md): all workflows are **manual**; no test suite

MIT licensed.
