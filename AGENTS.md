# AI Garbage Collector

Read README.md before changing cleanup behavior. This is a standalone local macOS, Windows and Ubuntu tool; there is no cloud service.

- Treat uncertain ownership or activity as protected, never as permission to delete.
- Use native Git and device-management commands for their resources.
- Preserve user work, pinned resources and other tasks' files.
- Do not add or run tests unless the user explicitly requests them. Do not trigger tests through CI or release workflows.
- Keep the README coverage table and CLI help accurate. Do not advertise report-only categories as automatic cleanup.
- Run cargo fmt --check, cargo clippy --locked --lib --bin aigc -- -D warnings and cargo build --release --locked.
- Never exercise destructive cleanup against the developer's real worktrees, simulators or SDKs.
- Clean owned scratch/build output at task completion after preserving requested deliverables. Never sweep shared caches or another task's folders.
