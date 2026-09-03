# Migrating a SQLite deployment to the log store

The SQL backends are gone, so an existing `uc.db` cannot be opened by the
current binary. Its contents have to be written into the object store first.

`scripts/migrate_sqlite_to_log.py` does this. It is stdlib-only, read-only with
respect to the database, and writes to a local directory so the output can be
inspected before it is uploaded.

    ./scripts/migrate_sqlite_to_log.py /path/to/uc.db ./out
    mc cp --recursive ./out/_uc_log  myminio/uc-meta/<org>/

What follows is why it does what it does. The traps below are the ones that
lose data quietly rather than loudly, and they are the reason this is a script
in the repository rather than something reconstructed each time.

## What does not need migrating

**Key material.** Earlier revisions of this work assumed the RSA keypair in the
config directory had to be carried across. It does not: uc-server issues no
tokens now, so it holds no signing key. `private_key.der`, `public_key.der`,
`key_id.txt` and `certs.json` are simply discarded, and no tokens are
invalidated by that because there are no UC-issued tokens in circulation —
callers present tokens from the OIDC issuer, which is unaffected.

**Table data.** Only metadata lives in `uc.db`. Delta files, volumes and
anything else in object storage are untouched by any of this.

## The output is one checkpoint, not a replay

The obvious approach — replay every row as its own commit — is unnecessary. A
checkpoint *is* the materialised state, as JSONL `upsert` lines, and the store
loads one at startup in preference to replaying. So the migrator writes:

```
<root>/_uc_log/00000000000000000001.checkpoint.json    every row, one per line
<root>/_uc_log/_last_checkpoint                        {"version":1,"size":N,"checksum":"…"}
```

and nothing else. On boot the store resolves the pointer, loads the checkpoint,
finds no commits above version 1, and serves. The next write lands at version 2.

`size` is the line count and `checksum` is FNV-1a over the body; both are
verified on load, and a mismatch silently falls back to a full log scan — which
here means an empty catalog. Getting them wrong therefore fails *quietly*, so
the migrator must verify its own output by reading it back.

## Ids must be preserved exactly

Not negotiable, and the reason an export/import through the public API will not
do. Table ids are handed to Delta clients as `table-uuid`, and every casbin
grant references principal and resource ids. Regenerating them breaks every
permission and every client holding a uuid, silently and later.

## Shape of each line

```json
{"upsert":{"kind":"catalog","id":"<uuid>","body":{ …the row… }}}
```

`kind` is the snake_case `EntityKind`; `body` must deserialise into the
matching `*Row` struct in `crates/uc-db/src/models/`. Four conversions matter:

- **UUIDs are `BLOB`** in SQLite — 16 raw bytes. JSON needs the hyphenated form.
- **Booleans are `INTEGER`** 0/1. JSON needs `true`/`false`. Affects
  `nullable`, `is_backfilled_latest_commit`, `stage_committed`,
  `is_deterministic`, `is_null_call`.
- **`uc_tables.type`** is `r#type` in Rust and serialises as `"type"`.
- **`casbin_rule` has no UUID** — it uses an `INTEGER AUTOINCREMENT` surrogate.
  The log needs one, and insertion order is significant (the adapter returns
  rows `ORDER BY id`, which the store reproduces by sorting on UUIDv7). So mint
  v7 ids **in ascending `casbin_rule.id` order** and the ordering survives.

## Delta commits do not go in the checkpoint

`uc_delta_commits` is not a snapshot entity. Each row becomes its own object in
that table's partition:

```
<root>/_uc_log/tables/<table_id>/<commit_version padded to 20>.json
```

containing the `DeltaCommitRow` JSON directly — not wrapped in an `upsert`. The
version is the object key, which is what makes the uniqueness constraint the
filename. Writing these into the checkpoint instead would leave every table with
no commit history and no way to notice.

## Ordering

Sort by kind, then by id, as `Snapshot::encode_checkpoint` does. Not required for
correctness, but it makes the output byte-reproducible, so a re-run over an
unchanged database produces an identical object and a diff means something.

## Where the log goes

**Not** the bucket that vends credentials. Credentials are vended per bucket with
no session policy, so anything holding one for the data bucket could read and
rewrite a log placed there. The metadata root must be a separate bucket that the
vending role has no access to.

## Procedure

1. Stop the old server. It is the only writer; a live `uc.db` is a moving target.
2. Copy `uc.db` somewhere and migrate from the copy, so the original stays
   pristine as the rollback.
3. Run the migrator, writing to the metadata root.
4. **Verify before starting anything**: per-kind entity counts against
   `SELECT count(*)` per table, and spot-check that a known table's id and a
   known grant survived unchanged.
5. Start uc-server with `--storage-root`. It logs the version it replayed to;
   confirm it is 1 and that the entity-count gauges match step 4.
6. Compare a handful of API responses against what the old deployment returned.

## Rollback

The migration is read-only with respect to `uc.db`, so rollback is pointing the
previous image back at the volume. That requires **keeping the pre-migration
image tag**, since the current build cannot open a database at all. Pin it
before starting.

## What has been verified

A fixture database built from the pre-removal schema — metastore, catalog,
schema, table, column, volume, user, three casbin rules and three delta commits
— migrates, uploads to MinIO, and uc-server boots against it and serves the
catalog, schema, table and volume over the API. The table id it serves is
byte-identical to the one in SQLite, the delta commits land as three partition
objects, and a write succeeds on top of the migrated state.

What that does *not* cover: functions, models, staging tables, properties,
dependencies and external locations have no fixture rows, so their column
mappings are exercised by the generic row-to-JSON path but not asserted. Run
the count check in step 4 against a real database before trusting them.
