#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

RPC_URL="${RPC_URL:-https://api.mainnet-beta.solana.com}"
DB_PATH="${DB_PATH:-${ROOT_DIR}/GeoLite2-City_20260210/GeoLite2-City.mmdb}"
OUTPUT_PATH="${OUTPUT_PATH:-${ROOT_DIR}/procedure/data/leader_geo_map.bin}"

if [[ ! -f "${DB_PATH}" ]]; then
  echo "GeoLite2 database not found: ${DB_PATH}" >&2
  echo "Set DB_PATH to the GeoLite2-City.mmdb location." >&2
  exit 1
fi

echo "Rebuilding leader geo map"
echo "rpc_url=${RPC_URL}"
echo "db_path=${DB_PATH}"
echo "output_path=${OUTPUT_PATH}"

(
  cd "${ROOT_DIR}"
  cargo run -p geo-mapper -- \
    --rpc-url "${RPC_URL}" \
    --db "${DB_PATH}" \
    --output "${OUTPUT_PATH}"
)
