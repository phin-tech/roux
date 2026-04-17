#!/usr/bin/env bash
# One-shot bootstrap: read .env.signing via `op run`, then push each value
# into GitHub Actions secrets via `gh secret set`. Idempotent — rerun after
# rotating anything in 1Password.
set -euo pipefail

VARS=(
  APPLE_SIGNING_IDENTITY
  APPLE_CERTIFICATE
  APPLE_CERTIFICATE_PASSWORD
  APPLE_ID
  APPLE_PASSWORD
  APPLE_TEAM_ID
  TAURI_SIGNING_PRIVATE_KEY
  TAURI_SIGNING_PRIVATE_KEY_PASSWORD
)

command -v op >/dev/null || { echo "op CLI required" >&2; exit 1; }
command -v gh >/dev/null || { echo "gh CLI required" >&2; exit 1; }
[ -f .env.signing ] || { echo ".env.signing not found in $(pwd)" >&2; exit 1; }

missing=()
for v in "${VARS[@]}"; do
  if ! grep -qE "^${v}=" .env.signing; then
    missing+=("$v")
  fi
done
if (( ${#missing[@]} > 0 )); then
  echo "Missing from .env.signing (uncomment or add these lines):" >&2
  printf '  - %s\n' "${missing[@]}" >&2
  exit 1
fi

# op run expands op:// refs into real env values for the child process.
# --no-masking keeps stderr diagnostics readable on failure; the actual
# secret values are still piped directly to `gh secret set --body -`.
op run --env-file=.env.signing --no-masking -- bash -c '
  set -euo pipefail
  for v in '"${VARS[*]}"'; do
    val="${!v-}"
    if [ -z "$val" ]; then
      echo "Skipping ${v} (empty after op resolution)" >&2
      continue
    fi
    printf "%s" "$val" | gh secret set "$v" --body -
    echo "Set ${v}"
  done
'
