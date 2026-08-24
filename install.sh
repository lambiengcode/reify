#!/bin/sh
# Install the latest reify release binary.
#
#   curl -fsSL https://raw.githubusercontent.com/lambiengcode/reify/main/install.sh | sh
#
# REIFY_INSTALL_DIR overrides the destination (default: ~/.local/bin).
set -eu

REPO="lambiengcode/reify"
BIN_DIR="${REIFY_INSTALL_DIR:-$HOME/.local/bin}"

os=$(uname -s)
arch=$(uname -m)
target=""
exe=""
case "$os" in
  Darwin)
    case "$arch" in
      arm64)  target="aarch64-apple-darwin" ;;
      x86_64) target="x86_64-apple-darwin" ;;
    esac ;;
  Linux)
    case "$arch" in
      aarch64 | arm64) target="aarch64-unknown-linux-gnu" ;;
      x86_64)          target="x86_64-unknown-linux-gnu" ;;
    esac ;;
  # Git Bash, MSYS2 and Cygwin are how a Windows developer runs `curl | sh`.
  MINGW* | MSYS* | CYGWIN*)
    case "$arch" in
      x86_64) target="x86_64-pc-windows-msvc"; exe=".exe" ;;
    esac ;;
esac
if [ -z "$target" ]; then
  echo "install.sh: no prebuilt binary for $os/$arch." >&2
  echo "Build from source instead: cargo install --git https://github.com/$REPO reify-cli" >&2
  exit 1
fi

tag=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" \
  | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -n1)
if [ -z "$tag" ]; then
  echo "install.sh: could not determine the latest release of $REPO" >&2
  exit 1
fi

name="reify-$tag-$target"
url="https://github.com/$REPO/releases/download/$tag/$name.tar.gz"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

echo "downloading $url"
curl -fsSL "$url" -o "$tmp/$name.tar.gz"

# Verify before unpacking, the same check `reify upgrade` makes. Every release
# publishes `<archive>.sha256`; a missing or mismatched one stops the install rather
# than running an unverified binary.
curl -fsSL "$url.sha256" -o "$tmp/$name.tar.gz.sha256"
expected=$(awk '{print $1}' "$tmp/$name.tar.gz.sha256")
if command -v shasum >/dev/null 2>&1; then
  actual=$(shasum -a 256 "$tmp/$name.tar.gz" | awk '{print $1}')
elif command -v sha256sum >/dev/null 2>&1; then
  actual=$(sha256sum "$tmp/$name.tar.gz" | awk '{print $1}')
else
  echo "install.sh: no shasum or sha256sum available to verify the download" >&2
  exit 1
fi
if [ -z "$expected" ] || [ "$expected" != "$actual" ]; then
  echo "install.sh: checksum mismatch for $name.tar.gz" >&2
  echo "  published:  $expected" >&2
  echo "  downloaded: $actual" >&2
  exit 1
fi

tar -xzf "$tmp/$name.tar.gz" -C "$tmp"
mkdir -p "$BIN_DIR"
install -m 0755 "$tmp/$name/reify$exe" "$BIN_DIR/reify$exe"

echo "installed reify $tag to $BIN_DIR/reify$exe"
case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *) echo "note: $BIN_DIR is not on your PATH" ;;
esac
