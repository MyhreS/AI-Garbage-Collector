# Releases

1. Update the package version and lockfile. Review the README coverage table against implementation.
2. Run formatting, Clippy, tests, and a release build on macOS.
3. Exercise status and dry-run with isolated state. Test destructive changes only on owned fixtures.
4. Commit and push the version tag (for example `v0.1.0`). The release workflow builds `aarch64-apple-darwin` and `x86_64-apple-darwin`, creates archives, and publishes SHA256SUMS.
5. Verify both assets and test the installer. The installer starts a per-user collector; use a disposable test account for install/uninstall testing.

Release builds currently have no Developer ID signing or notarization. Do not describe checksum verification as notarization. Add protected signing secrets and a notarization workflow before making that claim.

Build output belongs to the person or agent running the build. Reuse `target` during development. Before finishing a task, preserve requested executables outside `target`, then remove owned build/scratch output if no active build needs it. Keep shared Cargo caches available to other projects.
