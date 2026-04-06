#!/usr/bin/env bash
set -euo pipefail

if [[ $# -eq 0 ]]; then
  echo "usage: $0 <command> [args...]" >&2
  exit 64
fi

missing=()

if [[ -z "${APPLE_SIGNING_IDENTITY:-}" ]]; then
  missing+=("APPLE_SIGNING_IDENTITY")
fi

has_apple_id_auth=false
if [[ -n "${APPLE_ID:-}" && -n "${APPLE_PASSWORD:-}" && -n "${APPLE_TEAM_ID:-}" ]]; then
  has_apple_id_auth=true
fi

has_api_auth=false
if [[ -n "${APPLE_API_KEY:-}" && -n "${APPLE_API_ISSUER:-}" ]]; then
  if [[ -n "${APPLE_API_KEY_PATH:-}" || -n "${APPLE_API_KEY_CONTENT:-}" ]]; then
    has_api_auth=true
  fi
fi

if [[ "${has_apple_id_auth}" != true && "${has_api_auth}" != true ]]; then
  missing+=("APPLE_ID/APPLE_PASSWORD/APPLE_TEAM_ID or APPLE_API_KEY/APPLE_API_ISSUER/(APPLE_API_KEY_PATH or APPLE_API_KEY_CONTENT)")
fi

tmpdir=""
cleanup() {
  if [[ -n "${tmpdir}" && -d "${tmpdir}" ]]; then
    rm -rf "${tmpdir}"
  fi
}
trap cleanup EXIT

if [[ -n "${APPLE_API_KEY_CONTENT:-}" && -z "${APPLE_API_KEY_PATH:-}" ]]; then
  if [[ -z "${APPLE_API_KEY:-}" ]]; then
    missing+=("APPLE_API_KEY")
  else
    tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/roux-notary.XXXXXX")"
    export APPLE_API_KEY_PATH="${tmpdir}/AuthKey_${APPLE_API_KEY}.p8"
    printf '%s' "${APPLE_API_KEY_CONTENT}" > "${APPLE_API_KEY_PATH}"
    chmod 600 "${APPLE_API_KEY_PATH}"
  fi
fi

if (( ${#missing[@]} > 0 )); then
  printf 'Missing macOS signing environment variables:\n' >&2
  printf '  - %s\n' "${missing[@]}" >&2
  exit 1
fi

exec "$@"
