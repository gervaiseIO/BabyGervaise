#!/usr/bin/env bash
set -euo pipefail

API_KEY="${GEMINI_API_KEY:-${GOOGLE_API_KEY:-${1:-}}}"
API_ROOT="${GEMINI_API_ROOT:-https://generativelanguage.googleapis.com/v1beta}"

if [[ -z "$API_KEY" ]]; then
  cat >&2 <<'EOF'
usage: scripts/list-gemini-models.sh [api-key]

Provide a Gemini API key as the first argument, or set GEMINI_API_KEY / GOOGLE_API_KEY.
EOF
  exit 1
fi

if ! command -v curl >/dev/null 2>&1; then
  echo "curl is required" >&2
  exit 1
fi

if ! command -v jq >/dev/null 2>&1; then
  echo "jq is required" >&2
  exit 1
fi

output="$(
  curl -fsS "${API_ROOT}/models?key=${API_KEY}" \
    | jq -r '
        .models // []
        | sort_by(.name)
        | (["MODEL", "SUPPORTED METHODS"], ["-----", "-----------------"]),
          (.[] | [
            .name,
            ((.supportedGenerationMethods // []) | sort | join(", "))
          ])
        | @tsv
      '
)"

if command -v column >/dev/null 2>&1; then
  printf '%s\n' "$output" | column -t -s $'\t'
else
  printf '%s\n' "$output"
fi
