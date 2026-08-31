# Log-structured metadata (no database, no PVC)

Status: design sketch, branch `feat/log-structured-metadata`. Nothing here is wired up yet.

## Why

uc-server today keeps its metadata in SQLite on a per-org ReadWriteOnce PVC. That
works, but it pins each org to exactly one uc-server replica forever, and it makes
the org's durable state a volume rather than an object.

The metadata is small, low-write, and read-dominated. That is the profile that
suits an append-only log with an in-memory materialisation: commits go to object
storage, every replica replays the log at startup, and the PVC disappears.

## The primitive this rests on

The whole design needs exactly one thing from the object store: **atomic
create-if-absent**. That is what makes "commit version N" have a single winner
among racing writers.

S3 gained this in August 2024 (`If-None-Match: *` on PutObject). MinIO supports
it too, and the deployment here is on `RELEASE.2025-04-08`, comfortably past it.

This is worth stating plainly because it is the reason this design was not
viable earlier. UC is itself the commit coordinator for its tenants' Delta
tables (that is what `uc_delta_commits` is). Before conditional writes existed,
UC's own log would have needed a coordinator, and the only available coordinator
would have been UC, which is still booting. Conditional PUT breaks that
recursion. UC's own log bootstraps on the raw object-store primitive and never
routes through UC.

## Why not literal Delta format

Because of the bootstrap above, UC's own log cannot be a *managed* Delta table —
it has to sit on the raw conditional-put path. So Delta format would buy
readability, not mechanism, while putting protocol versions, schema evolution
and Parquet checkpoint reading on the boot-critical path.

A plain append-only JSON log with a JSON checkpoint is the same architecture with
a fraction of the surface. If "query UC metadata as a table" is wanted later, it
can be a derived export rather than the primary store.

## Layout

Modelled directly on `_delta_log`:

    <storage_root>/_uc_log/
      00000000000000000001.json         JSONL commit: commitInfo + one action/line
      00000000000000000002.json
      ...
      00000000000000000200.checkpoint.json
      _last_checkpoint                  {"version": 200, "size": 1483}
      _keys.json                        JWT signing keypair, conditionally created

20-digit zero-padded versions so lexicographic LIST order equals numeric order.

Commit files are **JSONL**, not a JSON array — same as Delta, which also keeps
the `.json` extension despite the newline-delimited contents. Following that
naming means anyone who can read `_delta_log` can read `_uc_log` unprompted.

    {"commitInfo":{"format":1,"timestamp":1756...,"operation":"CREATE CATALOG"}}
    {"upsert":{"kind":"catalog","id":"018f...","body":{...}}}
    {"remove":{"kind":"schema","id":"018f..."}}

`commitInfo` first, as in Delta, carrying provenance only — replay ignores it,
so it can be extended without a format bump. Actions are externally tagged
single-key objects (`{"upsert": ...}`), mirroring Delta's `{"add": ...}` /
`{"remove": ...}`; that is serde's default enum encoding, so it costs nothing
and keeps the log greppable by action type.

JSONL rather than one JSON document because a commit can be applied line by line
without holding the file in memory, a truncated trailing line is detectable
where a truncated array is not, and the line count is the action count that
`_last_checkpoint.size` reports.

Checkpoints are JSONL too — a state dump of `upsert` lines only, since a
checkpoint is state rather than history and a deletion is represented by
absence. Delta uses Parquet here; JSONL keeps one parser on the boot path, and
UC metadata is nowhere near the size that motivates columnar checkpoints.
Encoding is deterministic (sorted by kind then id) so two replicas checkpointing
the same version write identical bytes, which is what makes `size` a usable
truncation guard.

## Commit protocol

    1. read current in-memory version V
    2. evaluate the precondition against in-memory state
         (e.g. "no catalog named 'x'", "no commit at version 7 for table T")
    3. PUT _uc_log/{V+1}.json  with  If-None-Match: *
    4a. 201  -> apply actions to in-memory state, version = V+1, done
    4b. 412  -> someone else took V+1. Replay from V+1 forward, then GOTO 2.
                Bounded retries; surface the domain error (AlreadyExists /
                CommitVersionConflict) if the precondition now fails.

Step 4b is the part that carries correctness and is easy to get wrong. The
conditional PUT alone only gives *serialisation* — it says who owns version N+1.
Uniqueness comes from **re-evaluating the precondition after replaying the
commit that beat us**. Two replicas can both believe a catalog name is free;
only one wins the version, and the loser must re-check rather than blindly
retry, or it will happily write a duplicate name at V+2.

## Replay and checkpoint

Startup: GET `_last_checkpoint` -> load that checkpoint -> LIST log files above
it -> apply in order. `_last_checkpoint` is advisory only; if it is missing or
stale, fall back to LIST from 0. Never treat it as authoritative, or a failed
checkpoint write becomes data loss.

Checkpoint every N commits (start with 100). Writing a checkpoint is idempotent
and racy-safe: two replicas may write the same checkpoint version with the same
content. Old log files can be pruned behind the newest checkpoint, but not in v1.

## The seam in the code

The repo layer is already a keyed document store wearing SQL. Across 108 call
sites there are 2 transactions and 1 join. Every repo function is a free
function shaped `(pool: &AnyPool, ...) -> Result<Row, UcError>`.

So the swap is a type alias:

    -pub type AnyPool = sqlx::SqlitePool;
    +pub type AnyPool = Store;

`AppState.pool: Arc<AnyPool>` derefs to `&Store`, and the ~262 `&state.pool`
call sites in uc-api compile unchanged. The work is confined to:

  - crates/uc-db/src/repos/*.rs      13 modules, ~1400 lines of bodies
  - crates/uc-api/src/**             19 direct sqlx sites (12 of them in
                                     delta_api/tables.rs)
  - crates/uc-auth/src/db_adapter.rs 431-line casbin adapter -> policy lives in
                                     the log, or casbin's file adapter
  - crates/uc-server/src/main.rs     drop prepare_database_url + the S3 sync

Rename `AnyPool` -> `Store` afterwards as a separate mechanical commit; doing it
in the same change would bury the real diff.

## Behaviour to preserve exactly

  - Cursor pagination is `ORDER BY name` + `name > token`. A `BTreeMap<String, Uuid>`
    name index gives that range scan directly.
  - Unique violations map to *domain* errors, not 500s: `CatalogAlreadyExists`,
    `CommitVersionConflict`. Preserve the mapping.
  - **FKs are declared but not enforced.** pool.rs never sets
    `PRAGMA foreign_keys=ON`, so SQLite has them off. Do not "helpfully" add
    referential integrity during the port — that is a behaviour change wearing
    a bugfix costume, and it will reject writes that work today.

## The PVC holds more than the database

`--config-dir /var/uc` is the same mount as `uc.db`. It also holds
`private_key.der`, `public_key.der`, `key_id.txt` and `certs.json` — the JWT
signing keypair (uc-auth/src/keys.rs).

Delete the PVC without moving these and every restart generates a fresh keypair:
all outstanding UC tokens become invalid and the JWKS `kid` rotates. It fails at
runtime for clients, not at startup, so it will not show up in a smoke test.

Keys move to `_keys.json` via the same conditional create — first boot is a
race between replicas, and PUT-if-absent makes the loser adopt the winner's key
instead of generating its own.

## Open question: read freshness

Once there is more than one replica, replica B serves stale reads until it
replays. The PVC makes that impossible today, so it is a new problem the log
introduces.

v1 should stay single-replica and treat multi-replica as explicitly not yet
supported. The options when it matters: replay-before-read on mutating paths
only, short-poll the log tail, or accept bounded staleness on list/get. Not
worth choosing now — but worth not pretending multi-replica arrives for free.

## Sequencing

  1. `store/` module: actions, log, replay, checkpoint, in-memory indices  <- sketched
  2. port repos/catalog.rs as the worked example                           <- sketched
  3. port remaining 12 repo modules
  4. port the 19 direct sqlx sites in uc-api
  5. casbin policy off the pool
  6. keys to _keys.json
  7. drop PVC from k8s_effects.rs, pass --storage-root instead of --database-url
  8. rename AnyPool -> Store

Steps 1-6 keep the SQLite build working behind a feature flag, so the cutover in
7 is the only irreversible step.

## Prerequisite found while sketching

None of the 14 `*Row` models in `crates/uc-db/src/models/` derive `Serialize`
or `Deserialize` — they are `FromRow` only. The log format serialises rows as
JSON, so this is a hard prerequisite for step 1, not a detail of step 3.

It is a good first commit on its own: additive, no behaviour change, reviewable
in isolation, and it keeps the SQL build green.

Note also that ids are `BLOB` in SQLite. Serde will render `Uuid` as a hyphenated
string in the log, which is the right choice for a debuggable log format, but it
means the log is not a byte-for-byte dump of the table and a migration path from
an existing `uc.db` has to go through the repo layer rather than a raw copy.
