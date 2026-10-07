# Changes

## 0.7.9

- Fix the Windows build after removing device cleanup.

## 0.7.8

- Stop inventorying and removing iOS simulators, simulator runtimes, Android AVDs and Android SDK packages. Reinstalling them is slow and they were being deleted while still wanted.

## 0.5.0

- Remove Docker inventory, image deletion and build-cache pruning, including CLI flags and settings.
- Remove Docker support claims from user and agent documentation.
- Discard obsolete settings when upgrading and invalidate earlier inventory snapshots.

## 0.2.0

- Add detailed resource inspection, explicit ownership and project requirements, process attribution, observed/native usage evidence and advisory duplicate reports.
- Count nested filesystem resources and hardlinks once in a bounded union measurement; retain uncertainty for APFS storage.
- Map Poetry and project-local Python environments, protect linked/shared/tool environments, and support explicit disposal of known generated environments.
- Discover configured package stores; add opt-in pip/pnpm/npm native maintenance with budgets and cooldowns. Report other manager-owned storage conservatively.
- Associate build output with projects, protect source-package checkouts, and register owned scratch/custom output with a retention deadline.
- Map retained simulators/AVDs and known project requirements to runtime/SDK resources, with opt-in native removal and supported dry-run previews.
- Add scoped process-group reservations and retain protection after a wrapper crash.
- Revalidate complete inventory before each action; record skips, maintenance and no-op outcomes separately.
- Include detailed adapter and agent documentation in release archives.

Report JSON is now schema 2. Old settings remain readable; cached reports refresh. New cleanup categories require explicit disposal registration. Report-only adapters and unresolved ownership remain protected. This release does not merge environments, sweep personal data, infer complete agent-session ownership.
