# Cleaning up after parallel AI agents

Research date: 2026-10-01. Scope: local macOS storage. This is a proposal, not a description of additional shipped cleanup behavior. The [README coverage table](../README.md#what-it-does-and-does-not-clean) remains authoritative for v0.1.

## Recommendation

Build richer inventory and ownership information before broadening automatic deletion. Start with Docker build cache and Python environments: both can grow across many agent tasks, and their tools expose useful metadata beyond folder size. Then add package-store maintenance and build-output ownership. Keep SDK and simulator-runtime removal explicit until their consumers can be identified reliably.

The product should answer four questions for every resource:

1. What created it, and which projects or tasks still need it?
2. What evidence shows recent use, and how complete is that evidence?
3. How much space might removing it actually recover?
4. Can it be recreated, and what would that cost in time or downloads?

An old modification timestamp answers none of these by itself. An installed dependency can be read repeatedly without being modified. Conversely, a background indexer can touch a resource without anyone needing the project.

## What the current implementation actually knows

Source reviewed: `src/inventory.rs`, `src/policy.rs`, `src/collect.rs` and `src/runtime.rs` at commit `470faead4a34017d095988bfe769103e257b3570`.

| Area | Existing evidence | Important gap |
| --- | --- | --- |
| Filesystem resources | Allocated blocks, entry count, latest modification, repeated observations, completeness | No authoritative last-use timestamp, dependency equivalence or cross-resource ownership |
| Processes | Open paths from `lsof`, recognized process names, global build/agent deferral | Open paths are not retained as PID-to-resource relationships; no agent/session attribution |
| Reservations | `aigc run` child PID and start time | Protects everything; no resource-specific reservations or detached child lifecycle |
| Worktrees | Git registration, locks, working-tree changes including ignored files, recovery eligibility | No exact agent identity; app-managed trees remain protected |
| Docker images | Image ID, first tag, size, references from running and stopped containers | No complete tag set, Compose requirements, historical use or unique-space accounting |
| Docker build cache | Default builder aggregate count and Docker size/reclaimability text | No per-record last use, usage count, parent relationships or other local builders |
| Python | Recognized project-local `.venv` | No central Poetry environments, tool environments or shared-environment references |
| Devices | Device inventory and current state | Shutdown does not establish abandonment; runtime consumers are not mapped |

The earlier discussion of deeper worktree statistics was a design investigation. It did not implement per-agent ownership. The same evidence model proposed below should support worktrees and the other categories.

## 1. Docker: separate images from build cache

### Build cache is the strongest initial adapter

`docker buildx du --format=json` exposes cache-record IDs, parents, creation and last-use times, usage counts, sizes, shared/mutable/reclaimable flags and record types. Its JSON output is newline-delimited. These are cache records, not image popularity statistics. [Buildx disk-usage reference](https://docs.docker.com/reference/cli/docker/buildx/du/).

**Proposed inventory:** enumerate explicitly local builders, validate every builder node's endpoint, then collect records per builder. A local Docker context does not establish that every Buildx builder is local. Unavailable or remote builders should appear as skipped, never as empty. Record native field availability because installed Buildx versions differ.

**Proposed cleanup:** use the selected builder's native pruning with supported age and capacity controls. Buildx supports last-use age filtering, record/type/sharedness selectors and space controls. BuildKit also has its own periodic GC policies. Prefer configuring or invoking these mechanisms over deleting cache directories. [Buildx prune](https://docs.docker.com/reference/cli/docker/buildx/prune/), [BuildKit GC](https://docs.docker.com/build/cache/garbage-collection/).

Keep expensive cache mounts conservatively until their use and rebuild cost are understood. Preview scope and estimated savings, then re-query immediately before native pruning. If a requested pin cannot be expressed by the installed native filters, skip that pruning scope. Never present a broad native prune as an exact approved list of record deletions.

### Images need references, not a top-three rule

There is no general Docker image last-used counter comparable to BuildKit's cache usage fields. Image creation time is not last use: image pruning's `until` filter selects by creation time. An old image can power today's workload. [Image prune reference](https://docs.docker.com/reference/cli/docker/image/prune/).

Add these relationships to an image report:

- All tags and repository digests, plus immutable local image ID.
- Every running and stopped container that refers to it.
- Discovered Compose image declarations and resolved Dockerfile base-image references.
- Explicit task ownership and disposable status, stored separately.
- Last observed container use, with observation coverage and gaps.

Compose can output image names through `docker compose config --images`. Configuration involves variable resolution, so missing variables, overrides and profiles can leave requirements unresolved. Extract only necessary references; do not persist expanded secrets or whole environments. Dynamic Dockerfile arguments and external scripts can also leave unknown requirements. [Compose config](https://docs.docker.com/reference/cli/docker/compose/config/).

Docker events could improve observations, but only the latest 256 historical events are returned. An hourly poll can miss activity. A future event listener needs bounded local history and an explicit gap marker when disconnected. Missing events must never imply non-use. [Docker events](https://docs.docker.com/reference/cli/docker/system/events/).

**Deletion rule:** retain the current explicit disposable registration, pins, container protection and non-forced exact-ID removal. Add protection for known project requirements. Never introduce a popularity quota. No container reference means only that no existing container uses the image; it does not prove nobody will need it tomorrow.

### Report savings honestly

Docker's verbose disk report distinguishes shared and unique image size. Removing one of several tags or an image with shared layers may reclaim very little. Report these separately from logical image size. [Docker disk usage](https://docs.docker.com/reference/cli/docker/system/df/).

Docker Desktop stores data in a VM disk image. Show engine-reported reclaimed space separately from observed host free-space change; reclaim timing and sparse allocation affect the latter. Never remove or truncate `Docker.raw`. [Docker Desktop Mac storage FAQ](https://docs.docker.com/desktop/troubleshoot-and-support/faqs/macfaqs/).

## 2. Poetry, uv and redundant Python environments

### Discover ownership before looking for duplicates

For each discovered Poetry project, use `poetry env list --full-path` and `poetry env info --path` to associate environments. Poetry supports removing a particular environment; deleting all environments is unnecessarily broad. These commands describe the selected project, not a universal registry of every project on the disk. [Poetry environment management](https://python-poetry.org/docs/managing-environments/).

Resolve configured cache/environment paths and project-local `.venv` settings. Do not assume every environment lives in one standard directory. External active environments may be reused by Poetry. Environment names are hints, not proof of project ownership. [Poetry configuration](https://python-poetry.org/docs/configuration/).

**Proposed record:** canonical environment path and file identity, interpreter path/version/architecture, manager/version, owning worktree(s), manifest/lock digest, selected dependency groups/extras when known, editable local dependencies, active interpreter processes, observed activity, disposable authorization and reconstruction instructions.

A removed project can leave an environment with no discoverable owner. Label it **owner unknown** until a prior registration or another trustworthy record establishes ownership. Renamed projects, disconnected disks and roots outside discovery can otherwise look abandoned.

### Duplicate candidates are not interchangeable environments

Compare lockfile digest, Python implementation/ABI/architecture, dependency selections and editable source paths. Matching lockfiles alone are insufficient. A group selection or local package can make two installations different. Mutable environments may also have additional packages or manual changes.

Proposed output should distinguish:

- Multiple environments that appear to have equivalent dependency requirements.
- Different environments sharing cached package data.
- Environments owned by completed disposable tasks.
- Environments whose owner or recreation inputs cannot be established.

Automatically replacing several worktrees' `.venv` directories with one shared mutable environment is not recommended. One agent's install could change another agent's dependencies. First remove known disposable, inactive environments; encourage native package-cache sharing for future installs.

### uv has native cleanup, with important boundaries

Use `uv cache dir` rather than a guessed path. Use native cache operations, not manual directory surgery. Current uv documentation says `uv cache prune` removes unused entries **and all centralized project environments**. Cache mutation coordinates with other uv operations, but ordinary programs using an installed environment are not necessarily protected by that lock. Never use its force override. Avoid the CI-oriented prune mode as a laptop default because retaining useful downloaded packages reduces repeated downloads. [uv caching](https://docs.astral.sh/uv/concepts/cache/).

The current centralized-project-environment feature is a preview and can place `.venv` as a link to cache storage. Resolve this as a consumer relationship while preserving the link boundary; do not treat the target as exclusively owned by one worktree. Capability checks must account for installed version and enabled features. [uv project layout](https://docs.astral.sh/uv/concepts/projects/layout/).

**Proposed first release:** inventory Poetry and uv environment relationships; clean only explicitly disposable project environments after checking all known consumers. Keep global uv prune opt-in until cached-environment implications are handled. Protect uv/pipx tool environments and Python installations: an environment hosting Poetry itself is an installed tool, not abandoned project output.

### Package caches are a separate category

Poetry exposes cache listing and scoped clearing, but these are not proof of project abandonment. Pip exposes configured cache location, size information, wheel listing and removal; a full purge also clears HTTP cache. Prefer occasional budget-based maintenance to wholesale purges. Preserve expensive source-built wheels and explain likely downloads. [Poetry CLI](https://python-poetry.org/docs/cli/), [pip caching](https://pip.pypa.io/en/stable/topics/caching/).

## 3. JavaScript dependencies and browser downloads

| Candidate | Evidence and proposed behavior |
| --- | --- |
| Project `node_modules` | Connect to worktree, lockfile, manager, active dev server and tool reservations; retain current generated-directory safeguards |
| pnpm store | Resolve with `pnpm store path`; use native `store prune` occasionally, never manually remove hashed entries |
| npm download cache | Resolve configured cache; offer native maintenance with a budget and cooldown |
| npx tool installations | Add inventory through installed npm's supported cache commands; distinguish running tools from old downloaded executables |
| Playwright browsers | Associate browser revisions with requesting packages and custom browser paths; respect native GC and active browser processes |
| Yarn/Bun caches | Initially report-only until adapters establish version-specific location, references and removal behavior |

pnpm supports pruning unreferenced packages and rejects pruning while its store server is running. Newer global virtual-store functionality also tracks project references; it must not be assumed on older installations. Frequent pruning can cause downloads when switching branches. [pnpm store](https://pnpm.io/10.x/cli/store).

`npm cache verify` performs garbage collection as well as integrity checks: it is a mutation, not a read-only status command. Current npm also documents npx cache listing, details and removal. Probe command support rather than assuming every installed npm has those features. [npm cache](https://docs.npmjs.com/cli/v11/commands/npm-cache/).

Playwright already tracks packages needing its browsers and garbage-collects older revisions as packages update. It also supports shared custom paths and project-local browser installations. Let its ownership mechanism guide collection; do not delete a browser because another project uses a newer version. [Playwright browser management](https://playwright.dev/docs/browsers).

## 4. Build output: resolve the real destination

### Rust

Cargo output can be redirected with configuration or `CARGO_TARGET_DIR`, and a workspace can share output. Map consumers to the resolved target directory and record profiles/targets before deleting anything. Cargo's native clean command can narrow removal by supported selectors. Keep exported executables and release deliverables outside automatic cache policy. [Cargo build cache](https://doc.rust-lang.org/cargo/reference/build-cache.html), [cargo clean](https://doc.rust-lang.org/cargo/commands/cargo-clean.html).

Cargo already tracks use and automatically cleans its global cache; this is distinct from project build output. Report its native settings rather than adding a competing filesystem sweeper. [Cargo cache configuration](https://doc.rust-lang.org/cargo/reference/config.html#cache).

`cargo-sweep` demonstrates selective artifact cleanup, but its repository currently declares it unmaintained. Do not make it a default runtime dependency. A future adapter must establish compatibility and ownership independently. [cargo-sweep repository](https://github.com/holmgr/cargo-sweep).

### Gradle and Android builds

Gradle has native maintenance of versioned and shared caches, with configurable retention in newer versions. Multiple versions may share a user home and apply different policies. Prefer reporting and optionally configuring native retention over deleting `~/.gradle` subdirectories. [Gradle-managed directories](https://docs.gradle.org/current/userguide/directory_layout.html).

For project output, resolve the project/build relationship and protect active daemon work. A daemon existing does not prove every project is busy; absent command-line attribution does not prove an output is idle. Running Gradle to discover settings can execute project code. Status should read known configuration or previously recorded build metadata and return unknown when static discovery cannot resolve it.

### Xcode and Swift

Improve DerivedData inventory with project/workspace association where available, active build processes, selected developer directory and custom output paths. Treat any undocumented metadata layout as a versioned hint. Distinguish rebuildable objects/indexes from source-package checkouts, logs and archives. Check checkouts for edits before proposing removal; keep release archives, signing material and exported deliverables protected.

A future build wrapper can record output paths directly, which is stronger than guessing from a directory name. Preserve native application ownership for app-managed data.

## 5. Simulators, runtimes and Android SDK packages

### iOS runtimes: native age information exists

Read-only capability inspection on Xcode 26.3 (build 17C529) confirmed:

- `xcrun simctl help runtime` advertises JSON runtime listing.
- It advertises `runtime delete --notUsedSinceDays <days> --dry-run`.
- Its help warns that runtime deletion can shut down booted simulators.

Only help/version commands were run for this research; no runtime deletion or dry-run deletion was executed. This CLI capability is local evidence, not a promise for every Xcode version. Apple's documented component-management UI also supports removing unused runtimes. [Xcode component management](https://developer.apple.com/documentation/xcode/downloading-and-installing-additional-xcode-components).

**Proposed adapter:** discover capabilities, combine native age eligibility with runtime-to-device references, current device state, supported Xcode versions and explicit retention preferences. Protect booted devices and their runtimes independently of the native delete command. A runtime used by a retained device or declared project requirement stays protected. Recheck immediately before removal; without coordination, a concurrent boot remains a race.

Keep runtime cleanup opt-in initially. Avoid blanket deletion of “unavailable” devices: unavailable under one selected Xcode does not establish that their app data is disposable. Do not manually sweep mounted runtime volumes or protected system assets.

### Android SDKs and emulators

Use SDK package IDs and native package management. `sdkmanager` supports listing and uninstalling packages; `avdmanager` supports device inventory and deletion. Neither establishes every project's future SDK requirements. [sdkmanager](https://developer.android.com/tools/sdkmanager), [avdmanager](https://developer.android.com/tools/avdmanager).

Build proposed relationships from AVD configuration to system image; from project declarations to compile SDK, build-tools, NDK and CMake; and from running emulator processes to AVD. Respect configured SDK/AVD roots. Keep unresolved dynamic Gradle requirements protected. Old API levels can still be intentional compatibility targets.

Delete only an explicitly disposable inactive AVD or explicitly selected SDK package with no retained known consumer. Preserve snapshots/user data unless disposal was authorized. Never infer that a downloaded package is unused merely because its version is old.

## 6. Other worthwhile candidates

Homebrew offers native cleanup and dry-run support. Add a report of downloadable cache and obsolete-version cleanup candidates, with the native preview shown before any opt-in action. Do not extend this into uninstalling current developer tools or automatic `autoremove`. [Homebrew manual](https://docs.brew.sh/Manpage).

Agent-owned logs, temporary downloads, packaging staging folders and duplicate release clones are useful future targets **when registered at creation** with owner and retention deadline. Generic `dist`, `build`, Downloads and `/tmp` sweeps are inappropriate: names do not establish ownership or reproducibility. Persistent databases, Docker volumes, credentials, user documents and saved agent conversations remain outside automatic cleanup.

## Shared design for richer statistics

### Evidence and relationships

Represent resources and consumers separately. A task may own an environment; several projects may consume one cache; a container refers to an image; an AVD refers to an SDK image. Ownership alone does not authorize disposal.

Proposed fields:

| Field | Meaning |
| --- | --- |
| Stable resource identity | Native ID plus local engine/builder, or canonical path plus filesystem identity |
| Owners and consumers | Task IDs, worktrees, projects, processes and other resources; include evidence source |
| Process identity | PID plus process start time, executable, relevant cwd/open-path association; never PID alone |
| Usage times | Native last-use, last observed use, first seen and observation gaps as separate fields |
| Usage count | Native count or observed count, explicitly labelled; absent is unknown, not zero |
| Storage | Logical, allocated, shared and estimated reclaimable bytes; provenance and uncertainty |
| Reconstruction | Lock/build inputs, expected downloads, locally observed rebuild time when available |
| Decision | Eligible/protected/unknown plus every blocking reason and failed discovery check |

Use process trees and resource-specific leases to improve attribution. Foreground completion must not release a reservation while tracked descendants still use the resource. A paused agent or closed application may still own unfinished work; use explicit task lifecycle integration where available, and do not scrape private application databases as a default solution.

### Space accounting

Count nested resources once in totals: a worktree already contains its `node_modules`. Deduplicate hardlinked inodes across the inventory where possible. APFS clone extents and snapshots make exact physical reclamation harder; expose unknown/shared estimates rather than subtracting logical sizes. Docker owns its internal layer accounting.

Track native-reported reclaimed bytes, estimated removed bytes and observed host free-space delta separately. Record no-op pruning as a no-op. Add rebuild/download churn statistics so repeatedly deleting useful cache does not look like success.

### Collection protocol

1. Discover resources and consumers within configured limits; record missing coverage.
2. Apply pins, ownership/disposal authorization and freshness rules.
3. Plan native operations and show their actual scope and uncertainty.
4. Obtain cooperative reservations where possible; recheck identity, references and activity.
5. Execute the narrowest supported operation without force overrides of activity protection.
6. Record outcomes and failures; measure disk changes; apply a cooldown before reconsidering rebuilt data.

Observation plus a final check still has a race with uncooperative processes. Strong guarantees require cooperation with the creator or native locking. When an adapter cannot protect pinned resources, establish ownership or verify activity, keep it report-only. Low free space does not override these constraints.

### Proposed CLI experience

The commands below are **design examples, not available commands**:

```text
aigc inspect RESOURCE --json
aigc status python --owners
aigc status docker --builders
aigc duplicates --json
```

An inspection should explain “protected: referenced by stopped container” or “unknown: Poetry project mapping incomplete,” show the source and timestamp, and list expected rebuild consequences. A duplicate report should show candidates and confidence rather than imply safe interchangeability.

Keep baseline scans cheap. Cache slow size walks, bound process/event history, expire stale observations conservatively and offer deeper inspection on demand. Do not hash entire environments on every hourly pass. Avoid retaining command arguments, credentials, prompts or source contents in status/history.

## Proposed implementation order

| Phase | Deliverable | Initial deletion scope |
| --- | --- | --- |
| 1 | Resource/consumer relationships, PID attribution, evidence timestamps, correct overlapping-size accounting, detailed inspection | Existing scope only |
| 2 | Per-builder BuildKit records; Poetry/uv environment mapping; configured cache paths | Report first; existing Docker image protections retained |
| 3 | Narrow cleanup of registered abandoned environments; version-aware native cache maintenance with budgets/cooldowns | Opt-in adapters; no shared mutable-environment deduplication |
| 4 | Rust/Gradle/Xcode output ownership, Playwright/npx inventory, task-specific reservations | Known generated output with retained-consumer protection |
| 5 | Runtime age preview and device/SDK dependency relationships | Explicit runtime/package cleanup after complete evidence |

The most valuable first increment is a better answer to “what uses this, and why is it protected?” That evidence then supports useful automatic collection without guessing which agent has finished.

## Research boundaries

This change adds documentation only. No cleanup policy, configuration, release binary or host service was changed. No tests were added or run, and no user resources were removed. Local inspection was limited to source and read-only tool/version/help queries. Current upstream documentation can describe capabilities newer than an installed tool; every proposed adapter needs version/capability checks and explicit unknown states.
