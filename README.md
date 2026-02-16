# Zela Interview Assignment: Leader Routing


## Return value
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
| `UNKNOWN` / unmapped | deterministic hash fallback (`fnv1a64 % 4`) by leader pubkey |

`fnv1a64 % 4` is a small deterministic fallback hash over leader pubkey bytes.
- Starts from fixed 64-bit offset basis: `0xcbf29ce484222325`
- For each byte: XOR first, then multiply by fixed prime
- `fnv` = Fowler-Noll-Vo
- `1a` = FNV-1a variant
- `64` = 64-bit output (`u64`)

## Geo data notes

- The bundled map is now a compact binary file at `procedure/data/leader_geo_map.bin`.
- A sidecar freshness/traceability file is generated next to it: `procedure/data/leader_geo_map.meta.json`.
- Record layout is fixed-size: `[leader_pubkey_32_bytes][geo_bucket_1_byte]` (33 bytes per leader).
- The `geo-mapper` crate regenerates this file by fetching `getClusterNodes` from Solana RPC,
  deriving `validator_pubkey -> preferred_ip`, and mapping IPs to coarse geo buckets via GeoLite2 City.

Reproducible pipeline command:
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

- No runtime external geo API calls are needed.

### Why this meets geo constraints

Runtime only calls Solana RPC methods (`getSlot`, `getEpochSchedule`, `getLeaderSchedule`) and does not call any external geo HTTP APIs. Geo resolution is done from a bundled compact binary map (`33` bytes per leader record), generated offline by `geo-mapper` from GeoLite2. This keeps the procedure artifact small (order of magnitude around the assignment target), while still providing deterministic region routing and a stable fallback for unmapped leaders.

## Script-based quickstart (from repo root)

```bash
./scripts/auth.sh
./scripts/exec.sh
```

> `scripts/exec.sh` currently expects to run from repo root (`source .env`). You can use the `.example.env` file to see what env vars are used.

## Real sample response from executor

```json
{
  "jsonrpc":"2.0",
  "id":1,
  "result":{
    "closest_region":"Frankfurt",
    "leader":"JupmVLmA8RoyTUbTMMuTtoPWHEiNQobxgTeGTrPNkzT",
    "leader_geo":"EU",
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

Feedback can be found in the designated [file](FEEDBACK.md).
