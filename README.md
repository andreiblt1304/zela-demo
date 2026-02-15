# Zela Interview Assignment: Leader Routing

## What this procedure does

This procedure answers:

> If called right now, which server region should handle the request to be closest (coarsely) to the current Solana leader?

It returns:

- `slot`: current Solana slot
- `leader`: validator identity pubkey that is leader for that slot
- `leader_geo`: coarse geo label for the leader (`EU`, `NA`, `APAC`, `ME`, or `UNKNOWN`)
- `closest_region`: one of `Dubai | Frankfurt | NewYork | Tokyo`

## How leader and region are derived

1. Fetch current slot from Solana RPC (`getSlot` via `RpcClient::get_slot`).
2. Fetch epoch schedule (`getEpochSchedule`) and compute `slot_index` within epoch.
3. Fetch leader schedule for the slot (`getLeaderSchedule`) and resolve the leader whose index list contains `slot_index`.
4. Look up leader pubkey in a bundled static map (`leader_pubkey -> leader_geo`).
5. Map `leader_geo` to a Zela region with deterministic rules:
   - `EU` (+ common EU country codes) -> `Frankfurt`
   - `ME` (+ common Middle East country codes) -> `Dubai`
   - `NA` (+ `US`, `CA`, `MX`) -> `NewYork`
   - `APAC` (+ common APAC country codes) -> `Tokyo`
6. If leader geo is unknown:
   - return `leader_geo = "UNKNOWN"`
   - choose `closest_region` using a deterministic hash fallback on leader pubkey

This fallback avoids random behavior and prevents flapping for the same leader.

### Deterministic rule table

| leader_geo input | closest_region |
| --- | --- |
| `EU` (or EU country code) | `Frankfurt` |
| `ME` (or ME country code) | `Dubai` |
| `NA` (or `US`/`CA`/`MX`) | `NewYork` |
| `APAC` (or APAC country code) | `Tokyo` |
| `UNKNOWN` / unmapped | deterministic hash fallback by leader pubkey |

## Build locally

```bash
cargo check
cargo test
```

## Deploy on Zela

1. Push this repository to GitHub.
2. In Zela Dashboard, create a source:
   - repository
   - branch
   - Cargo package: `procedure`
3. Wait for Builder status `Success`.
4. Use OAuth2 `client_credentials` flow to obtain a JWT:

```bash
curl \
  --header 'Authorization: Basic base64($key_client_id:$key_secret)' \
  --data 'grant_type=client%5Fcredentials' \
  --data 'scope=zela%2Dexecutor%3Acall' \
  'https://auth.zela.io/realms/zela/protocol/openid-connect/token'
```

5. Call executor:

```bash
curl \
  --header "authorization: Bearer $jwt" \
  --header 'Content-Type: application/json' \
  --data '{
    "jsonrpc":"2.0",
    "id":1,
    "method":"zela.<PROCEDURE_NAME>#COMMIT_HASH",
    "params": {}
  }' \
  'https://executor.zela.io'
```

## Real sample response from executor

```json
{
  "jsonrpc":"2.0",
  "id":1,
  "result":{
    "closest_region":"Frankfurt",
    "leader":"JupmVLmA8RoyTUbTMMuTtoPWHEiNQobxgTeGTrPNkzT",
    "leader_geo":"UNKNOWN",
    "slot":400403429
  }
}
```

## Sample error response

If required Solana data cannot be fetched, the procedure returns a structured error with `stage` and `details`:

```json
{
  "jsonrpc":"2.0",
  "id":1,
  "error":{
    "code":500,
    "message":"leader routing procedure failed",
    "data":{
      "stage":"get_leader_schedule",
      "details":"failed to fetch leader schedule for slot 400403429: <rpc error>"
    }
  }
}
```

## Geo data notes

- The bundled map is now a compact binary file at `procedure/data/leader_geo_map.bin`.
- A sidecar freshness/traceability file is generated next to it: `procedure/data/leader_geo_map.meta.json`.
- Record layout is fixed-size: `[leader_pubkey_32_bytes][geo_bucket_1_byte]` (33 bytes per leader).
- The `geo-mapper` crate regenerates this file by fetching `getClusterNodes` from Solana RPC,
  deriving `leader_pubkey -> preferred_ip`, and mapping IPs to coarse geo buckets via GeoLite2 City.
- Reproducible pipeline command:

```bash
./scripts/rebuild-leader-geo-map.sh
```

- The pipeline prints deterministic generation stats:
  - `total_leaders`
  - `mapped_leaders`
  - `unknown_leaders`
  - `unknown_rate`
  - `output_bytes`
- Metadata file fields include:
  - `generated_at_unix_secs`
  - `rpc_url`, `rpc_slot`
  - `db_path`, `mmdb_sha256`
  - `record_size_bytes`, `map_size_bytes`, `map_sha256`
  - mapping totals and unknown rate
- Example:

```bash
cargo run -p geo-mapper -- \
  --rpc-url https://api.mainnet-beta.solana.com \
  --db GeoLite2-City_20260210/GeoLite2-City.mmdb \
  --output procedure/data/leader_geo_map.bin
```

- No runtime external geo API calls are needed.

### Why this meets geo constraints

Runtime only calls Solana RPC methods (`getSlot`, `getEpochSchedule`, `getLeaderSchedule`) and does not call any external geo HTTP APIs. Geo resolution is done from a bundled compact binary map (`33` bytes per leader record), generated offline by `geo-mapper` from GeoLite2. This keeps the procedure artifact small (order of magnitude around the assignment target), while still providing deterministic region routing and a stable fallback for unmapped leaders.

## Short feedback for Zela

- What was confusing/missing:
  - Public examples for a no-input `CustomProcedure` that uses `RpcClient::get_leader_schedule`.
  - Clear statement of preferred crate target layout (`lib`/`cdylib`) for interview submissions.
  - Zela has examples on how to auth and deploy for the `hello_world` example crate. This can be configured to be available for all custom procedures.
- Improvement ideas:
  - Add an official template repo with:
    - minimal `CustomProcedure` scaffold
    - CI check for procedure exports
    - one end-to-end executor call example
