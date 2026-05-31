#!/usr/bin/env sh
set -eu

REPO="${ROUX_REPO:-phin-tech/roux}"
VERSION="${ROUX_VERSION:-latest}"
INSTALL_DIR="${ROUX_INSTALL_DIR:-$HOME/.local/bin}"

need() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "Missing required command: $1" >&2
    exit 1
  fi
}

need curl
need tar

OS="$(uname -s)"
ARCH="$(uname -m)"
case "${OS}:${ARCH}" in
  Darwin:arm64) TARGET="aarch64-apple-darwin" ;;
  Darwin:x86_64) TARGET="x86_64-apple-darwin" ;;
  Linux:x86_64) TARGET="x86_64-unknown-linux-gnu" ;;
  *)
    echo "Unsupported platform: ${OS} ${ARCH}" >&2
    exit 1
    ;;
esac

ASSET="roux-${TARGET}.tar.gz"
if [ "$VERSION" = "latest" ]; then
  BASE_URL="https://github.com/${REPO}/releases/latest/download"
else
  case "$VERSION" in
    v*) TAG="$VERSION" ;;
    *) TAG="v${VERSION}" ;;
  esac
  BASE_URL="https://github.com/${REPO}/releases/download/${TAG}"
fi

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

curl -fsSL "${BASE_URL}/${ASSET}" -o "${TMP_DIR}/${ASSET}"
if curl -fsSL "${BASE_URL}/${ASSET}.sha256" -o "${TMP_DIR}/${ASSET}.sha256"; then
  if command -v sha256sum >/dev/null 2>&1; then
    (cd "$TMP_DIR" && sha256sum -c "${ASSET}.sha256")
  elif command -v shasum >/dev/null 2>&1; then
    (cd "$TMP_DIR" && shasum -a 256 -c "${ASSET}.sha256")
  else
    echo "No sha256 verifier found; skipping checksum verification." >&2
  fi
else
  echo "Checksum not found; skipping verification." >&2
fi

tar -xzf "${TMP_DIR}/${ASSET}" -C "$TMP_DIR"
ROUX_BIN="$(find "$TMP_DIR" -type f -name roux | head -n 1)"
if [ -z "$ROUX_BIN" ]; then
  echo "roux binary not found in ${ASSET}" >&2
  exit 1
fi

mkdir -p "$INSTALL_DIR"
install -m 0755 "$ROUX_BIN" "${INSTALL_DIR}/roux"

echo "Installed roux to ${INSTALL_DIR}/roux"
