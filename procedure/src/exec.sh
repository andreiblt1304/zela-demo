#!/usr/bin/env bash
set -euo pipefail

source ../.env

: "${JWT:?JWT is required}"
: "${PROC:?PROC is required}"
: "${HASH:?HASH is required}"

curl -sS \
  -H "Authorization: Bearer ${JWT}" \
  -H "Content-Type: application/json" \
  --data "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"zela.${PROC}#${HASH}\",\"params\":{}}" \
  https://executor.zela.io
