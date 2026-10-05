#!/bin/sh
# Download a published release, verify its checksum, install locally and start collection.
set -eu
[ "$(id -u)" != 0 ] || { echo 'Run as your normal user, without sudo.' >&2; exit 1; }
case "$(uname -s)/$(uname -m)" in
  Darwin/arm64) target=aarch64-apple-darwin ;;
  Darwin/x86_64) target=x86_64-apple-darwin ;;
  Linux/x86_64)
    target=x86_64-unknown-linux-gnu
    command -v lsof >/dev/null || { echo 'Install prerequisites: sudo apt install git curl lsof' >&2; exit 1; }
    systemctl --user show-environment >/dev/null || { echo 'A logged-in systemd user session is required.' >&2; exit 1; }
    ;;
  *) echo 'Supported: macOS Apple Silicon/Intel, Ubuntu x86_64.' >&2; exit 1 ;;
esac
command -v git >/dev/null || { echo 'Git is required.' >&2; exit 1; }
version=${AIGC_VERSION:-latest}
base="https://github.com/MyhreS/AI-Garbage-Collector/releases"
if [ "$version" = latest ]; then base="$base/latest/download"; else base="$base/download/$version"; fi
asset="aigc-$target.tar.gz"
scratch=$(mktemp -d "${TMPDIR:-/tmp}/aigc-install.XXXXXX")
trap 'rm -rf "$scratch"' EXIT HUP INT TERM
curl --fail --location --proto '=https' --tlsv1.2 "$base/$asset" -o "$scratch/$asset"
curl --fail --location --proto '=https' --tlsv1.2 "$base/SHA256SUMS" -o "$scratch/SHA256SUMS"
(cd "$scratch" && awk -v asset="$asset" '$2 == asset {print}' SHA256SUMS > selected.sha256 && test -s selected.sha256 && { if command -v sha256sum >/dev/null; then sha256sum -c selected.sha256; else shasum -a 256 -c selected.sha256; fi; })
tar -xzf "$scratch/$asset" -C "$scratch" aigc
mkdir -p "$HOME/.local/bin"
install -m 755 "$scratch/aigc" "$HOME/.local/bin/aigc"
"$HOME/.local/bin/aigc" service install
printf '\nInstalled. The hourly collector is running. Regular linked worktrees idle for seven days can be force-removed, including local files, without recovery.\n'
printf 'Status: %s/.local/bin/aigc status\n' "$HOME"
printf 'If needed, add %s/.local/bin to your PATH.\n' "$HOME"
