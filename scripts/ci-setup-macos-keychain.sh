#!/usr/bin/env bash
# Create a temporary keychain on a CI runner and import the Developer ID
# Application certificate from $APPLE_CERTIFICATE (base64-encoded .p12) so
# `codesign` can run unattended. Only meant for CI; local dev uses the
# developer's login keychain.
set -euo pipefail

: "${APPLE_CERTIFICATE:?base64-encoded .p12 required}"
: "${APPLE_CERTIFICATE_PASSWORD:?.p12 password required}"
: "${RUNNER_TEMP:?RUNNER_TEMP must be set (GitHub Actions runner var)}"

KEYCHAIN_NAME="roux-build.keychain"
KEYCHAIN_PASSWORD="$(openssl rand -base64 24)"
CERT_PATH="${RUNNER_TEMP}/cert.p12"

cleanup() { rm -f "${CERT_PATH}"; }
trap cleanup EXIT

printf '%s' "${APPLE_CERTIFICATE}" | base64 --decode > "${CERT_PATH}"

security create-keychain -p "${KEYCHAIN_PASSWORD}" "${KEYCHAIN_NAME}"
security set-keychain-settings -lut 21600 "${KEYCHAIN_NAME}"
security unlock-keychain -p "${KEYCHAIN_PASSWORD}" "${KEYCHAIN_NAME}"

# Prepend the build keychain to the user search list so codesign finds the identity.
EXISTING_KEYCHAINS=$(security list-keychains -d user | sed -E 's/^[[:space:]]*"?//; s/"?$//')
# shellcheck disable=SC2086
security list-keychains -d user -s "${KEYCHAIN_NAME}" ${EXISTING_KEYCHAINS}

security import "${CERT_PATH}" \
  -k "${KEYCHAIN_NAME}" \
  -P "${APPLE_CERTIFICATE_PASSWORD}" \
  -T /usr/bin/codesign \
  -T /usr/bin/security \
  -T /usr/bin/productbuild

security set-key-partition-list \
  -S apple-tool:,apple: \
  -s -k "${KEYCHAIN_PASSWORD}" "${KEYCHAIN_NAME}" >/dev/null

echo "Imported signing identities:"
security find-identity -v -p codesigning "${KEYCHAIN_NAME}"
