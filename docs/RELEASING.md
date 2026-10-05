# Builds and releases

Every workflow is manual. Pushes, tags and pull requests do not start jobs. The shared build workflow can be called by a manually started build or release.

## Build

Run **Actions → Build → Run workflow**, or `gh workflow run ci.yml`.
Formatting, production Clippy checks and native release builds run without tests or cleanup commands. Executables are started with `--version` and `--help` only.

Assets include README, docs and license:

- `aigc-aarch64-apple-darwin.tar.gz`: macOS 13+, Apple Silicon
- `aigc-x86_64-apple-darwin.tar.gz`: macOS 13+, Intel
- `aigc-x86_64-unknown-linux-gnu.tar.gz`: Ubuntu 22.04+, x86_64
- `aigc-x86_64-pc-windows-msvc.zip`: Windows 10/11, x86_64

Build artifacts expire after seven days. Native runners build each platform; Ubuntu uses 22.04 to retain glibc 2.35 compatibility. Neither Apple nor Windows binaries are publisher-signed yet.

## Release

1. Update Cargo.toml and Cargo.lock, build and inspect changes, commit and push.
2. Create and push the matching version tag. A tag alone starts no job.
3. Manually start release: `gh workflow run release.yml -f tag=v0.6.0`.
4. The workflow verifies the tag/version, builds all four binaries, then publishes archives and `SHA256SUMS` to GitHub Releases.
5. After publication, manually run `gh workflow run homebrew.yml`. It reads the latest release, validates and commits the updated macOS formula to main.

Use a new version for changed code; never move a published tag. A failed run can be retried manually. Publishing an existing version replaces its assets, so only retry the same source tag.

The terminal installers download the latest release (or `AIGC_VERSION`) and verify the manifest checksum. Only publishing/formula jobs receive write permission. No signing secrets or additional service is needed. Homebrew remains macOS-only; Ubuntu uses the shell installer and Windows uses PowerShell.

## Local build storage

Reuse `target` while building. Preserve requested executables outside `target`, then remove owned scratch/build output at completion. Keep shared Cargo caches available to other tasks.
