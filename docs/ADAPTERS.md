# Cleanup adapters

AI Garbage Collector runs on the local computer, using already-installed tools. It does not install dependencies, run project build scripts or contact a remote development environment. Unavailable tools are reported as unavailable; missing information never authorizes removal.

## Evidence and commands

`status --json` uses report schema 2. `timings_ms` records total inventory time and time spent in each scan stage. Independent worktree and Poetry inspections use at most four workers; deletion remains sequential. Repository identity is reused within one inventory unless Git enables per-worktree configuration. PR state and filesystem/activity checks are refreshed before deletion.

Each resource has an `evidence` object with:

- Explicit owners, separate from disposal permission.
- Known consumers and whether each consumer blocks collection.
- Processes with PID, start time when available, executable name and cwd.
- Native last-use timestamps/counts when exposed, plus untouched native text where only approximate output exists.
- First observation, last observed active use and monitoring gaps.
- Filesystem identity, native action, reconstruction notes and protection reasons.
- Input fingerprints for advisory duplicate reports.

`inspect ID` performs a fresh inventory and returns one resource. `status python --owners` exposes the deeper metadata. `own ID --owner TASK` records ownership. `require ID --project PROJECT` protects an explicit future requirement. `unrequire` removes that reference. Neither `own` nor `require` authorizes deletion.

All implemented cleanup adapters are enabled by default. `manage ID --owner TASK` records an owner; it is not required to enable cleanup. References, pins, activity and age checks still apply. Resource IDs should be copied exactly from status; Python environments discovered through existing project-local inventory can retain a `dependencies:` ID for compatibility.

Regular linked Git worktrees use the latest file or directory modification time, HEAD commit time and detected use as the inactivity clock. Seven-day-old trees can qualify on the first scan, including trees with tracked edits, untracked files and ignored files. Those dirty trees are removed with `git worktree remove --force` and **no recovery copy of local files**. The Git branch remains. Clean trees are also removed without recovery archives. Set `worktree-cleanup false` to disable this adapter or `worktree-force false` to retain dirty trees.

Before deletion, regular worktrees check for open PRs in the checkout repository and its fork parent. Named branches use the branch name; detached checkouts use GitHub commit-to-PR associations. A detected open PR protects the worktree. Failed verification is recorded in metadata and the eligibility reason, but permits cleanup by default after the inactivity checks. Set `worktree-require-pr-verification true` to protect on lookup failure. The lookup repeats before removal. PRs targeting unrelated repositories and incomplete commit associations require a pin or `require` entry. Primary, locked, submodule-containing and current-working-directory worktrees remain protected. Codex-managed linked worktrees follow the same age, activity and PR policy as other linked worktrees. Their chats are not archived and no Codex snapshot is created; Codex-specific pins, chat timestamps and permanent-worktree settings are not inspected. Use aigc pins and reservations. Filesystem write times cannot reveal an agent that only reads a tree or plans to return to it, so pins and `aigc run` reservations remain useful. Detached commits have no retained branch and may eventually be pruned by Git after a worktree is removed without a bundle.

`duplicates` groups matching recorded inputs. Python fingerprints include lockfile, interpreter configuration and installed distribution metadata (including available direct-URL records); build/Node fingerprints cover available lock inputs. Fingerprints do not establish identical mutable contents, selected flags or safe interchangeability. No environments are merged.

## Python

The scanner associates project-local environments and Poetry-listed central environments with discovered `pyproject.toml` projects. It resolves Poetry's configured environment path. Old central environments qualify even when their original project no longer exists. Symlinked environments and environments referenced by multiple discovered projects are protected.

Python environments use the latest recursive file/directory write, native last-use timestamp when available and detected activity. Seven-day-old environments can qualify on the first scan. File timestamps cannot prove that an environment has not been read. On Windows, running interpreter executable paths protect their environments; a Python process whose executable path cannot be inspected defers Python cleanup. Unrelated agent processes do not block it.

Project-associated Poetry environments are removed through Poetry. Orphaned environments directly inside the native configured Poetry environment root use validated directory removal, with the root queried again immediately beforehand. Other recognized project environments use directory removal. `pyvenv.cfg`, path identity, tracked-source checks, native association and activity are rechecked. Installed uv/pipx tool environments in recognized locations are report-only.

This does not enumerate every environment on the whole disk, execute environment interpreters, infer all extras/groups or discover arbitrary tool-manager roots. Discovery is bounded; use configured project roots and explicit registration where ownership is known. Custom tool installations should remain unregistered.

## Shared caches and downloaded tools

| Storage | Discovery | Cleanup |
| --- | --- | --- |
| pip | `pip3 cache dir`, native summary | Automatic `pip3 cache purge`; covers HTTP and wheel cache |
| pnpm | `pnpm store path` | Automatic `pnpm store prune`; native store-server protection applies |
| npm | Configured cache root | Automatic `npm cache verify`; this command mutates the cache |
| uv | `uv cache dir` | Report-only: prune may remove centralized environments or break symlink consumers |
| Poetry | Configured cache root and cache names | Report-only: root may include environments |
| Homebrew | `brew --cache` | Native dry-run through `preview`; cleanup can affect Cellar versions beyond the cache |
| Cargo | Configured/default Cargo registry cache | Report-only; keep Cargo's native cache GC |
| Gradle | Configured/default user home | Report-only; use Gradle's version-aware retention |
| Yarn/Bun | Documented locations; Bun environment override | Report-only; no assumption of complete project references |
| Playwright | Default/custom browser path, revisions and available package links | Report-only; keep Playwright's native package-aware GC |
| npx | Installation directories under configured npm cache | Report-only; arbitrary tool use cannot be inferred from age |

Supported native maintenance requires observed inactivity and the configured cooldown, regardless of cache size. Pins inside a cache protect its containing resource. Native paths are queried again immediately before maintenance. A successful operation is recorded as maintenance, not a claim that the whole cache was deleted.

No cache manager is installed automatically. A manager's default path is not proof that every project uses that path. Do not mark a shared cache disposable if an untracked consumer relies on its exact contents or offline availability.

## Build output and registered scratch

Existing conventional Rust, Swift and Next.js output remains discoverable. The richer scanner adds project associations, conventional Gradle build directories and statically configured Cargo target directories. Shared discovered outputs are protected. It reads configuration; it does not execute Gradle or build scripts to resolve dynamic destinations. Global Cargo config, workspace configuration outside discovery and custom flags are not exhaustively resolved; register known custom output explicitly.

Swift output containing package checkouts is protected. Xcode DerivedData records include workspace metadata when available. Directories containing source-package checkouts remain protected. Archives, signing material and exported release deliverables are outside automatic cleanup.

Use explicit registration for disposable packaging output, temporary downloads and generated custom builds:

```sh
aigc register /Users/me/Projects/task/package-staging --kind scratch \
  --owner task-123 --purpose 'Rebuildable packaging output'
```

Supported kinds are `scratch`, `builds` and `python`. Registration records owner and purpose; normal inactivity rules apply with no additional retention deadline. It authorizes generated-content disposal only. Home roots, recognized sensitive/application/personal locations, tracked source, nested repositories, noncanonical paths and incomplete scans are refused. A registered Python path must contain `pyvenv.cfg`. A regular linked Git worktree is handled by the worktree policy above.

`unregister PATH` withdraws this registration. `unmanage ID` clears its managed owner label. Use `pin ID` to prevent automatic cleanup of recognized resources.

## Mobile resources

Simulators, simulator runtimes, Android emulators and SDK packages are outside scope. They are not inventoried or removed; manage them with Xcode, `simctl`, Android Studio or `sdkmanager`.

## Reservations and races

`run --resource ID --owner TASK -- COMMAND` accepts repeatable IDs or absolute paths; omitting resources reserves everything. IDs must exist in the last inventory. The wrapper writes a protective lease before spawning, creates a process group and waits for inherited background members after the foreground command exits.

A process that explicitly escapes the group cannot be tracked by this mechanism. Use pins for daemonized work. A crash leaves the lease in place, including older-format leases; `leases` shows retained records. `release-lease ID` is an explicit user/agent statement that the protected work has ended, not automatic proof of inactivity.

Broad recognized-process deferral remains for categories other than worktrees. A regular worktree instead uses its own open-file/working-directory evidence, scoped reservations and global `aigc run` reservations, so an agent elsewhere does not indefinitely block old-tree collection. OS inspection cannot establish that every paused agent has finished. No private agent session database is modified.

## Accounting, limits and migration

- Schema 2 adds evidence and a filesystem union measurement. Hardlinks and nested resources are counted once in that union; APFS clones, snapshots and VM internals prevent an exact universal reclaimable number.
- Tree walks stop at 500,000 entries per resource; union measurement stops at one million. Deep adapters inspect at most 250 discovered projects. Incomplete project discovery disables new reference-dependent cleanup.
- Each pass attempts at most ten initially eligible resources. Every action gets fresh inventory/policy and identity checks; changed resources are skipped. Full revalidation is intentionally conservative and can take time on large roots.
- Native maintenance and recollection have a default seven-day cooldown. History separates `removed`, `maintained`, `no_op` and `skipped`, estimated bytes, optional native reclaimed bytes and observed host free-space change.
- `deep-inventory=false` disables the new discovery layer, including new adapters.
- Existing config loads with defaults for new fields. Policy version 7 invalidates old cached reports. Regular worktree eligibility uses filesystem/commit age on the first scan; other filesystem observations still restart when identities change. The executable does not migrate or delete user data.

The release does not implement exact agent-session attribution, automatic mutable-environment sharing, full dynamic build evaluation or exact APFS extent accounting. Those limitations are visible rather than replaced with guessed ownership or fabricated usage statistics.

## Platform boundaries

See the README platform section for Windows activity limitations and persistent reservations. Xcode DerivedData handling is macOS-only. Windows junctions/reparse points prevent removal, and sizes are logical bytes. Ubuntu needs lsof, ps and a systemd user session for scheduled collection.
