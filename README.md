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
│   ├── uc-db/           sqlx row structs + repositories (SQLite / Postgres)
│   ├── uc-auth/         JWT (RS512) + Casbin RBAC
│   ├── uc-credentials/  AWS/Azure/GCP credential vending
│   ├── uc-api/          Axum routers — catalog, control, delta APIs
│   └── uc-server/       Binary: startup wiring, CLI args, serve
├── migrations/
│   ├── sqlite/          DDL for SQLite (default)
│   └── postgres/        DDL for PostgreSQL
├── tests/python/        Pytest integration tests
├── scripts/
│   └── seed.py          Seeds sample data (unity catalog + default schema)
```

## Stack

| Concern | Crate |
|---|---|
| HTTP server | `axum 0.7` + `tower-http` |
| Database | `sqlx 0.7` (SQLite default, Postgres via feature flag) |
| Auth | `jsonwebtoken 9` (RS512 JWT) + `casbin` (RBAC) |
| Serialization | `serde` + `serde_json` |
| Cloud credentials | `aws-sdk-sts` (feature-gated) |

## Quick Start

### 1. Build

```bash
cargo build
```

For Postgres instead of SQLite:

```bash
cargo build --no-default-features --features postgres
```

### 2. Run the server

```bash
./target/debug/uc-server \
  --port 8080 \
  --config-dir ./etc/conf \
  --database-url "sqlite:./etc/db/uc.db?mode=rwc" \
  --no-auth
```

RSA keys are generated automatically on first start under `--config-dir`.

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

## Database

SQLite is the default (zero setup). Switch to Postgres at compile time:

```bash
cargo build --no-default-features --features postgres
```

Migrations run automatically on startup from `migrations/sqlite/` or `migrations/postgres/`.

## CLI Options

```
--port          Port to listen on (default: 8080)
--config-dir    Path to config directory — RSA keys, JWKS, token (default: ./etc/conf)
--database-url  SQLite or Postgres connection string
--no-auth       Disable JWT/RBAC enforcement
```

## Development

```bash
cargo check          # fast type check
cargo test --lib     # 16 unit tests (JWT, serde, error mapping)
cargo build          # full build
```

## License

Licensed under the [Apache License, Version 2.0](LICENSE). See [NOTICE](NOTICE) for
attribution to the upstream [Unity Catalog](https://github.com/unitycatalog/unitycatalog)
Java project this is ported from.
