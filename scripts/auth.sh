#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ENV_FILE="${ENV_FILE:-${ROOT_DIR}/.env}"

if [[ -f "${ENV_FILE}" ]]; then
  # shellcheck disable=SC1090
  source "${ENV_FILE}"
fi

: "${ID:?ID is required (client id)}"
: "${SECRET:?SECRET is required (client secret)}"

token_response="$(
  curl -sS \
    -u "${ID}:${SECRET}" \
    -d 'grant_type=client_credentials' \
    -d 'scope=zela-executor:call' \
    'https://auth.zela.io/realms/zela/protocol/openid-connect/token'
)"

new_jwt="$(printf '%s' "${token_response}" | jq -r '.access_token // empty')"
if [[ -z "${new_jwt}" ]]; then
  echo "failed to retrieve access_token from auth response" >&2
  echo "${token_response}" >&2
  exit 1
fi

mkdir -p "$(dirname "${ENV_FILE}")"
if [[ -f "${ENV_FILE}" ]]; then
  tmp_file="$(mktemp)"
  awk -v jwt="${new_jwt}" '
    BEGIN { updated = 0 }
    /^JWT=/ { print "JWT=" jwt; updated = 1; next }
    { print }
    END { if (updated == 0) print "JWT=" jwt }
  ' "${ENV_FILE}" > "${tmp_file}"
  mv "${tmp_file}" "${ENV_FILE}"
else
  printf 'JWT=%s\n' "${new_jwt}" > "${ENV_FILE}"
fi

export JWT="${new_jwt}"
echo "updated JWT in ${ENV_FILE}"
