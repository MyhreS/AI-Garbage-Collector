# Builds and releases

## Ordinary builds

Pushes to `main` and pull requests call the shared **Build macOS binaries** workflow. It checks Rust formatting, lints production code, and builds Apple Silicon (`aarch64-apple-darwin`) and Intel (`x86_64-apple-darwin`) executables targeting macOS 13 or newer. It runs no tests or cleanup commands.

Each run uploads two archives containing the executable, README, detailed documentation and license. Download them from the run's Artifacts section; build artifacts expire after seven days. The same workflow can be started manually from the Actions tab. No signing secrets or external services are needed.

## Publish a release

1. Update `version` in Cargo.toml and run `cargo check` to update Cargo.lock. Commit and push the change.
2. Create and push a matching version tag:

   ```sh
   git tag v0.3.0
   git push origin v0.3.0
   ```

3. The **Release** workflow checks that the tag exists and matches the package version, then runs the shared build workflow.
4. Only after both builds succeed, it publishes both archives and `SHA256SUMS` to GitHub Releases. The terminal installer uses the latest published release.

To retry an existing tag, re-run its failed workflow or use **Actions → Release → Run workflow** and enter the tag. Publishing can be repeated: existing release assets are replaced. Use a new version for changed code; do not move a published tag.

Only the publishing job has repository write permission. Release files persist until the release is removed; temporary build artifacts expire after seven days. Builds use the lockfile. There is no test suite or test-only dependency.

Release binaries currently have no Developer ID signing or notarization. Checksums verify archive integrity against the release manifest; they do not replace publisher signing.

## Homebrew formula

The repository is also a custom Homebrew tap: `Formula/aigc.rb` selects the published binary and checksum for each Mac architecture. The formula template is `scripts/aigc.rb.in`.

After a successful release, **Update Homebrew formula** reads the latest published release and its checksum manifest, regenerates the formula, checks Ruby syntax, and commits changes to `main`. It uses the repository's built-in token; no separate tap repository or personal token is needed. Reading the latest release prevents an older release rerun from downgrading the formula.

Use **Actions → Update Homebrew formula → Run workflow** to refresh it manually, including after changing the template. If repository rules later block direct pushes to `main`, the update job will fail visibly and the formula update must be merged through the allowed process. The release binaries remain available.

A local update uses:

```sh
python3 scripts/update-homebrew.py --tag v0.3.0 --checksums /path/to/SHA256SUMS
ruby -c Formula/aigc.rb
```

The formula has no test block and the workflow runs no tests. A direct `brew install` does not start the service; the documented Brewfile installs and starts it in one command.

## Local build storage

Reuse `target` while building. At task completion, preserve requested executables outside `target`, then remove owned build/scratch output if no active build needs it. Keep shared Cargo caches available to other projects.
