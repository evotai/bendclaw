#!/usr/bin/env sh
# Usage: curl -fsSL https://evot.ai/install | sh
#
# POSIX sh compatible — do NOT use bash-specific syntax (e.g. [[ ]], pipefail,
# arrays, process substitution). This script is piped to 'sh' which may be
# dash on Ubuntu/WSL.
set -e

REPO="evotai/evot"
BINARY="evot"
INSTALL_DIR="${EVOT_INSTALL_DIR:-$HOME/.evotai/bin}"

# --- Colors & helpers ---

RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[0;33m'
NC='\033[0m'

info()  { printf "${BLUE}%s${NC}\n" "$*"; }
ok()    { printf "${GREEN}%s${NC}\n" "$*"; }
warn()  { printf "${YELLOW}%s${NC}\n" "$*"; }
error() { printf "${RED}%s${NC}\n" "$*" >&2; exit 1; }

# --- Download abstraction (curl with wget fallback) ---

DOWNLOADER=""
if command -v curl > /dev/null 2>&1; then
  DOWNLOADER="curl"
elif command -v wget > /dev/null 2>&1; then
  DOWNLOADER="wget"
else
  error "Either curl or wget is required but neither is installed"
fi

download() {
  _url="$1"; _output="$2"
  if [ "$DOWNLOADER" = "curl" ]; then
    curl -fsSL -o "$_output" "$_url"
  else
    wget -q -O "$_output" "$_url"
  fi
}

fetch() {
  _url="$1"
  if [ "$DOWNLOADER" = "curl" ]; then
    curl -fsSL "$_url"
  else
    wget -qO- "$_url"
  fi
}

# --- Platform detection ---

OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
  Darwin) os="darwin" ;;
  Linux)  os="linux" ;;
  *)      error "Unsupported OS: $OS" ;;
esac

case "$ARCH" in
  x86_64|amd64)  arch="x86_64" ;;
  aarch64|arm64)  arch="aarch64" ;;
  *)              error "Unsupported architecture: $ARCH" ;;
esac

case "${os}-${arch}" in
  linux-x86_64)   TARGET="x86_64-unknown-linux-gnu" ;;
  linux-aarch64)  TARGET="aarch64-unknown-linux-gnu" ;;
  darwin-x86_64)  TARGET="x86_64-apple-darwin" ;;
  darwin-aarch64) TARGET="aarch64-apple-darwin" ;;
esac

# --- Version resolution ---

if [ -n "${EVOT_INSTALL_VERSION:-}" ]; then
  TAG="$EVOT_INSTALL_VERSION"
else
  TAG="$(fetch "https://api.github.com/repos/${REPO}/releases/latest" \
    | sed -n 's/.*"tag_name":[[:space:]]*"\([^"]*\)".*/\1/p')"
fi

[ -z "$TAG" ] && error "Failed to determine latest version. GitHub API rate limit?"
VERSION="${TAG#v}"

ASSET="${BINARY}-v${VERSION}-${TARGET}.tar.gz"
URL="https://github.com/${REPO}/releases/download/${TAG}/${ASSET}"
SHA_URL="${URL}.sha256"

# --- Download & verify ---

info "Installing ${BINARY} v${VERSION} for ${TARGET}..."

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

download "$URL" "$TMP/$ASSET"

# SHA256 verification (best-effort: skip if .sha256 file not published)
EXPECTED_SHA="$(fetch "$SHA_URL" 2>/dev/null || true)"
if [ -n "$EXPECTED_SHA" ]; then
  EXPECTED_SHA="$(echo "$EXPECTED_SHA" | awk '{print $1}')"
  if command -v sha256sum > /dev/null 2>&1; then
    ACTUAL_SHA="$(sha256sum "$TMP/$ASSET" | awk '{print $1}')"
  elif command -v shasum > /dev/null 2>&1; then
    ACTUAL_SHA="$(shasum -a 256 "$TMP/$ASSET" | awk '{print $1}')"
  else
    ACTUAL_SHA=""
  fi
  if [ -n "$ACTUAL_SHA" ] && [ "$ACTUAL_SHA" != "$EXPECTED_SHA" ]; then
    error "Checksum verification failed (expected $EXPECTED_SHA, got $ACTUAL_SHA)"
  fi
  info "Checksum verified"
fi

# --- Install ---

tar -xzf "$TMP/$ASSET" -C "$TMP"

mkdir -p "$INSTALL_DIR"
# On Linux, a running binary cannot be overwritten in place (ETXTBSY).
# Remove first so the kernel unlinks the old inode while the running process
# keeps its file descriptor; then copy the new binary to a fresh inode.
rm -f "$INSTALL_DIR/$BINARY"
cp "$TMP/bin/$BINARY" "$INSTALL_DIR/$BINARY"
chmod +x "$INSTALL_DIR/$BINARY"

# Copy lib files (napi bindings)
LIB_DIR="${INSTALL_DIR%/bin}/lib"
if [ -d "$TMP/lib" ]; then
  mkdir -p "$LIB_DIR"
  cp "$TMP"/lib/* "$LIB_DIR/"
fi

# Remove macOS quarantine/provenance attributes
if [ "$os" = "darwin" ]; then
  xattr -cr "$INSTALL_DIR/$BINARY" 2>/dev/null || true
  for f in "$LIB_DIR"/*.node; do
    [ -f "$f" ] && xattr -cr "$f" 2>/dev/null || true
  done
fi

ok "  ✓ Installed ${BINARY} to ${INSTALL_DIR}/${BINARY}"

# --- PATH guidance ---

case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *)
    SHELL_NAME="$(basename "${SHELL:-/bin/sh}")"
    case "$SHELL_NAME" in
      zsh)  RC="$HOME/.zshrc" ;;
      bash) RC="$HOME/.bashrc" ;;
      fish) RC="$HOME/.config/fish/config.fish" ;;
      *)    RC="$HOME/.profile" ;;
    esac

    warn "$INSTALL_DIR is not in your PATH. Run:"
    echo ""
    if [ "$SHELL_NAME" = "fish" ]; then
      echo "  set -Ux fish_user_paths $INSTALL_DIR \$fish_user_paths"
    else
      echo "  echo 'export PATH=\"$INSTALL_DIR:\$PATH\"' >> $RC"
      echo "  source $RC"
    fi
    echo ""
    ;;
esac
