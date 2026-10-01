# AI Garbage Collector

**Keep the Mac you already own usable when coding agents fill its disk.**

`aigc` is a local command-line app and hourly background collector for development storage. It inventories Git worktrees, dependencies, build output, Docker images and cache, iOS simulators, and Android emulators. It explains what is in use, what is protected, and what can be collected.

No cloud environment, subscription, account, or AI model. A release installs as one native executable; users do not need Rust, Python, or Node.

**Version 0.1 is an early, conservative implementation.** It does not yet safely automate every category it can report. Read the coverage table before enabling it.

## Install

macOS only. Apple Silicon and Intel release artifacts are built by the release workflow. The release installer requires a published GitHub release:

```sh
curl -fsSL https://raw.githubusercontent.com/MyhreS/AI-Garbage-Collector/main/scripts/install.sh -o /tmp/aigc-install.sh
sh /tmp/aigc-install.sh
rm /tmp/aigc-install.sh
```

The installer verifies the release archive's SHA-256 checksum, installs `~/.local/bin/aigc`, and starts an hourly per-user LaunchAgent. It prints the full executable path if `~/.local/bin` is not on your `PATH`. It does not request administrator access or edit your shell startup files.

Release binaries are **not Developer ID signed or notarized** yet. Checksums check the downloaded archive against the release manifest; they do not replace publisher signing. There is no automatic binary updater. Re-run the installer to update. Set `AIGC_VERSION=v0.1.0` to select a particular release.

### Build and install from source

Requires the Rust toolchain and Apple's command-line developer tools:

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
aigc status docker
aigc status simulators
aigc status emulators
aigc status --json
aigc status --refresh
```

Status reuses a clearly labelled snapshot for up to one hour so repeated queries are fast. Use `--refresh` for a fresh inventory. Cleanup always performs a fresh scan; configuration changes invalidate the snapshot. The first scan can take a few minutes on machines with many worktrees.

The overview shows disk capacity, available space, service installation, the last collection time, and a table with **Total / In use / Protected / Observing / Eligible / Unknown** counts and category sizes. A category argument shows resource IDs and individual reasons. JSON also includes paths, allocated bytes, observation duration, scan completeness, warnings, and schema version.

- **In use:** a running device, container reference, or open file/working directory was detected. Docker references include stopped containers.
- **Protected:** a pin, ownership rule, saved work, read-only category, pause, or a detected build/agent prevents collection.
- **Observing:** a potentially disposable resource has not completed its inactivity period.
- **Eligible:** it passed the current policy; the collector checks again before removal.
- **Unknown:** a size or activity check was incomplete. Unknown is never treated as unused.

Counts are resource counts, not agent/session counts. Docker cache is one builder-level entry; SDK and package-cache entries represent storage directories. Unavailable tools produce warnings, not fake zero counts.

**Sizes are estimates, not a guaranteed reclaimable total.** Worktrees include their dependencies; Docker images share layers; APFS clones and snapshots can retain blocks. Category sizes must not be added together. Docker's cache size/reclaimability text is preserved separately. History records both estimated bytes removed and the actual before/after change in free disk space; concurrent applications can affect that change.

## What it does and does not clean

| Resource | Reports | Automatic cleanup in v0.1 |
| --- | --- | --- |
| Xcode DerivedData children | Yes | After observed inactivity and activity checks |
| `node_modules`, project `.venv` | Yes | Recognized project folders only; refuses Git-tracked files |
| Rust `target`, Swift `.build`, Next.js `.next` | Yes | Recognized project folders only; refuses Git-tracked files |
| Regular Git linked worktrees | Yes | Only explicitly registered disposable trees; must be clean, including ignored files; verifies a recovery bundle first |
| App-managed worktrees under `.codex` / `.codex-workspaces` | Yes | **Protected.** Use the owning application's archive tool; aigc does not edit its session database |
| Docker build cache | Yes, default builder | Native `docker builder prune`, with age filter and cache storage setting |
| Docker images | Yes | Only images explicitly registered disposable; native removal without force |
| iOS simulator devices | Yes | Only registered disposable, shut-down devices; deletes app data through `simctl` |
| Android AVDs | Yes | Only registered disposable AVDs; requires `avdmanager`; defers while any emulator is running |
| iOS simulator runtimes | Yes, when exposed by `simctl` | **Report only** |
| Android SDK platforms, NDKs, system images | Yes, standard Mac SDK location | **Report only** |
| Homebrew, pip, uv and npm global caches | Selected standard paths | **Report only** |
| Xcode release archives | Yes | **Always protected** |
| Recovery bundles created by aigc | Yes | **Always protected; user-managed retention** |
| Docker volumes, containers, databases, credentials, signing keys, personal files | Not a general-purpose inventory | **Never targeted** |

This version does **not** deduplicate dependencies, share environments between worktrees, delete Git branches, compact Docker's VM disk, uninstall Xcode, remove arbitrary `build`/`dist` folders, or sweep global IDE caches. It does not manage remote Docker endpoints, remote computers, or cloud workspaces. Windows and Linux cleanup are not supported.

These generated directories are treated as disposable. Pin them if you keep manual changes or irreplaceable files inside them. Removing dependencies or build output means a later install/build may take longer and require internet access. Registered disposable simulator and emulator data is permanently deleted. There is no universal undo for caches or devices.

## Default policy

Installation starts the service immediately. Each hourly run inventories resources and removes those that qualify:

| Setting | Default |
| --- | --- |
| Normal observed inactivity | 30 days |
| Inactivity when available space is below the target | 7 days |
| Free-space target | 20 GiB |
| Docker cache storage setting | 5 GiB |
| Recovery bundle budget | 2 GiB; further worktree removal stops when it would be exceeded |
| Estimated removal limit per pass | 50 GiB, excluding native Docker cache pruning |
| Schedule | Every hour while your user session is logged in; also when loaded |

**A first install starts filesystem observation, not a disk wipe.** Docker build cache is the exception: Docker provides native age/usage filtering, so eligible old cache can be pruned on the first pass. For filesystem resources, age alone does not authorize deletion. A change in size, latest modification time, entry count, or detected use resets the observation clock. A monitoring gap longer than 48 hours also resets it. Leaving the Mac off for a month does not make everything eligible on startup.

The free-space target is a policy trigger, not a guarantee or hard quota. The collector does not remove protected resources to meet it. Native Docker pruning uses Docker's eligibility rules; `--keep-storage` is a cache retention setting, not a guaranteed final disk size. Docker cache pruning is not covered by the filesystem byte limit.

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
aigc config set budget.docker-cache 5GB
aigc config set budget.backups 2GB
aigc config set retention-days 14
aigc config set pressure-retention-days 3
aigc config set max-delete-per-run 20GB
aigc config set docker-cache-cleanup false
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

### Opt in disposable worktrees, images and virtual devices

Copy an exact resource ID from `aigc status <category> --json`:

```sh
aigc manage 'worktrees:/Users/me/Projects/task-123' --owner task-123
aigc manage 'simulators:UUID-FROM-STATUS' --owner mobile-tests
aigc manage 'emulators:throwaway-pixel' --owner mobile-tests
aigc manage 'docker-images:sha256:FULL-IMAGE-ID' --owner task-123
aigc unmanage 'emulators:throwaway-pixel'
```

Registration authorizes disposal after policy checks. It does not bypass pins, activity checks, worktree changes, or app-managed worktree protection. For devices/images, register only data you are willing to lose. Inventory still works without registration.

## Use it from an agent

Use `--json` for reports and exact IDs. Exit code 0 means the command completed, not that anything was deleted; inspect the outcomes and warnings. Invalid commands/configuration, lock contention, and command failures exit nonzero. Errors are currently text on stderr.

```sh
aigc status --json
aigc plan --json
aigc run -- npm test
aigc run -- xcodebuild -scheme MyApp build
```

`aigc run` reserves all resources while its foreground child runs. Reservation records include the process ID and start time to avoid trusting a reused PID. It forwards the child's exit code. Use it for builds and agent sessions when possible. Detached subprocesses are not a supported reservation lifecycle.

Collection uses `lsof`, process inspection, device state, filesystem observations and native Git/Docker checks. It defers while recognized builds or agents run. **These are best-effort signals, not proof that an arbitrary paused agent is finished.** External tools do not take aigc's lock, so a process can start between inspection and deletion. Use reservations and pins for important work. aigc's own commands serialize state updates and collection with a lock.

No MCP server is required. See [agent usage](docs/AGENTS.md) for a short integration guide.

## Recovery, storage and uninstall

State lives in `~/Library/Application Support/aigc`:

- `config.json`: settings, pins and explicitly disposable resources.
- `state.json`: bounded observation records and last collection time.
- `last-report.json`: the most recent inventory snapshot.
- `history.json`: the most recent 500 cleanup outcomes.
- `leases/`: foreground command reservations.
- `backups/`: verified Git bundles for removed worktrees; not automatically expired.

Before removing a regular worktree, aigc refuses modified, untracked and ignored files, then creates and verifies a bundle of its HEAD history, including unpushed commits. The branch stays in the original repository. Restore using:

```sh
git clone '/path/from/history/to/backup.bundle' restored-worktree
```

Bundles consume disk space and should be reviewed when no longer needed. Creation checks a 2 GiB default backup budget and available space; failed new bundles are removed. They do not back up other linked worktrees, arbitrary ignored files, local config, or external files. If backup creation or verification fails, worktree removal does not proceed.

Stop and uninstall without deleting configuration or recovery data:

```sh
aigc service uninstall
rm "$HOME/.local/bin/aigc"
```

The LaunchAgent is `~/Library/LaunchAgents/io.aigc.collector.plist`. `service uninstall` removes it. Inspect retained state/backups before removing the state directory yourself. `AIGC_STATE_DIR` is available for isolated manual tests; service installation refuses this override.

## Development and validation

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --release --locked
```

Tests cover observed retention, disk pressure, missing activity evidence, pins, unowned devices, malformed configuration, symlink handling, changes between scan and removal, tracked files, and recovery of an unpushed commit after worktree removal. Native device and Docker deletion still need broader testing across tool versions; this project does not claim production-proven cleanup for every setup.

The CI workflow validates changes on macOS. Pushing a version tag builds Apple Silicon and Intel archives and publishes them with checksums. See [release instructions](docs/RELEASING.md).

MIT licensed. Contributions that improve activity detection, tool compatibility, recovery and test coverage are welcome.
