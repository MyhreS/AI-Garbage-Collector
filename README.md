# AI Garbage Collector

**Keep the Mac you already own usable when coding agents fill its disk.**

AI Garbage Collector exists to clean up after AI coding agents. Running many agents in parallel can leave behind abandoned worktrees, duplicated dependencies, build output, Docker cache and virtual devices. The goal is to keep limited local storage usable by identifying those leftovers and collecting what is safe to remove.

`aigc` is a local command-line app and hourly background collector for development storage. It inventories Git worktrees, dependencies, build output, Docker images and cache, iOS simulators, and Android emulators. It explains what is in use, what is protected, and what can be collected.

No cloud environment, subscription, account, or AI model. A release installs as one native executable; users do not need Rust, Python, or Node.

**Version 0.4 automatically removes regular linked Git worktrees after seven days without a file write, HEAD commit or detected use.** On the first scan, old worktrees can qualify immediately. Eligible worktrees with local changes are force-removed, including tracked edits, untracked files and ignored files, with no recovery copy of those files. Read the coverage table before installing.

## Install

Requires macOS 13 or newer. Apple Silicon and Intel binaries are available.

### Homebrew

Install and start the hourly collector with one command:

```sh
curl -fsSL https://raw.githubusercontent.com/MyhreS/AI-Garbage-Collector/main/Brewfile | brew bundle --file=-
```

The [Brewfile](Brewfile) adds the project's [custom Homebrew tap](https://docs.brew.sh/Taps), trusts this formula, installs the release binary, and starts its per-user LaunchAgent immediately. It runs again every hour while you are logged in and starts at login. No `aigc start` command, Rust installation, or administrator access is needed. Homebrew verifies the release checksum.

If you run `brew install myhres/aigc/aigc` directly, Homebrew only installs the executable; it cannot automatically start a formula's service. Use the command above for install-and-start, or run `brew services start myhres/aigc/aigc` after a direct install.

If switching from the standalone installer, run `aigc service uninstall` before starting the Homebrew service. Both use the same service identity so there should be only one collector. Manage a Homebrew installation with `brew services`, rather than `aigc service install`.

```sh
brew update
brew upgrade myhres/aigc/aigc
brew services restart myhres/aigc/aigc
```

The formula uses Homebrew's stable `opt` path, so it does not point at an old version's executable. Each successful GitHub release updates the formula automatically.

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

The installer verifies the release archive's SHA-256 checksum, installs `~/.local/bin/aigc`, and starts an hourly per-user LaunchAgent. It prints the full executable path if `~/.local/bin` is not on your `PATH`. It does not request administrator access or edit your shell startup files.

Release binaries are **not Developer ID signed or notarized** yet. Checksums check the downloaded archive against the release manifest; they do not replace publisher signing. There is no automatic binary updater. Re-run the installer to update. Set `AIGC_VERSION=v0.4.1` to select a particular release.

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

Status reuses a clearly labelled snapshot for up to one hour so repeated queries are fast. Use `--refresh` for a fresh inventory. Cleanup always performs a fresh scan; configuration changes invalidate the snapshot. The first scan can take a few minutes on machines with many worktrees. Deep adapters inspect at most 250 discovered projects. Missing coverage disables reference-dependent cleanup; native tools must already be installed.

The overview shows disk capacity, available space, service installation, the last collection time, and a table with **Total / In use / Protected / Observing / Eligible / Unknown** counts and category sizes. A category argument shows resource IDs and individual reasons. JSON also includes paths, allocated bytes, observation duration, scan completeness, warnings, and schema version.

- **In use:** a running device, container reference, or open file/working directory was detected. Docker references include stopped containers.
- **Protected:** a pin, ownership rule, saved work, read-only category, pause, or a detected build/agent prevents collection.
- **Observing:** a potentially disposable resource has not completed its inactivity period.
- **Eligible:** it passed the current policy; the collector checks again before removal.
- **Unknown:** a size or activity check was incomplete. Unknown is never treated as unused.

Counts are resource counts, not agent/session counts. Detailed Docker cache entries represent native records; SDK entries represent installed packages when discovery succeeds. Unavailable tools produce warnings, not fake zero counts.

**Sizes are estimates, not a guaranteed reclaimable total.** Worktrees include their dependencies; Docker images share layers; APFS clones and snapshots can retain blocks. Category sizes must not be added together. The filesystem union counts nested directories and hardlinks once, with a completeness flag; APFS sharing remains an estimate. Docker's human-formatted cache sizes are retained alongside conservative byte estimates. History records both estimated bytes removed and the actual before/after change in free disk space; concurrent applications can affect that change.

## What it does and does not clean

| Resource | Reports | Automatic cleanup in v0.4 |
| --- | --- | --- |
| Xcode DerivedData children | Yes | After observed inactivity and activity checks |
| `node_modules` | Yes | Recognized project folders only; refuses Git-tracked files |
| Python / Poetry project environments | Project associations, interpreter metadata, matching-input candidates | Explicitly disposable, unshared environments only; linked environments and installed tools protected |
| Rust `target`, Swift `.build`, Next.js `.next` | Yes | Recognized project folders only; refuses Git-tracked files |
| Regular Git linked worktrees | Yes | Automatic after seven days since the latest file write, HEAD commit or detected use, including on the first scan. Dirty trees are force-removed with no recovery of local files. Clean trees require a verified HEAD recovery bundle. Open GitHub PRs are protected. |
| App-managed worktrees under `.codex` / `.codex-workspaces` | Yes | **Protected.** Use the owning application's archive tool; aigc does not edit its session database |
| Docker build cache | Per-record metadata from running local single-node Buildx builders | Exact-ID native Buildx pruning; private immutable regular records only, native age filter and storage setting |
| Docker images | Yes | Only images explicitly registered disposable; native removal without force |
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
| Docker volumes, containers, databases, credentials, signing keys, personal files | Not a general-purpose inventory | **Never targeted** |

This version does **not** deduplicate dependencies, share environments between worktrees, delete Git branches, compact Docker's VM disk, uninstall Xcode, remove arbitrary `build`/`dist` folders, or sweep global IDE caches. It does not manage remote Docker endpoints, remote computers, or cloud workspaces. Windows and Linux cleanup are not supported.

These generated directories are treated as disposable. Pin them if you keep manual changes or irreplaceable files inside them. Removing dependencies or build output means a later install/build may take longer and require internet access. Registered disposable simulator and emulator data is permanently deleted. There is no universal undo for caches or devices.

## Detailed inspection and ownership

```sh
aigc status python --owners
aigc status docker --builders
aigc inspect 'EXACT-RESOURCE-ID'
aigc duplicates --json
aigc preview 'EXACT-RESOURCE-ID'
aigc own 'EXACT-RESOURCE-ID' --owner task-123
aigc require 'EXACT-RESOURCE-ID' --project my-project
aigc unrequire 'EXACT-RESOURCE-ID' --project my-project
```

`own` records ownership without authorizing deletion. `require` protects a resource needed by a project, including future builds. `manage` authorizes disposal for opt-in categories after all other checks. Regular linked worktrees need no `manage` command. Inspection includes process IDs/start times, known consumers, native metadata, observation coverage, reconstruction notes and protection reasons. It does not identify every agent session automatically.

`duplicates` groups matching recorded dependency/build inputs. It never merges environments or assumes matching lockfiles make two mutable installations interchangeable. Native previews can include resources that aigc would protect; a preview does not grant deletion permission.

See [adapter behavior and limitations](docs/ADAPTERS.md) for exact scope, and the [original research](docs/CLEANUP-RESEARCH.md) for rationale and longer-term ideas.

## Default policy

Both recommended installers start the service immediately. Each hourly run inventories resources and removes those that qualify:

| Setting | Default |
| --- | --- |
| Normal inactivity | 7 days |
| Inactivity when available space is below the target | 7 days |
| Free-space target | 20 GiB |
| Docker cache storage setting | 5 GiB |
| Clean-worktree recovery bundle budget | 2 GiB; further clean-worktree removal stops when it would be exceeded |
| Estimated removal limit per pass | 50 GiB; at most 10 eligible actions, with complete revalidation per action |
| Opt-in package-cache budget | 5 GiB per reported cache |
| Maintenance/recollection cooldown | 7 days |
| Schedule | Every hour while your user session is logged in; also when loaded |

**A first install can remove old regular linked worktrees immediately.** Their clock uses the newest file or directory modification time, HEAD commit time and any detected use. It cannot tell whether somebody read or intends to reuse a worktree. All other filesystem resources require seven days of observed inactivity; a change in size, modification time, entry count or detected use resets that clock, as does a monitoring gap longer than 48 hours. Docker build cache uses its native age and usage filters and can also qualify on the first pass.

The free-space target is a policy trigger, not a guarantee or hard quota. The collector does not remove protected resources to meet it. Native Buildx pruning applies an exact record selector, its age filter and `--max-used-space`. Size estimates are conservative when Docker returns rounded text. Native commands may reclaim less than their reported scope; the free-space target remains a trigger, not a guarantee.

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

### Docker images: protect what is still needed

An image referenced by **any running or stopped container** is protected, including when it was previously marked disposable. After the last container reference disappears, a fresh inactivity period must pass before collection. The free-space target never overrides these checks.

Other images are protected by default too. Removal requires explicit disposable registration, the inactivity period, no pin, and complete activity checks. The collector uses `docker image rm` without force; it never runs broad `docker system prune` or `docker image prune`. Build-cache pruning does not remove image objects.

There is no popularity ranking or fixed number of images to keep. For example, an image used by a Curly container stays protected while that container exists, even when stopped. Images needed only for a future build or referenced in a Compose file may have no container reference: leave them unregistered or pin their exact resource ID. The collector protects discovered Compose and simple Dockerfile references. Dynamic or failed reference discovery blocks image cleanup. Files outside configured discovery and future requirements still need explicit pins or `require`.

Older configuration files containing the retired `docker_keep_most_used` setting remain readable; that setting is ignored and disappears when configuration is next saved. Old ranking state is ignored, and snapshots from the previous policy are refreshed.

### Opt in disposable images and virtual devices

Copy an exact resource ID from `aigc status <category> --json`:

```sh
aigc manage 'simulators:UUID-FROM-STATUS' --owner mobile-builds
aigc manage 'emulators:throwaway-pixel' --owner mobile-builds
aigc manage 'docker-images:sha256:FULL-IMAGE-ID' --owner task-123
aigc unmanage 'emulators:throwaway-pixel'
```

Registration authorizes disposal after policy checks for images and devices. Regular linked worktrees need no registration. They still honor pins, activity checks, open-PR checks and app-managed worktree protection. For devices/images, register only data you are willing to lose. Inventory still works without registration.

For an eligible regular Git worktree, `aigc` uses [GitHub CLI](https://cli.github.com/manual/) to check open PRs in the checkout repository and its fork parent. Named branches use their branch name; detached checkouts use PRs associated with the HEAD commit. A detected open PR protects the worktree. **By default, a missing GitHub CLI, failed authentication or failed lookup does not block an otherwise eligible seven-day-old worktree.** The report labels unavailable verification. Set `worktree-require-pr-verification true` to retain worktrees whenever this check fails. Checks run during inventory and before removal. Commit associations and repository discovery cannot identify every related PR, especially PRs targeting unrelated repositories; pin those worktrees.

Codex-managed trees stay protected in aigc. Use Codex’s worktree cleanup settings to lower its retained-worktree limit, or archive completed chats. Codex saves a recovery snapshot and protects pinned, running and permanent worktrees; see [OpenAI’s worktree documentation](https://learn.chatgpt.com/docs/environments/git-worktrees). aigc does not rewrite Codex’s private state or directly remove its managed folders.

## Use it from an agent

Use `--json` for reports (schema version 2) and exact IDs. Exit code 0 means the command completed, not that anything was deleted; inspect the outcomes and warnings. Invalid commands/configuration, lock contention, and command failures exit nonzero. Errors are currently text on stderr.

```sh
aigc status --json
aigc plan --json
aigc run -- npm run build
aigc run -- xcodebuild -scheme MyApp build
```

`aigc run` reserves all resources by default. Pass repeatable `--resource` IDs or absolute paths and `--owner` for scoped reservations. It creates a separate process group and retains the reservation until the foreground command and inherited background group members finish. Explicitly daemonized processes that escape that group are not tracked: pin their resources. A crashed wrapper leaves a protective reservation; inspect `aigc leases`, then explicitly `aigc release-lease ID` only after its work has ended. A failed child makes the wrapper fail.

```sh
aigc run --resource /absolute/path/to/project --owner task-123 -- cargo build
aigc register /absolute/path/to/project/package-staging --kind scratch \
  --owner task-123 --purpose 'Disposable packaging output' --retain-days 30
aigc unregister /absolute/path/to/project/package-staging
```

Registration is explicit permission to dispose of generated contents after its minimum deadline and the normal observation period. It cannot authorize tracked source, nested Git repositories, protected application state or personal folders.

Collection uses `lsof`, process inspection, device state, filesystem observations and native Git/Docker checks. For regular worktrees, it checks for open files and working directories in that tree, scoped reservations and global `aigc run` reservations; a recognized process elsewhere does not block cleanup of an unrelated worktree. Other categories defer while recognized builds or agents run. **These are best-effort signals, not proof that an arbitrary paused agent is finished.** External tools do not take aigc's lock, so a process can start between inspection and deletion. Use reservations and pins for important work. aigc's own commands serialize state updates and collection with a lock.

No MCP server is required. See [agent usage](docs/AGENTS.md) for a short integration guide.

## Recovery, storage and uninstall

State lives in `~/Library/Application Support/aigc`:

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

GitHub Actions builds downloadable Apple Silicon and Intel archives on pushes to `main`, pull requests, and manual builds. Version tags publish both archives and `SHA256SUMS` to GitHub Releases. A failed build prevents publishing. See [release instructions](docs/RELEASING.md).

MIT licensed. Contributions that improve activity detection, tool compatibility and recovery are welcome.
