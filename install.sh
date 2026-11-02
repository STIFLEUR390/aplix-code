#!/bin/sh
# Install or update `apc` from a GitHub release. Running it again is the update path.
#
#   curl -fsSL https://raw.githubusercontent.com/STIFLEUR390/aplix-code/main/install.sh | sh
#   APC_VERSION=v1.2.0 sh install.sh          # pin a tag instead of the latest release
#   APC_INSTALL_DIR=/usr/local/bin sh install.sh
#
# Downloads the archive published by .github/workflows/release.yml, verifies its SHA-256, and puts
# the binary in APC_INSTALL_DIR (default ~/.local/bin). No sudo, no system dependency.
set -eu

REPO="STIFLEUR390/aplix-code"
INSTALL_DIR="${APC_INSTALL_DIR:-$HOME/.local/bin}"
VERSION="${APC_VERSION:-latest}"

say() { printf '%s\n' "$*"; }
die() { say "apc: $*" >&2; exit 1; }

# Asset names come from the release workflow: apc-<runner.os>-<runner.arch>.tar.gz
os=$(uname -s)
arch=$(uname -m)
case "$os" in
  Linux) os=Linux ;;
  Darwin) os=macOS ;;
  *)
    die "unsupported OS '$os' — download the archive from https://github.com/$REPO/releases"
    ;;
esac
case "$arch" in
  x86_64 | amd64) arch=X64 ;;
  arm64 | aarch64) arch=ARM64 ;;
  *) die "unsupported architecture '$arch'" ;;
esac

# The checksum file is named after the package, not the archive: <name>.sha256 covers the
# .tar.gz and the .zip (release.yml: sha256sum "$name".*).
stem="apc-$os-$arch"
asset="$stem.tar.gz"
if [ "$VERSION" = latest ]; then
  base="https://github.com/$REPO/releases/latest/download"
else
  base="https://github.com/$REPO/releases/download/$VERSION"
fi

# Report what is being replaced, so an update is visible rather than silent.
if [ -x "$INSTALL_DIR/apc" ]; then
  say "updating $("$INSTALL_DIR/apc" --version 2>/dev/null | head -n 1 || echo "apc (unknown version)")"
else
  say "installing apc"
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT INT TERM

say "downloading $asset ($VERSION)"
curl -fL --progress-bar "$base/$asset" -o "$tmp/$asset" ||
  die "no $asset in $VERSION releases — see https://github.com/$REPO/releases"
curl -fsSL "$base/$stem.sha256" -o "$tmp/$stem.sha256" ||
  die "missing checksum for $asset"

expected=$(cut -d' ' -f1 "$tmp/$stem.sha256")
if command -v sha256sum >/dev/null 2>&1; then
  actual=$(sha256sum "$tmp/$asset" | cut -d' ' -f1)
else
  actual=$(shasum -a 256 "$tmp/$asset" | cut -d' ' -f1)
fi
[ "$actual" = "$expected" ] || die "checksum mismatch for $asset — refusing to install"

tar xzf "$tmp/$asset" -C "$tmp"
binary=$(find "$tmp" -maxdepth 2 -type f -name apc | head -n 1)
[ -n "$binary" ] || die "$asset contains no apc binary"

mkdir -p "$INSTALL_DIR"
install -m 755 "$binary" "$INSTALL_DIR/apc"

say ""
"$INSTALL_DIR/apc" --version
case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *)
    say ""
    say "$INSTALL_DIR is not in your PATH. Add it to your shell profile:"
    say "  export PATH=\"$INSTALL_DIR:\$PATH\""
    ;;
esac
say ""
say "next: apc init && apc config"
