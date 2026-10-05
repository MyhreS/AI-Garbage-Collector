# User guide

[Back to the README](../README.md)

**Keep the computer you already own usable when coding agents fill its disk.**

AI Garbage Collector exists to clean up after AI coding agents. Running many agents in parallel can leave behind abandoned worktrees, duplicated dependencies, build output and virtual devices. The goal is to keep limited local storage usable by identifying those leftovers and collecting what is safe to remove.

`aigc` is a local command-line app and hourly background collector for development storage. It inventories Git worktrees, dependencies, build output, iOS simulators, and Android emulators. It explains what is in use, what is protected, and what can be collected.

No cloud environment, subscription, account, or AI model. A release installs as one native executable; users do not need Rust, Python, or Node.

**Version 0.6 automatically removes regular linked Git worktrees after seven days without a file write, HEAD commit or detected use.** On the first scan, old worktrees can qualify immediately. Eligible worktrees with local changes are force-removed, including tracked edits, untracked files and ignored files, with no recovery copy of those files. Read the coverage table before installing.

## Install

Supports macOS 13+ (Apple Silicon and Intel), Windows 10/11 x86_64, and Ubuntu 22.04+ x86_64. Git must already be installed. Mobile and package-manager adapters use your existing tools.

### Homebrew

Install and start the hourly collector with one command:

```sh
curl -fsSL https://raw.githubusercontent.com/MyhreS/AI-Garbage-Collector/main/Brewfile | brew bundle --file=-
```

The [Brewfile](../Brewfile) adds the project's [custom Homebrew tap](https://docs.brew.sh/Taps), trusts this formula, installs the release binary, and starts its per-user LaunchAgent immediately. It runs again every hour while you are logged in and starts at login. No `aigc start` command, Rust installation, or administrator access is needed. Homebrew verifies the release checksum.

If you run `brew install myhres/aigc/aigc` directly, Homebrew only installs the executable; it cannot automatically start a formula's service. Use the command above for install-and-start, or run `brew services start myhres/aigc/aigc` after a direct install.

If switching from the standalone installer, run `aigc service uninstall` before starting the Homebrew service. Both use the same service identity so there should be only one collector. Manage a Homebrew installation with `brew services`, rather than `aigc service install`.

```sh
brew update
brew upgrade myhres/aigc/aigc
brew services restart myhres/aigc/aigc
```

The formula uses Homebrew's stable `opt` path, so it does not point at an old version's executable. After publishing a release, manually run the Homebrew update workflow to refresh the formula.

To uninstall, preserving settings and recovery bundles:

```sh
brew services stop myhres/aigc/aigc
brew uninstall myhres/aigc/aigc
brew untap myhres/aigc
```

### Terminal installer

The standalone installer downloads the latest GitHub release and starts collection:

```sh
curl -fsSL https://raw.githubusercontent.com/MyhreS/AI-Garbage-Collector/main/scripts/install.sh -o /tmp/aigc-install.sh
sh /tmp/aigc-install.sh
rm /tmp/aigc-install.sh
```

The installer verifies the release archive's SHA-256 checksum, installs `~/.local/bin/aigc`, and starts the platform scheduler (LaunchAgent on macOS, systemd user timer on Ubuntu). It prints the full executable path if `~/.local/bin` is not on your `PATH`. It does not request administrator access or edit your shell startup files.

Release binaries are **not Developer ID signed or notarized** yet. Checksums check the downloaded archive against the release manifest; they do not replace publisher signing. There is no automatic binary updater. Re-run the installer to update. Set `AIGC_VERSION=v0.6.0` to select a particular release.

### Ubuntu terminal install

Install prerequisites once: `sudo apt install git curl lsof`. Then use the same shell installer above from your normal user session. The release targets Ubuntu 22.04+ x86_64 (glibc 2.35+). It installs and starts a systemd **user** timer immediately, then runs hourly and after login. It does not enable system-wide services or lingering; collection while logged out requires you to configure user lingering separately. A user systemd session is required; minimal containers and WSL without systemd are unsupported.

### Windows terminal install (PowerShell)

Install [Git for Windows](https://git-scm.com/download/win) first, then run in your normal logged-in Windows account:

```powershell
Invoke-WebRequest https://raw.githubusercontent.com/MyhreS/AI-Garbage-Collector/main/scripts/install.ps1 -OutFile "$env:TEMP\aigc-install.ps1"
& "$env:TEMP\aigc-install.ps1"
Remove-Item "$env:TEMP\aigc-install.ps1"
```

The installer verifies SHA-256, installs `%LOCALAPPDATA%\aigc\bin\aigc.exe`, adds it to your user PATH, and creates an hourly Task Scheduler task with a login trigger. It starts immediately. Open a new terminal for the updated PATH. If your execution policy blocks scripts, review the downloaded file and follow your organization's policy; the installer does not change that policy. Windows binaries are not Authenticode signed. Re-run the installer to update. `aigc service uninstall` removes the scheduled task and keeps your settings.

### Platform activity and storage limits

- **macOS / Ubuntu:** `lsof` and `ps` identify open paths and processes. Missing or failed activity queries prevent deletion. Ubuntu permissions and `/proc` restrictions can reduce visibility; this tool does not inspect other users' private processes or claim universal agent attribution.
- **Windows:** running agents elsewhere do not block worktree cleanup. Worktrees use the seven-day age policy and open-PR checks. Recognized developer processes still defer non-worktree categories. There is no built-in per-file handle scan or exact agent-to-worktree attribution. Unrecognized programs may be missed; use pins or `aigc run` reservations for work you need to preserve. Windows reservations remain after command exit to protect possible background children; release them explicitly with `aigc release-lease ID` after work ends. Windows `run --resource` accepts absolute paths.
- Windows skips resources containing reparse points (including junctions). Windows size fields report **logical bytes**, despite the shared JSON field names mentioning allocated bytes. Sparse files/compression may make actual recovered space different. macOS/Ubuntu use allocated blocks. All platforms deduplicate hardlinks by filesystem identity.
- State lives in `~/Library/Application Support/aigc` on macOS, `${XDG_STATE_HOME:-~/.local/state}/aigc` on Ubuntu, and `%LOCALAPPDATA%\aigc` on Windows. The program stays local; there is no remote collector.

### Build and install from source

Requires Rust and your platform's native build tools. On macOS, install Apple's command-line developer tools. The following shell commands install on macOS or Ubuntu:

```sh
git clone https://github.com/MyhreS/AI-Garbage-Collector.git
cd AI-Garbage-Collector
cargo build --release --locked
mkdir -p "$HOME/.local/bin"
install -m 755 target/release/aigc "$HOME/.local/bin/aigc"
"$HOME/.local/bin/aigc" service install
```

To inspect before enabling the service, run `target/release/aigc status` instead. Building or running `status` does not enable background deletion.

## See where the space went

```sh
aigc status
aigc status worktrees
aigc status simulators
aigc status emulators
aigc status --json
aigc status --refresh
```

Status reuses a clearly labelled snapshot for up to one hour so repeated queries are fast. Use `--refresh` for a fresh inventory. Cleanup always performs a fresh scan; configuration changes invalidate the snapshot. The first scan can take a few minutes on machines with many worktrees. Deep adapters inspect at most 250 discovered projects. Missing coverage disables reference-dependent cleanup; native tools must already be installed.

The overview shows disk capacity, available space, service installation, the last collection time, and a table with **Total / In use / Protected / Observing / Eligible / Unknown** counts and category sizes. A category argument shows resource IDs and individual reasons. JSON also includes paths, allocated bytes, observation duration, scan completeness, warnings, and schema version.

- **In use:** a running device or open file/working directory was detected.
- **Protected:** a pin, ownership rule, saved work, read-only category, pause, or a detected build/agent prevents collection.
- **Observing:** a potentially disposable resource has not completed its inactivity period.
- **Eligible:** it passed the current policy; the collector checks again before removal.
- **Unknown:** a size or activity check was incomplete. Unknown is never treated as unused.

Counts are resource counts, not agent/session counts. SDK entries represent installed packages when discovery succeeds. Unavailable tools produce warnings, not fake zero counts.

**Sizes are estimates, not a guaranteed reclaimable total.** Worktrees include their dependencies; APFS clones and snapshots can retain blocks. Category sizes must not be added together. The filesystem union counts nested directories and hardlinks once, with a completeness flag; APFS sharing remains an estimate. History records both estimated bytes removed and the actual before/after change in free disk space; concurrent applications can affect that change.

## What it does and does not clean

Docker inventory and cleanup are not supported. Images, containers, volumes and build cache are outside this app’s scope.

| Resource | Reports | Automatic cleanup |
| --- | --- | --- |
| Xcode DerivedData children | Yes | After observed inactivity and activity checks |
| `node_modules` | Yes | Recognized project folders only; refuses Git-tracked files |
| Python / Poetry project environments | Project associations, interpreter metadata, matching-input candidates | Explicitly disposable, unshared environments only; linked environments and installed tools protected |
| Rust `target`, Swift `.build`, Next.js `.next` | Yes | Recognized project folders only; refuses Git-tracked files |
| Regular Git linked worktrees | Yes | Automatic after seven days since the latest file write, HEAD commit or detected use, including on the first scan. Dirty trees are force-removed with no recovery of local files. Clean trees require a verified HEAD recovery bundle. Open GitHub PRs are protected. |
| Codex worktrees under `.codex` / `.codex-workspaces` | Yes | Same seven-day policy as other linked worktrees; native Git removal, without a Codex snapshot or chat archival |
| iOS simulator devices | Yes | Only registered disposable, shut-down devices; deletes app data through `simctl` |
| Android AVDs | Yes | Only registered disposable AVDs; requires `avdmanager`; defers while any emulator is running |
| iOS simulator runtimes | Native disk registration, build and retained devices | Explicitly disposable runtimes only, no retained devices; supported native schema required |
| Android SDK packages | Installed package IDs, AVD references and simple Gradle declarations | Explicitly disposable packages only; unresolved Gradle requirements protect packages |
| pip, pnpm, npm caches | Manager-configured paths | Opt-in native purge/prune/verify after observed inactivity, size budget and cooldown |
| uv, Poetry, Cargo, Gradle, Yarn, Bun storage | Configured or documented locations; scope varies by adapter | **Report only**; native ownership/retention can span resources |
| Playwright browsers, npx installations | Revisions/installations and available package references | **Report only**; keep native ownership controls |
| Homebrew cache | Configured cache path and native cleanup preview | **Report only**; native cleanup also affects installed versions |
| Registered scratch/custom build output | Explicit owner, purpose and retention deadline | Opt-in disposable directories after path, source and activity checks |
| Xcode release archives | Yes | **Always protected** |
| Recovery bundles created by aigc | Yes | **Always protected; user-managed retention** |
| Databases, credentials, signing keys, personal files | Not a general-purpose inventory | **Never targeted** |

This version does **not** deduplicate dependencies, share environments between worktrees, delete Git branches, uninstall Xcode, remove arbitrary `build`/`dist` folders, or sweep global IDE caches. It does not manage remote computers, or cloud workspaces. Xcode and iOS simulator adapters are macOS-only.

These generated directories are treated as disposable. Pin them if you keep manual changes or irreplaceable files inside them. Removing dependencies or build output means a later install/build may take longer and require internet access. Registered disposable simulator and emulator data is permanently deleted. There is no universal undo for caches or devices.

## Detailed inspection and ownership

```sh
aigc status python --owners
aigc inspect 'EXACT-RESOURCE-ID'
aigc duplicates --json
aigc preview 'EXACT-RESOURCE-ID'
aigc own 'EXACT-RESOURCE-ID' --owner task-123
aigc require 'EXACT-RESOURCE-ID' --project my-project
aigc unrequire 'EXACT-RESOURCE-ID' --project my-project
```

`own` records ownership without authorizing deletion. `require` protects a resource needed by a project, including future builds. `manage` authorizes disposal for opt-in categories after all other checks. Regular linked worktrees need no `manage` command. Inspection includes process IDs/start times, known consumers, native metadata, observation coverage, reconstruction notes and protection reasons. It does not identify every agent session automatically.

`duplicates` groups matching recorded dependency/build inputs. It never merges environments or assumes matching lockfiles make two mutable installations interchangeable. Native previews can include resources that aigc would protect; a preview does not grant deletion permission.

See [adapter behavior and limitations](ADAPTERS.md) for exact scope, and the [original research](CLEANUP-RESEARCH.md) for rationale and longer-term ideas.

## Default policy

The terminal installers and Homebrew Brewfile start the service immediately. Each hourly run inventories resources and removes those that qualify:

| Setting | Default |
| --- | --- |
| Normal inactivity | 7 days |
| Inactivity when available space is below the target | 7 days |
| Free-space target | 20 GiB |
| Clean-worktree recovery bundle budget | 2 GiB; further clean-worktree removal stops when it would be exceeded |
| Estimated removal limit per pass | 50 GiB; at most 10 eligible actions, with complete revalidation per action |
| Opt-in package-cache budget | 5 GiB per reported cache |
| Maintenance/recollection cooldown | 7 days |
| Schedule | Every hour while your user session is logged in; also when loaded |

**A first install can remove old regular linked worktrees immediately.** Their clock uses the newest file or directory modification time, HEAD commit time and any detected use. It cannot tell whether somebody read or intends to reuse a worktree. All other filesystem resources require seven days of observed inactivity; a change in size, modification time, entry count or detected use resets that clock, as does a monitoring gap longer than 48 hours.

The free-space target is a policy trigger, not a guarantee or hard quota. The collector does not remove protected resources to meet it. Native commands may reclaim less than their reported scope.

Discovery includes these existing locations:

```text
~/workdir
~/Developer
~/Projects
~/.codex/worktrees
~/.codex-workspaces/worktrees
~/.claude/worktrees
```

Project discovery includes hidden folders, is limited to eight directory levels and 50,000 entries per root, and skips dependency/build internals. Size measurement stops at 500,000 entries per resource and never follows symlinks or crosses filesystems. Add deeper project directories as roots if needed. Resources outside your home are not deleted.

## Configure it

```sh
aigc config show
aigc config path
aigc config set min-free-space 25GB
aigc config set budget.backups 2GB
aigc config set retention-days 14
aigc config set pressure-retention-days 3
aigc config set max-delete-per-run 20GB
aigc config set worktree-cleanup false
aigc config set worktree-force false
aigc config set worktree-require-pr-verification true
aigc config set budget.package-cache 5GB
aigc config set maintenance-cooldown-days 7
aigc config set deep-inventory true
aigc config set roots '["/Users/me/Projects", "/Users/me/other-repository"]'
```

`roots` replaces the project discovery list. Space units use powers of 1024: `GB` and `GiB` both mean GiB. Pressure retention must not exceed normal retention. Malformed settings stop collection instead of silently falling back.

```sh
aigc pin /absolute/path/to/important-worktree
aigc pin 'simulators:UUID-FROM-STATUS'
aigc unpin /absolute/path/to/important-worktree
aigc pause 2h
aigc resume
aigc plan
aigc clean --dry-run
aigc clean
aigc history
aigc service status
```

Pins protect a directory, its descendants, and containing resources that would otherwise remove it. A successful `clean` with no eligible items does nothing; use `plan` for the reasons.

### Opt in disposable virtual devices

Copy an exact resource ID from `aigc status <category> --json`:

```sh
aigc manage 'simulators:UUID-FROM-STATUS' --owner mobile-builds
aigc manage 'emulators:throwaway-pixel' --owner mobile-builds
aigc unmanage 'emulators:throwaway-pixel'
```

Registration authorizes disposal after policy checks for devices. Regular linked worktrees need no registration. They still honor aigc pins, activity checks and open-PR checks. For devices, register only data you are willing to lose. Inventory still works without registration.

For an eligible regular Git worktree, `aigc` uses [GitHub CLI](https://cli.github.com/manual/) to check open PRs in the checkout repository and its fork parent. Named branches use their branch name; detached checkouts use PRs associated with the HEAD commit. A detected open PR protects the worktree. **By default, a missing GitHub CLI, failed authentication or failed lookup does not block an otherwise eligible seven-day-old worktree.** The report labels unavailable verification. Set `worktree-require-pr-verification true` to retain worktrees whenever this check fails. Checks run during inventory and before removal. Commit associations and repository discovery cannot identify every related PR, especially PRs targeting unrelated repositories; pin those worktrees.

Codex-managed linked worktrees use the same cleanup policy as other linked worktrees. aigc removes eligible trees through Git without creating a Codex snapshot or archiving their chats. Codex may retain chats pointing to a removed folder. **Codex chat pins, chat recency and permanent-worktree settings are not read by aigc.** Use `aigc pin PATH` or `aigc run` reservations to protect work between sessions; process activity and filesystem age remain checked. The collector does not delete Codex conversation logs, credentials or its state database.

## Use it from an agent

Use `--json` for reports (schema version 2) and exact IDs. Exit code 0 means the command completed, not that anything was deleted; inspect the outcomes and warnings. Invalid commands/configuration, lock contention, and command failures exit nonzero. Errors are currently text on stderr.

```sh
aigc status --json
aigc plan --json
aigc run -- npm run build
aigc run -- xcodebuild -scheme MyApp build
```

`aigc run` reserves all resources by default. Pass repeatable `--resource` IDs or absolute paths and `--owner` for scoped reservations. On macOS and Ubuntu, it creates a separate process group and retains the reservation until the foreground command and inherited background group members finish. Explicitly daemonized processes that escape that group are not tracked: pin their resources. A crashed wrapper leaves a protective reservation; inspect `aigc leases`, then explicitly `aigc release-lease ID` only after its work has ended. A failed child makes the wrapper fail. On Windows, the reservation stays until explicitly released, including after successful command exit.

```sh
aigc run --resource /absolute/path/to/project --owner task-123 -- cargo build
aigc register /absolute/path/to/project/package-staging --kind scratch \
  --owner task-123 --purpose 'Disposable packaging output' --retain-days 30
aigc unregister /absolute/path/to/project/package-staging
```

Registration is explicit permission to dispose of generated contents after its minimum deadline and the normal observation period. It cannot authorize tracked source, nested Git repositories, protected application state or personal folders.

On macOS and Ubuntu, collection uses `lsof`, process inspection, device state, filesystem observations and native Git checks. For regular worktrees, it checks for open files and working directories in that tree, scoped reservations and global `aigc run` reservations; a recognized process elsewhere does not block cleanup of an unrelated worktree. Other categories defer while recognized builds or agents run. **These are best-effort signals, not proof that an arbitrary paused agent is finished.** External tools do not take aigc's lock, so a process can start between inspection and deletion. Use reservations and pins for important work. aigc's own commands serialize state updates and collection with a lock.

No MCP server is required. See [agent usage](AGENTS.md) for a short integration guide.

## Recovery, storage and uninstall

Use `aigc config path` to find state on your platform (locations are listed above):

- `config.json`: settings, pins and explicitly disposable resources.
- `state.json`: bounded observation records and last collection time.
- `last-report.json`: the most recent inventory snapshot.
- `history.json`: the most recent 500 cleanup outcomes.
- `leases/`: foreground command reservations.
- `backups/`: verified Git bundles for clean removed worktrees; not automatically expired.

For a **clean** regular worktree, aigc creates and verifies a bundle of its HEAD history, including unpushed commits. The branch stays in the original repository. Restore using:

```sh
git clone '/path/from/history/to/backup.bundle' restored-worktree
```

For a **dirty** regular worktree, aigc uses `git worktree remove --force`. It does **not** make a bundle or another recovery copy. Tracked edits, untracked files, ignored files, local configuration and generated content inside it are permanently discarded. A named Git branch and its committed history remain in the original repository. A detached checkout has no branch keeping its commits reachable; after force removal, Git may eventually discard those commits too. Pin a tree, use a reservation, or set `worktree-force false` to keep local files.

Clean-tree bundles consume disk space and should be reviewed when no longer needed. Creation checks a 2 GiB default backup budget and available space; failed new bundles are removed. They do not back up other linked worktrees or external files. If clean-tree backup creation or verification fails, removal does not proceed.

Stop and uninstall without deleting configuration or recovery data:

```sh
aigc service uninstall
rm "$HOME/.local/bin/aigc"
```

The LaunchAgent is `~/Library/LaunchAgents/io.aigc.collector.plist`. `service uninstall` removes it. Inspect retained state/backups before removing the state directory yourself. `AIGC_STATE_DIR` is available for isolated manual runs; service installation refuses this override.

## Development and validation

```sh
cargo fmt --check
cargo clippy --locked --lib --bin aigc -- -D warnings
cargo build --release --locked
```

This repository contains no test suite or test-only dependencies. CI and releases check formatting, lint production code, and compile the executable; they do not execute tests or cleanup commands.

All GitHub Actions workflows are manually triggered. Builds produce macOS, Windows and Ubuntu archives. The manual release workflow publishes those archives and `SHA256SUMS`; pushing a tag does not start it. A failed build prevents publishing. See [release instructions](RELEASING.md).

MIT licensed. Contributions that improve activity detection, tool compatibility and recovery are welcome.
