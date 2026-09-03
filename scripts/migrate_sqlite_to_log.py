#!/usr/bin/env python3
"""One-off migration: a legacy SQLite uc.db -> the log-structured metadata store.

Writes to a local directory so the output can be inspected before it goes
anywhere; upload it afterwards, e.g.

    ./scripts/migrate_sqlite_to_log.py /path/to/uc.db ./out
    mc cp --recursive ./out/_uc_log  myminio/uc-meta/<org>/

Stdlib only, and read-only with respect to the database: the original stays
pristine as the rollback.

This is a throwaway. It is not wired into the build and has no tests of its own
beyond the round-trip check in `scripts/test_migrate.sh`.
"""

import json
import os
import sqlite3
import sys
import time
import uuid

# Table -> EntityKind, using the serde snake_case names the store reads.
KINDS = {
    "uc_metastore": "metastore",
    "uc_catalogs": "catalog",
    "uc_schemas": "schema",
    "uc_tables": "table",
    "uc_columns": "column",
    "uc_volumes": "volume",
    "uc_functions": "function",
    "uc_function_parameters": "function_parameter",
    "uc_registered_models": "registered_model",
    "uc_model_versions": "model_version",
    "uc_staging_tables": "staging_table",
    "uc_users": "user",
    "uc_credentials": "credential",
    "uc_external_locations": "external_location",
    "uc_properties": "property",
    "uc_dependencies": "dependency",
}

# INTEGER 0/1 in SQLite, bool in the Rust structs. Anything not listed stays a
# number, so adding a bool column upstream without adding it here would produce
# a body that fails to deserialise -- loudly, at replay, which is the right way
# for this to fail.
BOOLS = {
    "nullable",
    "is_backfilled_latest_commit",
    "stage_committed",
    "is_deterministic",
    "is_null_call",
}

# Declaration order of EntityKind, which is what `kinds.sort()` produces on the
# Rust side. Matching it makes the output byte-identical to a checkpoint the
# server would write itself.
KIND_ORDER = [
    "metastore", "catalog", "schema", "table", "column", "volume", "function",
    "function_parameter", "registered_model", "model_version", "staging_table",
    "delta_commit", "user", "credential", "external_location", "property",
    "dependency", "casbin_rule",
]


def fnv1a64(data: bytes) -> str:
    """Matches store::log::content_hash."""
    h = 0xCBF29CE484222325
    for b in data:
        h ^= b
        h = (h * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return f"{h:016x}"


_last_v7 = 0


def uuid_v7() -> uuid.UUID:
    """Time-ordered v7, monotonic within a run.

    Monotonicity matters for casbin_rule: the adapter returns rows ordered by
    id, and the store reproduces that by sorting on the UUID. Mint these in
    ascending casbin_rule.id order and the ordering survives the migration.
    """
    global _last_v7
    ms = int(time.time() * 1000)
    if ms <= _last_v7:
        ms = _last_v7 + 1
    _last_v7 = ms
    rand = int.from_bytes(os.urandom(10), "big")
    val = (ms << 80) | (0x7 << 76) | ((rand >> 66) << 64) | (0b10 << 62) | (rand & ((1 << 62) - 1))
    return uuid.UUID(int=val & ((1 << 128) - 1))


def convert(col: str, val):
    """SQLite value -> what the Rust struct expects."""
    if val is None:
        return None
    # UUIDs are stored as 16 raw bytes; JSON needs the hyphenated form.
    if isinstance(val, (bytes, bytearray)):
        if len(val) == 16:
            return str(uuid.UUID(bytes=bytes(val)))
        return val.decode("utf-8", "replace")
    if col in BOOLS:
        return bool(val)
    return val


def rows(conn, table):
    cur = conn.execute(f"SELECT * FROM {table}")
    names = [d[0] for d in cur.description]
    for row in cur.fetchall():
        yield {n: convert(n, v) for n, v in zip(names, row)}


def main():
    if len(sys.argv) != 3:
        sys.exit(f"usage: {sys.argv[0]} <uc.db> <output-dir>")
    db_path, out_dir = sys.argv[1], sys.argv[2]

    conn = sqlite3.connect(f"file:{db_path}?mode=ro", uri=True)
    existing = {
        r[0] for r in conn.execute("SELECT name FROM sqlite_master WHERE type='table'")
    }

    # ── collect every snapshot entity ────────────────────────────────────────
    entities = {}  # kind -> {id: body}
    for table, kind in KINDS.items():
        if table not in existing:
            continue
        for body in rows(conn, table):
            entities.setdefault(kind, {})[body["id"]] = body

    # casbin_rule has an INTEGER surrogate; the log needs a UUID, and insertion
    # order is significant, so mint them in ascending id order.
    if "casbin_rule" in existing:
        cur = conn.execute(
            "SELECT ptype, v0, v1, v2, v3, v4, v5 FROM casbin_rule ORDER BY id"
        )
        for ptype, *v in cur.fetchall():
            body = {"ptype": ptype}
            body.update({f"v{i}": (x or "") for i, x in enumerate(v)})
            entities.setdefault("casbin_rule", {})[str(uuid_v7())] = body

    # ── the checkpoint: one upsert per line, kind then id, as the server writes
    lines = []
    for kind in KIND_ORDER:
        for ent_id in sorted(entities.get(kind, {})):
            lines.append(
                json.dumps(
                    {"upsert": {"kind": kind, "id": ent_id,
                                "body": entities[kind][ent_id]}},
                    separators=(",", ":"),
                    sort_keys=True,
                )
            )
    body = ("\n".join(lines) + "\n").encode() if lines else b""

    log_dir = os.path.join(out_dir, "_uc_log")
    os.makedirs(log_dir, exist_ok=True)
    version = 1
    with open(os.path.join(log_dir, f"{version:020}.checkpoint.json"), "wb") as f:
        f.write(body)
    with open(os.path.join(log_dir, "_last_checkpoint"), "w") as f:
        json.dump({"version": version, "size": len(lines),
                   "checksum": fnv1a64(body)}, f)

    # ── delta commits are not snapshot entities ──────────────────────────────
    # Each is its own object in the table's partition, where the version *is*
    # the key. Putting these in the checkpoint would leave every table with no
    # commit history and nothing to notice it.
    n_commits = 0
    if "uc_delta_commits" in existing:
        for c in rows(conn, "uc_delta_commits"):
            part = os.path.join(log_dir, "tables", str(c["table_id"]))
            os.makedirs(part, exist_ok=True)
            name = f"{int(c['commit_version']):020}.json"
            with open(os.path.join(part, name), "w") as f:
                json.dump(c, f, separators=(",", ":"), sort_keys=True)
            n_commits += 1

    conn.close()

    print(f"checkpoint: {len(lines)} entities, version {version}")
    for kind in KIND_ORDER:
        if entities.get(kind):
            print(f"  {kind:<20} {len(entities[kind])}")
    print(f"delta commits: {n_commits} objects in per-table partitions")
    print(f"\nwritten to {log_dir}")


if __name__ == "__main__":
    main()
