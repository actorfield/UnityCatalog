# Unity Catalog — Rust Server

A full Rust port of the [Unity Catalog](https://github.com/unitycatalog/unitycatalog) Java server.
Full API parity with the Java server, excluding the Iceberg REST catalog.

## Architecture

```
unitycatalog-rs/
├── crates/
│   ├── uc-errors/       ErrorCode enum, UcError, UC/Delta wire error shapes
│   ├── uc-types/        Privilege, UriScheme, TokenType, SecurableType
│   ├── uc-openapi/      Serde types from all.yaml + control.yaml + delta.yaml
│   ├── uc-db/           row structs, repositories, log-structured store
│   ├── uc-auth/         JWT (RS512) + Casbin RBAC
│   ├── uc-credentials/  AWS/Azure/GCP credential vending
│   ├── uc-api/          Axum routers — catalog, control, delta APIs
│   └── uc-server/       Binary: startup wiring, CLI args, serve
├── tests/python/        Pytest integration tests
├── scripts/
│   └── seed.py          Seeds sample data (unity catalog + default schema)
```

## Stack

| Concern | Crate |
|---|---|
| HTTP server | `axum 0.7` + `tower-http` |
| Storage | log-structured object store (S3 / MinIO), materialised in memory |
| Auth | `jsonwebtoken 9` (RS512 JWT) + `casbin` (RBAC) |
| Serialization | `serde` + `serde_json` |
| Cloud credentials | `aws-sdk-sts` (always compiled in; vending toggled at runtime via `--enable-aws-credentials`, default on) |

## Quick Start

### 1. Build

```bash
cargo build
```

There is one backend and no feature flags to choose it.

### 2. Run the server

Key material comes from a secret store, so generate some once:

```bash
./target/debug/uc-server --generate-key-file ./etc/conf/keys.json
```

Then point it at an object store (`AWS_ENDPOINT_URL` redirects to MinIO):

```bash
AWS_ENDPOINT_URL=http://localhost:9000 \
./target/debug/uc-server \
  --port 8080 \
  --storage-root s3://my-bucket/my-org \
  --key-file ./etc/conf/keys.json \
  --no-auth
```

Key material is never written to the object store, and there is no option to do
so. Credentials vended by this server are bucket-scoped, so a private key in
that bucket would be readable by anything holding one. A missing key file is a
startup error, never a cue to generate: silently minting a new keypair
invalidates every token already issued.

### 3. Seed sample data

```bash
python3 scripts/seed.py
# or against a different host:
python3 scripts/seed.py http://localhost:8080
```

This creates: catalog `unity`, schema `default`, tables (marksheet, numbers, user_countries), volumes (txt_files, json_files), functions (sum, lowercase).

### 4. Run integration tests

```bash
pip install unitycatalog-client pytest pytest-asyncio
cd tests/python
python3 -m pytest -v
```

Requires the server to be running and seeded first.

## API Coverage

### Catalog API — `/api/2.1/unity-catalog/*`

| Resource | Ops |
|---|---|
| Catalogs | list, create, get, update, delete |
| Schemas | list, create, get, update, delete |
| Tables | list, create, get, delete |
| Volumes | list, create, get, update, delete |
| Functions | list, create, get, delete |
| Registered Models | list, create, get, update, delete |
| Model Versions | list, create, get, update, finalize, delete |
| Credentials | list, create, get, update, delete |
| External Locations | list, create, get, update, delete |
| Permissions | get, update |
| Metastore | summary |
| Staging Tables | create |
| Delta Commits | list, commit |
| Temp Credentials | table, volume, model-version, path |

### Delta Protocol API — `/delta/v1/*`

Config negotiation, table CRUD, CCv2 coordinated commits (`add-commit`, `set-properties`, `set-protocol`, `set-columns`, `set-partition-columns`, `set-domain-metadata`), rename, metrics, staging tables, credential vending.

### Control API — `/api/1.0/unity-control/*`

OAuth2 token exchange (RFC 8693), JWKS endpoint, SCIM2 user management.

### Not implemented

Iceberg REST catalog (`/api/2.1/unity-catalog/iceberg/*`).

## Authentication

**Disabled (development):** pass `--no-auth` — all requests allowed, dummy claims injected.

**Enabled (default):** JWT bearer token required. RS512 RSA-2048 keys auto-generated on startup. Token exchange via `POST /api/1.0/unity-control/auth/tokens`.

RBAC uses [Casbin](https://casbin.org/) with a hierarchical model: Metastore → Catalog → Schema → Table/Volume/Function/Model.

## Storage

Metadata is an append-only JSONL commit log with periodic checkpoints, laid out
like Delta's `_delta_log`, materialised in memory at startup. There is no
database, no driver, no migrations and no local state.

Concurrency rests on conditional writes (`If-None-Match: *`), so it needs S3
(August 2024 or later) or MinIO. Delta commits are partitioned into a log per
table, matching Delta's own layout.

Multiple replicas may share one log. Writes are safe — a stale replica loses the
conditional write, replays and retries — but reads are only eventually
consistent, bounded by `--refresh-interval-secs`.

See [docs/log-structured-metadata.md](docs/log-structured-metadata.md) for the
design and its limits.

## CLI Options

```
--port                    Port to listen on (default: 8080)
--storage-root            Object-store root, as s3://bucket/prefix
--key-file PATH           JWT signing key material, the path a mounted secret
                          arrives at. Required. A missing file is an error,
                          never a cue to generate.
--generate-key-file PATH  Write fresh key material to PATH and exit, for loading
                          into a secret store. Refuses to overwrite an existing
                          file.
--config-dir              Config directory — holds the dev admin token
                          (default: ./etc/conf)
--refresh-interval-secs   Refresh the in-memory snapshot from the log every N
                          seconds (default 0, off). Only useful with more than
                          one replica on the same log.
--no-auth                 Disable JWT/RBAC enforcement
```

## Development

```bash
cargo check                                  # fast type check
cargo test --lib                             # unit tests across the workspace
cargo build                                  # full build

cargo test -p uc-db --test test_repos          # repo layer, end to end
```

## License

Licensed under the [Apache License, Version 2.0](LICENSE). See [NOTICE](NOTICE) for
attribution to the upstream [Unity Catalog](https://github.com/unitycatalog/unitycatalog)
Java project this is ported from.
