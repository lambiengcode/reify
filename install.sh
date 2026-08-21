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
tar -xzf "$tmp/$name.tar.gz" -C "$tmp"
mkdir -p "$BIN_DIR"
install -m 0755 "$tmp/$name/reify" "$BIN_DIR/reify"

echo "installed reify $tag to $BIN_DIR/reify"
case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *) echo "note: $BIN_DIR is not on your PATH" ;;
esac
