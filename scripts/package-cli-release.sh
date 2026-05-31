#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

MANIFEST_VERSION="$(python3 -c 'import re; print(re.search(r"(?m)^version = \"([^\"]+)\"", open("crates/roux-cli/Cargo.toml").read()).group(1))')"

if [ -n "${ROUX_CLI_VERSION:-}" ]; then
  VERSION="$ROUX_CLI_VERSION"
  EXPECTED_VERSION="${VERSION#v}"
  if [ "$MANIFEST_VERSION" != "$EXPECTED_VERSION" ]; then
    echo "roux-cli Cargo.toml version ${MANIFEST_VERSION} does not match requested CLI release ${VERSION}" >&2
    exit 1
  fi
else
  VERSION="v${MANIFEST_VERSION}"
fi

TARGET="${ROUX_CLI_TARGET:-$(rustc -vV | sed -n 's/^host: //p')}"
OUT_DIR="${ROUX_CLI_DIST_DIR:-target/cli-release}"
mkdir -p "$OUT_DIR"

if [ -n "${ROUX_CLI_TARGET:-}" ]; then
  cargo build -p roux-cli --release --bin roux --target "$TARGET"
  BIN="target/${TARGET}/release/roux"
else
  cargo build -p roux-cli --release --bin roux
  BIN="target/release/roux"
fi

if [ ! -f "$BIN" ] && [ -f "${BIN}.exe" ]; then
  BIN="${BIN}.exe"
fi
if [ ! -f "$BIN" ]; then
  echo "roux binary not found at ${BIN}" >&2
  exit 1
fi

WORK_DIR="$(mktemp -d "${OUT_DIR}/roux.XXXXXX")"
trap 'rm -rf "$WORK_DIR"' EXIT

PACKAGE_DIR="${WORK_DIR}/roux-${VERSION}-${TARGET}"
mkdir -p "$PACKAGE_DIR"
cp "$BIN" "$PACKAGE_DIR/roux"
chmod +x "$PACKAGE_DIR/roux"

cat > "${PACKAGE_DIR}/README.txt" <<EOF
Roux CLI ${VERSION}

Install:
  install -m 0755 roux ~/.local/bin/roux
EOF

ARCHIVE="${OUT_DIR}/roux-${TARGET}.tar.gz"
tar -C "$WORK_DIR" -czf "$ARCHIVE" "$(basename "$PACKAGE_DIR")"

if command -v sha256sum >/dev/null 2>&1; then
  (cd "$OUT_DIR" && sha256sum "$(basename "$ARCHIVE")" > "$(basename "$ARCHIVE").sha256")
else
  (cd "$OUT_DIR" && shasum -a 256 "$(basename "$ARCHIVE")" > "$(basename "$ARCHIVE").sha256")
fi

echo "$ARCHIVE"
echo "${ARCHIVE}.sha256"
