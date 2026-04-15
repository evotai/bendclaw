#!/usr/bin/env bash
set -euo pipefail

REPO="evotai/evot"
BINARY="evot"
INSTALL_DIR="${EVOT_INSTALL_DIR:-$HOME/.evotai/bin}"

# Detect platform
OS="$(uname -s)"
ARCH="$(uname -m)"

case "${OS}-${ARCH}" in
  Linux-x86_64)   TARGET="x86_64-unknown-linux-gnu" ;;
  Linux-aarch64)  TARGET="aarch64-unknown-linux-gnu" ;;
  Darwin-x86_64)  TARGET="aarch64-apple-darwin" ;;  # Rosetta 2 compatible
  Darwin-arm64)   TARGET="aarch64-apple-darwin" ;;
  *)
    echo "Unsupported platform: ${OS}-${ARCH}" >&2
    exit 1
    ;;
esac

# Get release tag: prefer EVOT_INSTALL_VERSION env var, fallback to latest
if [ -n "${EVOT_INSTALL_VERSION:-}" ]; then
  TAG="${EVOT_INSTALL_VERSION}"
else
  TAG="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" | grep '"tag_name"' | cut -d'"' -f4)"
fi
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

# Copy lib files (napi bindings)
LIB_DIR="${INSTALL_DIR%/bin}/lib"
if [ -d "${TMP}/lib" ]; then
  mkdir -p "${LIB_DIR}"
  cp "${TMP}"/lib/* "${LIB_DIR}/"
fi

# Remove macOS quarantine/provenance attributes to prevent Gatekeeper from killing the binary
if [ "${OS}" = "Darwin" ]; then
  xattr -cr "${INSTALL_DIR}/${BINARY}" 2>/dev/null || true
  for f in "${LIB_DIR}"/*.node 2>/dev/null; do
    xattr -cr "$f" 2>/dev/null || true
  done
fi

echo ""
echo "  ✓ Installed ${BINARY} to ${INSTALL_DIR}/${BINARY}"
echo ""

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
