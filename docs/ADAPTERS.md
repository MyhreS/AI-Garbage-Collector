# Adapter behavior in v0.4

AI Garbage Collector runs on the local Mac, using already-installed tools. It does not install dependencies, start Docker builders, run project build scripts or contact a remote development environment. Unavailable tools are reported as unavailable; missing information never authorizes removal.

## Evidence and commands

`status --json` uses report schema 2. Each resource has an `evidence` object with:

- Explicit owners, separate from disposal permission.
- Known consumers and whether each consumer blocks collection.
- Processes with PID, start time when available, executable name and cwd.
- Native last-use timestamps/counts when exposed, plus untouched native text where only approximate output exists.
- First observation, last observed active use and monitoring gaps.
- Filesystem identity, native action, reconstruction notes and protection reasons.
- Input fingerprints for advisory duplicate reports.

`inspect ID` performs a fresh inventory and returns one resource. `status python --owners` and `status docker --builders` expose the deeper metadata. `own ID --owner TASK` records ownership. `require ID --project PROJECT` protects an explicit future requirement. `unrequire` removes that reference. Neither `own` nor `require` authorizes deletion.

`manage ID --owner TASK` marks an opt-in resource disposable. Its current references, pins, activity, observation period and other protections still apply. Regular linked Git worktrees no longer need registration. Resource IDs should be copied exactly from status; Python environments discovered through existing project-local inventory can retain a `dependencies:` ID for compatibility.

Regular linked Git worktrees use the latest file or directory modification time, HEAD commit time and detected use as the inactivity clock. Seven-day-old trees can qualify on the first scan, including trees with tracked edits, untracked files and ignored files. Those dirty trees are removed with `git worktree remove --force` and **no recovery copy of local files**. The Git branch remains. Clean trees still require a verified HEAD bundle. Set `worktree-cleanup false` to disable this adapter or `worktree-force false` to retain dirty trees.

Before deletion, regular worktrees require a successful authenticated GitHub CLI lookup for open PRs on their branch in the checkout repository and its fork parent, if any. An open PR or failed lookup protects the worktree. The lookup is repeated before removal; PRs targeting unrelated repositories require an explicit pin or `require` entry. Primary, locked, submodule-containing, current-working-directory and app-managed Codex worktrees remain protected. Filesystem write times cannot reveal an agent that only reads a tree or plans to return to it, so pins and `aigc run` reservations remain useful.

`duplicates` groups matching recorded inputs. Python fingerprints include lockfile, interpreter configuration and installed distribution metadata (including available direct-URL records); build/Node fingerprints cover available lock inputs. Fingerprints do not establish identical mutable contents, selected flags or safe interchangeability. No environments are merged.

## Docker

### Images

Images remain opt-in. The scanner records all tags/digests and references from every existing container, including stopped ones. It also resolves conventional Compose file image names and static Dockerfile `FROM` references in discovered projects. Dynamic Dockerfile arguments or failed Compose resolution make image inventory incomplete and prevent removal.

Only image references are retained from Compose; expanded environment values are not stored. Custom Dockerfile filenames, override combinations, external scripts and projects outside discovery are not a complete future-requirement graph. Keep such images unregistered or add a `require`/pin.

Native image disk usage retains Docker's shared/unique size text. Image creation time is never treated as last use. There is no top-three or popularity protection rule, and no claim of a complete historical image-use counter. Images are removed by immutable ID through non-forced `docker image rm`.

### BuildKit cache

The adapter inventories running local single-node builders with `docker` or `docker-container` drivers. It resolves named-context endpoints and accepts local Unix sockets only. Remote, multi-node, stopped and unsupported builders are skipped with warnings. Aliases resolving to the same local Docker endpoint are deduplicated.

Records include native parents, type, mutable/shared/reclaimable flags, usage counts and last-use output. Some installed Buildx versions return relative age and rounded sizes even in JSON. These remain labelled native text; byte values are upper estimates. No precise timestamp is invented from “two weeks ago.”

Only reclaimable, private, immutable regular records are candidates. Cache mounts, internal/frontend records and shared records remain protected. Each prune uses an anchored exact-ID selector, native age filtering and `--max-used-space`. The installed CLI must support those flags. Docker decides age eligibility again; a planned candidate can therefore produce a no-op. There is no broad builder prune fallback.

A pin on any cache record protects that builder's records. A failed native inventory disables affected cleanup. Docker's VM disk is never truncated or removed, and host free-space changes remain separate from engine-reported reclamation.

## Python

The scanner associates project-local environments and Poetry-listed central environments with discovered `pyproject.toml` projects. It resolves Poetry's configured environment path. Unknown-owner central environments are protected until explicitly registered as disposable generated storage. Linked/centralized environments and environments referenced by multiple projects are protected.

Known Poetry environments are removed through Poetry. Other explicitly disposable, validated project environments use directory removal. `pyvenv.cfg`, path identity, tracked-source checks, native association and activity are rechecked. Installed uv/pipx tool environments in recognized locations are report-only.

This does not enumerate every environment on the whole disk, execute environment interpreters, infer all extras/groups or discover arbitrary tool-manager roots. Discovery is bounded; use configured project roots and explicit registration where ownership is known. Custom tool installations should remain unregistered.

## Shared caches and downloaded tools

| Storage | Discovery | Cleanup |
| --- | --- | --- |
| pip | `pip3 cache dir`, native summary | Opt-in `pip3 cache purge`; covers HTTP and wheel cache |
| pnpm | `pnpm store path` | Opt-in `pnpm store prune`; native store-server protection applies |
| npm | Configured cache root | Opt-in `npm cache verify`; this command mutates the cache |
| uv | `uv cache dir` | Report-only: prune may remove centralized environments or break symlink consumers |
| Poetry | Configured cache root and cache names | Report-only: root may include environments |
| Homebrew | `brew --cache` | Native dry-run through `preview`; cleanup can affect Cellar versions beyond the cache |
| Cargo | Configured/default Cargo registry cache | Report-only; keep Cargo's native cache GC |
| Gradle | Configured/default user home | Report-only; use Gradle's version-aware retention |
| Yarn/Bun | Documented locations; Bun environment override | Report-only; no assumption of complete project references |
| Playwright | Default/custom browser path, revisions and available package links | Report-only; keep Playwright's native package-aware GC |
| npx | Installation directories under configured npm cache | Report-only; arbitrary tool use cannot be inferred from age |

Supported native maintenance requires explicit disposable registration, cache size above `budget.package-cache`, observed inactivity and the configured cooldown. Pins inside a cache protect its containing resource. Native paths are queried again immediately before maintenance. A successful operation is recorded as maintenance, not a claim that the whole cache was deleted.

No cache manager is installed automatically. A manager's default path is not proof that every project uses that path. Do not mark a shared cache disposable if an untracked consumer relies on its exact contents or offline availability.

## Build output and registered scratch

Existing conventional Rust, Swift and Next.js output remains discoverable. The richer scanner adds project associations, conventional Gradle build directories and statically configured Cargo target directories. Shared discovered outputs are protected. It reads configuration; it does not execute Gradle or build scripts to resolve dynamic destinations. Global Cargo config, workspace configuration outside discovery and custom flags are not exhaustively resolved; register known custom output explicitly.

Swift output containing package checkouts is protected. Xcode DerivedData records include workspace metadata when available. Directories containing source-package checkouts remain protected. Archives, signing material and exported release deliverables are outside automatic cleanup.

Use explicit registration for disposable packaging output, temporary downloads and generated custom builds:

```sh
aigc register /Users/me/Projects/task/package-staging --kind scratch \
  --owner task-123 --purpose 'Rebuildable packaging output' --retain-days 30
```

Supported kinds are `scratch`, `builds` and `python`. Registration records owner, purpose and minimum retention deadline. It authorizes generated-content disposal only. Home roots, recognized sensitive/application/personal locations, tracked source, nested repositories, noncanonical paths and incomplete scans are refused. A registered Python path must contain `pyvenv.cfg`. A regular linked Git worktree is handled by the worktree policy above.

`unregister PATH` withdraws this registration. `unmanage ID` separately withdraws ID-based disposal permission.

## Mobile resources

### Simulator runtimes

The scanner connects simulator runtime identifiers/builds to native runtime disk UUIDs where the installed schema exposes them. Every retained simulator device referencing a runtime protects it, including shut-down devices. Unknown runtime schemas remain protected.

Runtime removal requires explicit disposal, observed inactivity, no retained device references and fresh native identity checks. It uses exact-UUID `simctl runtime delete`; it never sweeps mounted runtime directories. Native deletion can otherwise shut down devices, so consumer checks are required. An external process can still race a final check; cooperate through reservations and pins.

`preview RUNTIME-ID` uses the native age-based `--dry-run` when supported. That preview can list all native age candidates and is not an aigc-approved deletion list.

### Android

Installed packages come from `sdkmanager --list_installed` under the configured SDK root. The adapter maps AVD system-image references and simple literal Gradle platform/version declarations. Dynamic or absent declarations protect packages. Implicit build-tool/CMake requirements and undeclared NDK versions remain conservative.

SDK removal requires explicit disposal, no retained known consumers, complete inventory and the normal observation period. It invokes native package-ID uninstall after revalidating roots and references. Custom Gradle plugins and undiscovered projects require explicit pins/requirements; installing an older SDK does not make it garbage.

Existing disposable-AVD handling remains, including deferral while an emulator may be running. Device app data is not recoverable after an authorized device deletion.

## Reservations and races

`run --resource ID --owner TASK -- COMMAND` accepts repeatable IDs or absolute paths; omitting resources reserves everything. IDs must exist in the last inventory. The wrapper writes a protective lease before spawning, creates a process group and waits for inherited background members after the foreground command exits.

A process that explicitly escapes the group cannot be tracked by this mechanism. Use pins for daemonized work. A crash leaves the lease in place, including older-format leases; `leases` shows retained records. `release-lease ID` is an explicit user/agent statement that the protected work has ended, not automatic proof of inactivity.

Broad recognized-process deferral remains for categories other than worktrees. A regular worktree instead uses its own open-file/working-directory evidence, scoped reservations and global `aigc run` reservations, so an agent elsewhere does not indefinitely block old-tree collection. OS inspection cannot establish that every paused agent has finished. No private agent session database is modified.

## Accounting, limits and migration

- Schema 2 adds evidence and a filesystem union measurement. Hardlinks and nested resources are counted once in that union; APFS clones, snapshots and VM internals prevent an exact universal reclaimable number.
- Tree walks stop at 500,000 entries per resource; union measurement stops at one million. Deep adapters inspect at most 250 discovered projects. Incomplete project discovery disables new reference-dependent cleanup.
- Each pass attempts at most ten initially eligible resources. Every action gets fresh inventory/policy and identity checks; changed resources are skipped. Full revalidation is intentionally conservative and can take time on large roots.
- Native maintenance and recollection have a default seven-day cooldown. History separates `removed`, `maintained`, `no_op` and `skipped`, estimated bytes, optional native reclaimed bytes and observed host free-space change.
- `deep-inventory=false` disables the new discovery layer, including new adapters; it does not enable an older broad Docker pruning fallback.
- Existing config loads with defaults for new fields. Policy version 4 invalidates old cached reports. Regular worktree eligibility uses filesystem/commit age on the first scan; other filesystem observations still restart when identities change. The executable does not migrate or delete user data.

The release does not implement a continuous Docker event listener, exact agent-session attribution, automatic mutable-environment sharing, full dynamic build evaluation or exact APFS extent accounting. Those limitations are visible rather than replaced with guessed ownership or fabricated usage statistics.
