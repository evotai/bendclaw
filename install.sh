#!/usr/bin/env bash
set -euo pipefail

REPO="evotai/bendclaw"
BINARY="bendclaw"
INSTALL_DIR="${BENDCLAW_INSTALL_DIR:-$HOME/.evotai/bin}"

# Detect platform
OS="$(uname -s)"
ARCH="$(uname -m)"

case "${OS}-${ARCH}" in
  Linux-x86_64)   TARGET="x86_64-unknown-linux-gnu" ;;
  Linux-aarch64)  TARGET="aarch64-unknown-linux-gnu" ;;
  Darwin-x86_64)  TARGET="x86_64-apple-darwin" ;;
  Darwin-arm64)   TARGET="aarch64-apple-darwin" ;;
  *)
    echo "Unsupported platform: ${OS}-${ARCH}" >&2
    exit 1
    ;;
esac

# Get latest release tag
TAG="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" | grep '"tag_name"' | cut -d'"' -f4)"
VERSION="${TAG#v}"

ASSET="${BINARY}-v${VERSION}-${TARGET}.tar.gz"
URL="https://github.com/${REPO}/releases/download/${TAG}/${ASSET}"

echo "Installing ${BINARY} ${VERSION} for ${TARGET}..."

TMP="$(mktemp -d)"
trap 'rm -rf "${TMP}"' EXIT

curl -fsSL "${URL}" -o "${TMP}/${ASSET}"
tar -xzf "${TMP}/${ASSET}" -C "${TMP}"

mkdir -p "${INSTALL_DIR}"
cp "${TMP}/bin/${BINARY}" "${INSTALL_DIR}/${BINARY}"
chmod +x "${INSTALL_DIR}/${BINARY}"

echo "Installed ${BINARY} to ${INSTALL_DIR}/${BINARY}"

if ! echo "${PATH}" | tr ':' '\n' | grep -qx "${INSTALL_DIR}"; then
  SHELL_NAME="$(basename "${SHELL:-/bin/bash}")"
  case "${SHELL_NAME}" in
    zsh)  RC="$HOME/.zshrc" ;;
    bash) RC="$HOME/.bashrc" ;;
    fish) RC="$HOME/.config/fish/config.fish" ;;
    *)    RC="$HOME/.profile" ;;
  esac

  echo ""
  echo "${INSTALL_DIR} is not in your PATH. Run:"
  echo ""
  if [ "${SHELL_NAME}" = "fish" ]; then
    echo "  set -Ux fish_user_paths ${INSTALL_DIR} \$fish_user_paths"
  else
    echo "  echo 'export PATH=\"${INSTALL_DIR}:\$PATH\"' >> ${RC}"
    echo "  source ${RC}"
  fi
  echo ""
fi
