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
      _last_checkpoint                  {"version": 200, "size": 1483, "checksum": …}

No key material. The signing keypair is supplied by `--key-file` from a secret
store and is never written here — see "Key material comes from a secret store".

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

Keys move to a Kubernetes Secret, mounted and passed as `--key-file`. An
earlier revision put them in the object store via a conditional create; that is
gone, because UC vends bucket-scoped credentials with no session policy and a
private key in that bucket would be readable by anything holding one.

## Rejected: Raft

Raft would mean running a consensus protocol on top of a system that already
provides one. `If-None-Match` on S3 is a linearizable compare-and-swap backed by
AWS's own replicated log; the object store *is* the consensus, and paying for a
second one buys nothing.

It also defeats the goal. Raft needs durable local state (log, term, vote) and
stable node identity for membership — so it would reintroduce the PVC and add a
StatefulSet, having set out to delete a PVC. Quorum means at least three
uc-server replicas per org where there is currently one at 100m/128Mi, and
writes would become unavailable below quorum, whereas a single pod against S3
works fine and inherits the object store's durability.

Raft would be right if writes needed single-digit-millisecond latency, or if the
object store could not do conditional writes. Neither holds here.

## Partitioning (decided: per-table delta logs)

One global log per org would serialise every write through a single version
counter. Two independent problems, both pointing the same way:

  - **Write concurrency.** Catalog CRUD does not care — humans create schemas.
    But every Delta commit in the org lands in `uc_delta_commits`, and SQLite
    accepts concurrent commits to *different* tables today via
    UNIQUE(table_id, commit_version). A shared log would not: a regression, not
    a wash.
  - **Boot cost.** `uc_delta_commits` is the only unbounded-growth entity —
    every commit to every table appends forever. Left in the main log it would
    dominate replay and every checkpoint, making startup scale with total
    commit history rather than with schema size. This is the larger problem of
    the two.

So Delta commits are partitioned out, one stream per table:

    _uc_log/tables/{table_id}/00000000000000000007.json

which is what Delta itself does — one `_delta_log` per table, not one per
metastore.

**The rule for where to cut.** A partition boundary is only safe if no
invariant spans it. UNIQUE(table_id, commit_version) lives entirely inside one
table's stream, so it survives the split intact. Catalogs, schemas, tables and
volumes stay in the shared log: they are low-write, and their invariants
interlock (a schema's uniqueness is scoped by its catalog).

**The payoff is that the partition collapses the mechanism.** `commit_version`
*is* the log version, so the constraint and the object key are the same thing.
`insert` becomes a single conditional PUT — no snapshot read, no commit loop, no
retry, no in-memory state for commit history at all — and a conflict arrives as
`AlreadyExists` rather than as something that has to be detected. Partitioning
made this path simpler than the unpartitioned version, not more complex.

`latest_version` keeps a per-table hint so the common case is a short listing
rather than paging the whole history. The hint is never trusted: appends are
conditional, so a stale hint costs one rejected PUT and a re-list, never
correctness. That is what lets it be a plain cache with no cross-replica
invalidation protocol.

**One sharp edge.** Partition keys sit under the same `_uc_log/` prefix as
metastore commits, and `tables/{id}/00000000000000000007.json` has a final
segment that parses as a version perfectly well. `action::version_from_key`
must reject nested keys, or a table's commit stream replays into the metastore
snapshot as metadata. Covered by
`per_table_partitions_are_not_mistaken_for_main_log_commits` and
`delta_partitions_do_not_corrupt_metastore_replay`.

## Listings must be drained, not trusted

The nastiest failure this design has. `ObjectLog::list_after` maps onto S3
ListObjectsV2, which caps a response at 1000 keys. A backend adapter that
returns one page — the obvious way to write it — silently corrupts three paths:

  - main-log replay stops at the first page, so a restarted uc-server serves a
    stale, partial metastore;
  - `latest_version` returns a version lower than the true head, so a Delta
    client commits at a version that already exists;
  - `list_for_table` returns a truncated commit history as if complete.

None of it errors. The gap check does not catch it either: keys 1..N of a longer
log are perfectly contiguous, so tail truncation looks exactly like a shorter
log. Verified by writing a `TruncatingLog` that pages at 10 — replay reported
version 10 of 25, and `latest_version` reported 9 of 24, both silently.

The fix is `log::list_all_after`, which pages until a listing comes back empty.
Every caller goes through it; a truncating backend then costs extra round trips
instead of correctness. It also refuses a backend that ignores `start_after`
rather than looping forever on it.

The contract is now stated on the trait: `list_after` MAY return a partial page,
and no caller may treat one call as the complete set. Stating it was not enough
on its own — the safeguard has to be in the shared helper, because a doc comment
does not survive the next implementor.

## Natural keys must be verified against the schema, not recalled

Three of the natural keys in the first sketch were wrong, and all three would
have failed silently — the store would simply enforce a different constraint
than SQLite did, or none:

  - `User` was keyed on `email`. `uc_users` declares `name TEXT NOT NULL
    UNIQUE`; `email` is nullable with no constraint. So the real uniqueness
    check was lost, and every user with a null email would have dropped out of
    the index entirely.
  - `Column` had no key at all, despite UNIQUE(table_id, ordinal_position).
  - `Property` had no key, despite UNIQUE(entity_id, entity_type, property_key).

Correctly absent: uc_metastore, uc_function_parameters, uc_model_versions (its
(registered_model_id, version) index is *not* UNIQUE), uc_staging_tables,
uc_dependencies. Inventing keys for those would reject writes SQLite accepts.

Integer components are rendered with `pad_i64` so lexicographic order matches
numeric order across zero — ordinal_position feeds ordered reads.

Tests now pin each key to its constraint, because this is exactly the class of
error that passes every functional test.

### The NUL separator has a precondition

Composite keys join components with NUL. That is unambiguous only while every
component *except the last* is NUL-free — a precondition on the inputs, not a
property of the encoding.

It holds today because user-supplied text is always the final component: names
sit last in every two-part key, and Property's middle component `entity_type`
is an internal literal ('table', 'schema', 'catalog'), never client input. If a
variable-length user-supplied field ever moves out of last position, this needs
escaping or length prefixes. Pinned by
`nul_separation_is_ambiguous_if_a_non_final_component_contains_nul`.

## Two SQL behaviours the port tightens

Both are cases where the log store is stricter than SQLite, not merely
equivalent. Called out because they are behaviour changes, even if they are
changes toward correctness.

**`metastore::get_or_init`** is a read-then-insert with nothing between the two
statements, and `uc_metastore` carries no UNIQUE constraint, so two uc-servers
starting together can both observe no row and both insert. Neither notices,
because `get` uses `LIMIT 1`. On the log store the check runs inside the commit
closure, so a replica that loses the race re-runs it, finds the winner's row,
and returns that.

**`property::replace`** is a DELETE followed by one INSERT per property, and its
own doc comment says it "must be called inside a transaction" — leaving
atomicity as a precondition on every caller. As a single commit the deletes and
inserts land together, so no reader can observe the entity mid-replace with its
properties missing. The warning no longer applies.

A no-op commit is skipped rather than written. `get_or_init` returns no actions
once the metastore exists, and it runs on every startup; writing an empty commit
would burn a log version and an object each time, growing the log forever
without changing state.

## What guards against corruption

Layered, and worth being precise about what each layer does and does not cover.

**The object store carries transfer and at-rest integrity.** S3 and MinIO
checksum uploads and validate on read; a silently corrupted object is their
problem, not something to re-solve here. Conditional PUT is also atomic, so
there is no such thing as a half-written commit to detect.

**Commits are validated structurally, not by checksum:**

  - every line must parse as JSON — a malformed line is fatal, never skipped;
  - `commitInfo` must be present, so a commit that cannot be version-checked is
    refused rather than assumed to be format 1;
  - a future `format` is refused rather than guessed at;
  - versions must be gapless, so a deleted or missing commit fails startup;
  - a key that lists but does not read is a hole, and fails rather than being
    skipped.

There is deliberately no per-commit checksum. It would have to live in a side
object — a file cannot contain its own hash — which doubles the writes on the
hot path to catch a case the object store already covers. Delta does not
checksum its commits either.

**Checkpoints get a content hash**, because they are the one place structural
validation is genuinely insufficient. `size` (the line count) catches a *short*
file, but a flipped byte inside a line leaves the count intact, and a corrupted
row that still parses as JSON would be materialised as real state with nothing
to notice. `_last_checkpoint.checksum` closes that.

It is FNV-1a, not a cryptographic digest: it catches accidental corruption,
which is the real threat, and it explicitly does **not** detect deliberate
tampering by someone who can write to the bucket. Anyone reading this should
not mistake it for an integrity guarantee against an adversary.

Two properties make it work:

  - `encode_checkpoint` is deterministic, so two replicas checkpointing the
    same version produce identical bytes and therefore the same hash. The
    determinism was built for idempotent writes; the checksum rides on it.
  - The field is optional, so a pointer written by an older build still loads.
    Treating a missing hash as a mismatch would reject every pre-existing
    checkpoint on upgrade.

A failed check **falls back to a full log scan rather than erroring**. The log
is the source of truth and can always rebuild the state, so a bad checkpoint
costs replay time. Failing hard would turn a recoverable situation into an
unstartable server.

## Read freshness across replicas

Resolved as far as it can be, with the limits stated rather than glossed.

**Writes converge on their own.** A stale replica cannot commit: its conditional
PUT loses, it replays, and its precondition is re-evaluated against what it just
learned. So multi-replica *writes* are safe with no coordination beyond the
object store — demonstrated with two live server processes racing one catalog
name and producing exactly one winner.

**Reads do not.** A replica that only serves reads never learns about another's
commits, and would serve stale metadata indefinitely. That is a real gap the PVC
did not have, because the PVC allowed only one writer in the first place.

`--refresh-interval-secs` bounds it: a background task calls `Store::catch_up`
on a timer, at the cost of one LIST per interval. Default 0, off, because a
single replica does not need it.

What this does **not** do is make reads linearizable. A request can still land
in the window between another replica's commit and the next refresh. It converts
unbounded staleness into bounded staleness, which is a weaker and different
claim, and the right one to make in the docs rather than in a footnote.

`catch_up` holds the write lock across its listing, so readers block for its
duration. Fine for a metadata log; not a pattern to copy onto a hot path.

## Status

  1. `store/` module: actions, log, replay, checkpoint, in-memory indices   DONE
  2. port repos/catalog.rs as the worked example                            DONE
  3. port the remaining 12 repo modules                                     DONE
  4. port the 19 direct sqlx sites in uc-api                                DONE
  5. casbin policy off the pool                                             DONE
  6. key material out of the PVC                                            DONE
     Landed by removing key material altogether rather than relocating it:
     uc-server issues no tokens, so it holds no signing key. See "Auth is
     verify-only" below. The original plan for this step -- putting the private
     key in the object store -- was unsafe and is gone.
  7. uc-server: `--storage-root`, `S3Log`, graceful shutdown, OTLP           DONE
  8. remove the SQL backends entirely                                       DONE
     `AnyPool` keeps its name: it is the handle the build selected, and there
     is now exactly one.

What is left is deployment: whatever runs uc-server must stop passing
`--database-url`, pass `--storage-root` instead, drop the metadata volume, and
raise the memory limit (the snapshot is resident, at roughly 21 KiB per entity).
That is outside this repository.

Migrating an existing SQLite deployment needs a one-off tool; see
`docs/migration.md`.

Not attempted, and deliberately out of scope for v1:

  - **Log pruning.** Old commits below a checkpoint are never deleted, so the
    log grows without bound. Safe but not free.
  - **Multi-replica in production.** Writes are proven safe; reads are only
    eventually consistent, bounded by `--refresh-interval-secs`.
  - **Postgres + logstore.** `logstore` supersedes the SQL backends rather than
    combining with them; the combination is meaningless, not merely untested.

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
