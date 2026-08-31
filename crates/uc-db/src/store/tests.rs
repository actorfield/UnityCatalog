//! Store tests against an in-memory `ObjectLog`.
//!
//! The fake enforces the one guarantee the design rests on — `put_if_absent`
//! never overwrites — so a backend that quietly lost that property would fail
//! here rather than in production.

use super::action::{Action, EntityKind};
use super::log::{ObjectLog, PutResult};
use super::{natural_key_for, pad_i64};
use super::*;

use super::memory::MemoryLog as MemLog;
use put_catalog_helper as put_catalog;

fn catalog(name: &str) -> serde_json::Value {
    serde_json::json!({ "name": name })
}

pub(super) async fn put_catalog_helper(store: &Store, name: &str) -> Result<Uuid, UcError> {
    let id = Uuid::now_v7();
    store
        .commit("CREATE CATALOG", |snap| {
            if snap.get_by_natural_key(EntityKind::Catalog, name).is_some() {
                return Err(UcError::new(
                    ErrorCode::CatalogAlreadyExists,
                    format!("Catalog '{name}' already exists"),
                ));
            }
            Ok((
                vec![Action::Upsert {
                    kind: EntityKind::Catalog,
                    id,
                    body: catalog(name),
                }],
                id,
            ))
        })
        .await
}

#[tokio::test]
async fn commits_replay_into_an_equivalent_snapshot() {
    let log = Arc::new(MemLog::default());
    let store = Store::open(log.clone()).await.unwrap();
    put_catalog(&store, "alpha").await.unwrap();
    put_catalog(&store, "beta").await.unwrap();
    assert_eq!(store.snapshot().await.version, 2);

    // A cold replica sees exactly the same state.
    let replica = Store::open(log).await.unwrap();
    let snap = replica.snapshot().await;
    assert_eq!(snap.version, 2);
    assert!(snap.get_by_natural_key(EntityKind::Catalog, "alpha").is_some());
    assert!(snap.get_by_natural_key(EntityKind::Catalog, "beta").is_some());
}

#[tokio::test]
async fn duplicate_name_is_a_domain_error_not_a_500() {
    let log = Arc::new(MemLog::default());
    let store = Store::open(log).await.unwrap();
    put_catalog(&store, "alpha").await.unwrap();

    let err = put_catalog(&store, "alpha").await.unwrap_err();
    assert_eq!(err.code, ErrorCode::CatalogAlreadyExists);
}

/// The crux. Two stores share a log. B commits the name first; A is holding a
/// snapshot that predates it, so A's conditional PUT loses the race for version
/// 1. A must replay and *re-run its precondition*, discovering the name is now
/// taken — not blindly retry at version 2, which would write a duplicate.
#[tokio::test]
async fn a_lost_race_re_evaluates_the_precondition_instead_of_retrying_blind() {
    let log = Arc::new(MemLog::default());
    let a = Store::open(log.clone()).await.unwrap();
    let b = Store::open(log.clone()).await.unwrap();

    // Both replicas currently believe "shared" is free.
    assert!(a.snapshot().await.get_by_natural_key(EntityKind::Catalog, "shared").is_none());
    assert!(b.snapshot().await.get_by_natural_key(EntityKind::Catalog, "shared").is_none());

    b.put_catalog_for_test("shared").await.unwrap();

    let err = a.put_catalog_for_test("shared").await.unwrap_err();
    assert_eq!(
        err.code,
        ErrorCode::CatalogAlreadyExists,
        "A must re-check after losing the race, not write a duplicate at v2"
    );

    // And exactly one commit exists.
    let cold = Store::open(log).await.unwrap();
    assert_eq!(cold.snapshot().await.version, 1);
}

/// A losing writer whose precondition still holds must land on the next
/// version rather than fail — contention is not an error.
#[tokio::test]
async fn a_lost_race_with_a_still_valid_precondition_retries_onto_the_next_version() {
    let log = Arc::new(MemLog::default());
    let a = Store::open(log.clone()).await.unwrap();
    let b = Store::open(log.clone()).await.unwrap();

    b.put_catalog_for_test("beta").await.unwrap();
    a.put_catalog_for_test("alpha").await.unwrap();

    let cold = Store::open(log).await.unwrap();
    let snap = cold.snapshot().await;
    assert_eq!(snap.version, 2, "both commits landed");
    assert!(snap.get_by_natural_key(EntityKind::Catalog, "alpha").is_some());
    assert!(snap.get_by_natural_key(EntityKind::Catalog, "beta").is_some());
}

#[tokio::test]
async fn rename_frees_the_old_natural_key() {
    let log = Arc::new(MemLog::default());
    let store = Store::open(log).await.unwrap();
    let id = put_catalog(&store, "before").await.unwrap();

    store
        .commit("UPDATE CATALOG", |_| {
            Ok((
                vec![Action::Upsert {
                    kind: EntityKind::Catalog,
                    id,
                    body: catalog("after"),
                }],
                (),
            ))
        })
        .await
        .unwrap();

    let snap = store.snapshot().await;
    assert!(
        snap.get_by_natural_key(EntityKind::Catalog, "before").is_none(),
        "the old name must not stay reachable"
    );
    assert!(snap.get_by_natural_key(EntityKind::Catalog, "after").is_some());
}

#[tokio::test]
async fn scan_is_ordered_and_exclusive_of_the_page_token() {
    let log = Arc::new(MemLog::default());
    let store = Store::open(log).await.unwrap();
    for name in ["delta", "alpha", "charlie", "bravo"] {
        put_catalog(&store, name).await.unwrap();
    }
    let snap = store.snapshot().await;

    let names = |rows: Vec<&serde_json::Value>| -> Vec<String> {
        rows.iter()
            .map(|v| v["name"].as_str().unwrap().to_string())
            .collect()
    };

    assert_eq!(
        names(snap.scan(EntityKind::Catalog, None, 10)),
        vec!["alpha", "bravo", "charlie", "delta"]
    );
    assert_eq!(
        names(snap.scan(EntityKind::Catalog, Some("bravo"), 10)),
        vec!["charlie", "delta"],
        "page token is exclusive, matching WHERE name > $1"
    );
    assert_eq!(names(snap.scan(EntityKind::Catalog, None, 2)), vec!["alpha", "bravo"]);
}

#[tokio::test]
async fn checkpoint_round_trips_and_is_byte_reproducible() {
    let log = Arc::new(MemLog::default());
    let store = Store::open(log.clone()).await.unwrap();
    for name in ["alpha", "beta", "gamma"] {
        put_catalog(&store, name).await.unwrap();
    }

    store.write_checkpoint(3).await.unwrap();

    let body = log.get(&action::checkpoint_key(3)).await.unwrap().unwrap();
    let rebuilt = Snapshot::decode_checkpoint(&body).unwrap();
    assert!(rebuilt.get_by_natural_key(EntityKind::Catalog, "gamma").is_some());

    // Deterministic ordering: re-encoding the rebuilt snapshot yields the same
    // bytes, which is what makes _last_checkpoint.size a usable truncation guard.
    let (again, size) = rebuilt.encode_checkpoint().unwrap();
    assert_eq!(again, body);
    assert_eq!(size, 3);
}

#[tokio::test]
async fn replay_prefers_the_checkpoint_and_still_applies_later_commits() {
    let log = Arc::new(MemLog::default());
    let store = Store::open(log.clone()).await.unwrap();
    put_catalog(&store, "alpha").await.unwrap();
    store.write_checkpoint(1).await.unwrap();
    put_catalog(&store, "beta").await.unwrap();

    let cold = Store::open(log).await.unwrap();
    let snap = cold.snapshot().await;
    assert_eq!(snap.version, 2);
    assert!(snap.get_by_natural_key(EntityKind::Catalog, "alpha").is_some());
    assert!(snap.get_by_natural_key(EntityKind::Catalog, "beta").is_some());
}

#[tokio::test]
async fn a_dangling_checkpoint_pointer_falls_back_to_a_full_scan() {
    let log = Arc::new(MemLog::default());
    let store = Store::open(log.clone()).await.unwrap();
    put_catalog(&store, "alpha").await.unwrap();
    put_catalog(&store, "beta").await.unwrap();

    // Pointer to a checkpoint that was never written.
    log.put(
        action::LAST_CHECKPOINT_KEY,
        serde_json::to_vec(&log::LastCheckpoint { version: 2, size: 2, checksum: None }).unwrap(),
    )
    .await
    .unwrap();

    let cold = Store::open(log).await.unwrap();
    assert_eq!(
        cold.snapshot().await.version,
        2,
        "a missing checkpoint must cost replay time, never data"
    );
}

/// `size` only catches a short file. A flipped byte inside a line leaves the
/// line count intact, and a corrupted row that still parses as JSON would be
/// materialised as real state — so the checksum is what actually guards this.
#[tokio::test]
async fn a_corrupted_checkpoint_is_rejected_in_favour_of_the_log() {
    let log = Arc::new(MemLog::default());
    let store = Store::open(log.clone()).await.unwrap();
    put_catalog(&store, "alpha").await.unwrap();
    put_catalog(&store, "beta").await.unwrap();
    store.write_checkpoint(2).await.unwrap();

    // Flip a byte inside the checkpoint, keeping it valid JSON and the same
    // length — so neither the parser nor the line count notices.
    let key = action::checkpoint_key(2);
    let body = log.get(&key).await.unwrap().unwrap();
    let mut corrupted = String::from_utf8(body.clone()).unwrap();
    assert!(corrupted.contains("alpha"));
    corrupted = corrupted.replace("alpha", "alppa");
    assert_eq!(corrupted.len(), body.len(), "same length, same line count");
    log.remove(&key);
    log.put(&key, corrupted.into_bytes()).await.unwrap();

    let cold = Store::open(log).await.unwrap();
    let snap = cold.snapshot().await;
    assert_eq!(snap.version, 2);
    assert!(
        snap.get_by_natural_key(EntityKind::Catalog, "alpha").is_some(),
        "must fall back to the log, not adopt the corrupted checkpoint"
    );
    assert!(snap.get_by_natural_key(EntityKind::Catalog, "alppa").is_none());
}

#[test]
fn content_hash_changes_on_any_edit() {
    let a = log::content_hash(b"{\"name\":\"alpha\"}");
    assert_eq!(a, log::content_hash(b"{\"name\":\"alpha\"}"), "must be stable");
    assert_ne!(a, log::content_hash(b"{\"name\":\"alppa\"}"));
    assert_ne!(a, log::content_hash(b"{\"name\":\"alph\"}"));
    assert_ne!(log::content_hash(b""), log::content_hash(b"\n"));
}

/// A pointer written before checksums existed carries none. It must still load
/// rather than be treated as a mismatch and rejected on every startup.
#[tokio::test]
async fn a_checkpoint_pointer_without_a_checksum_still_loads() {
    let log = Arc::new(MemLog::default());
    let store = Store::open(log.clone()).await.unwrap();
    put_catalog(&store, "alpha").await.unwrap();
    store.write_checkpoint(1).await.unwrap();

    log.put(
        action::LAST_CHECKPOINT_KEY,
        serde_json::to_vec(&serde_json::json!({"version": 1, "size": 1})).unwrap(),
    )
    .await
    .unwrap();

    let cold = Store::open(log).await.unwrap();
    assert!(cold
        .snapshot()
        .await
        .get_by_natural_key(EntityKind::Catalog, "alpha")
        .is_some());
}

#[tokio::test]
async fn a_truncated_checkpoint_is_rejected_in_favour_of_the_log() {
    let log = Arc::new(MemLog::default());
    let store = Store::open(log.clone()).await.unwrap();
    put_catalog(&store, "alpha").await.unwrap();
    put_catalog(&store, "beta").await.unwrap();
    store.write_checkpoint(2).await.unwrap();

    // Claim more lines than the object holds.
    log.put(
        action::LAST_CHECKPOINT_KEY,
        serde_json::to_vec(&log::LastCheckpoint { version: 2, size: 99, checksum: None }).unwrap(),
    )
    .await
    .unwrap();

    let cold = Store::open(log).await.unwrap();
    let snap = cold.snapshot().await;
    assert_eq!(snap.version, 2);
    assert!(snap.get_by_natural_key(EntityKind::Catalog, "beta").is_some());
}

#[tokio::test]
async fn a_gap_in_the_log_is_refused_rather_than_partially_applied() {
    let log = Arc::new(MemLog::default());
    let store = Store::open(log.clone()).await.unwrap();
    put_catalog(&store, "alpha").await.unwrap();
    put_catalog(&store, "beta").await.unwrap();

    // Simulate a deleted commit.
    log.remove(&action::commit_key(1));

    let err = match Store::open(log).await {
        Err(e) => e,
        Ok(_) => panic!("startup must fail on a log gap"),
    };
    assert!(
        format!("{err:?}").contains("gap"),
        "a hole in the log must fail startup, not silently skip: {err:?}"
    );
}

// ── partitioned delta logs ──────────────────────────────────────────────────

fn delta_row(table_id: Uuid, version: i64) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "table_id": table_id,
        "commit_version": version,
    }))
    .unwrap()
}

#[tokio::test]
async fn a_delta_commit_conflict_is_the_object_key_not_a_constraint() {
    let log = Arc::new(MemLog::default());
    let store = Store::open(log).await.unwrap();
    let t = Uuid::now_v7();

    store.delta.append(t, 0, delta_row(t, 0)).await.unwrap();
    let err = store.delta.append(t, 0, delta_row(t, 0)).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::CommitVersionConflict);
}

/// The point of partitioning: commits to different tables do not contend, and
/// neither advances the metastore log.
#[tokio::test]
async fn commits_to_different_tables_do_not_serialise_against_each_other() {
    let log = Arc::new(MemLog::default());
    let store = Store::open(log.clone()).await.unwrap();
    let (a, b) = (Uuid::now_v7(), Uuid::now_v7());

    for v in 0..5 {
        store.delta.append(a, v, delta_row(a, v)).await.unwrap();
        store.delta.append(b, v, delta_row(b, v)).await.unwrap();
    }

    assert_eq!(store.delta.latest_version(a).await.unwrap(), Some(4));
    assert_eq!(store.delta.latest_version(b).await.unwrap(), Some(4));
    assert_eq!(
        store.snapshot().await.version,
        0,
        "delta commits must not touch the metastore log"
    );

    // And a cold replica is unaffected by the volume of commit history.
    let cold = Store::open(log).await.unwrap();
    assert_eq!(cold.snapshot().await.version, 0);
    assert_eq!(cold.delta.latest_version(a).await.unwrap(), Some(4));
}

#[tokio::test]
async fn version_ranges_are_inclusive_at_both_ends() {
    let log = Arc::new(MemLog::default());
    let store = Store::open(log).await.unwrap();
    let t = Uuid::now_v7();
    for v in 0..10 {
        store.delta.append(t, v, delta_row(t, v)).await.unwrap();
    }

    assert_eq!(
        store.delta.versions(t, Some(3), Some(6)).await.unwrap(),
        vec![3, 4, 5, 6],
        "matches SQL's commit_version >= $2 AND commit_version <= $3"
    );
    assert_eq!(store.delta.versions(t, None, None).await.unwrap().len(), 10);
    assert_eq!(store.delta.versions(t, Some(0), None).await.unwrap().len(), 10);
}

#[tokio::test]
async fn a_table_with_no_commits_has_no_latest_version() {
    let log = Arc::new(MemLog::default());
    let store = Store::open(log).await.unwrap();
    assert_eq!(store.delta.latest_version(Uuid::now_v7()).await.unwrap(), None);
}

/// The hint is an optimisation, never a source of truth: a replica that never
/// saw another's commits must still find them.
#[tokio::test]
async fn a_stale_latest_hint_is_corrected_by_listing() {
    let log = Arc::new(MemLog::default());
    let a = Store::open(log.clone()).await.unwrap();
    let b = Store::open(log).await.unwrap();
    let t = Uuid::now_v7();

    a.delta.append(t, 0, delta_row(t, 0)).await.unwrap();
    assert_eq!(b.delta.latest_version(t).await.unwrap(), Some(0));

    // b now holds hint=0; a commits further without telling it.
    a.delta.append(t, 1, delta_row(t, 1)).await.unwrap();
    a.delta.append(t, 2, delta_row(t, 2)).await.unwrap();
    assert_eq!(
        b.delta.latest_version(t).await.unwrap(),
        Some(2),
        "a stale hint must be corrected, not trusted"
    );
}

/// Partition keys share the `_uc_log/` prefix with metastore commits. If the
/// main replay did not exclude nested keys, a table's commits would be applied
/// as metadata actions.
#[tokio::test]
async fn delta_partitions_do_not_corrupt_metastore_replay() {
    let log = Arc::new(MemLog::default());
    let store = Store::open(log.clone()).await.unwrap();
    put_catalog(&store, "alpha").await.unwrap();

    let t = Uuid::now_v7();
    for v in 0..3 {
        store.delta.append(t, v, delta_row(t, v)).await.unwrap();
    }

    let cold = Store::open(log).await.unwrap();
    let snap = cold.snapshot().await;
    assert_eq!(snap.version, 1, "only the catalog commit counts");
    assert!(snap.get_by_natural_key(EntityKind::Catalog, "alpha").is_some());
}

// ── paginated backends ──────────────────────────────────────────────────────

/// Mimics S3 ListObjectsV2, which caps a response at 1000 keys. Every listing
/// here returns at most `page` keys, so any caller that treats one call as the
/// complete set gets a silently short answer.
///
/// Tail truncation is the dangerous shape: keys 1..N of a longer log are
/// perfectly contiguous, so the gap check cannot see it. Before
/// `list_all_after`, all three of these returned wrong answers with no error.
struct TruncatingLog {
    inner: MemLog,
    page: usize,
}

#[async_trait::async_trait]
impl ObjectLog for TruncatingLog {
    async fn put_if_absent(&self, key: &str, body: Vec<u8>) -> Result<PutResult, UcError> {
        self.inner.put_if_absent(key, body).await
    }
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, UcError> {
        self.inner.get(key).await
    }
    async fn list_after(&self, prefix: &str, start_after: &str) -> Result<Vec<String>, UcError> {
        let mut keys = self.inner.list_after(prefix, start_after).await?;
        keys.truncate(self.page);
        Ok(keys)
    }
    async fn put(&self, key: &str, body: Vec<u8>) -> Result<(), UcError> {
        self.inner.put(key, body).await
    }
}

fn truncating(page: usize) -> Arc<TruncatingLog> {
    Arc::new(TruncatingLog { inner: MemLog::default(), page })
}

#[tokio::test]
async fn main_log_replays_fully_against_a_paginating_backend() {
    let log = truncating(10);
    let store = Store::open(log.clone()).await.unwrap();
    for i in 0..25 {
        put_catalog(&store, &format!("cat{i:03}")).await.unwrap();
    }

    let cold = Store::open(log).await.unwrap();
    let snap = cold.snapshot().await;
    assert_eq!(snap.version, 25, "replay must page, not stop at the first page");
    assert!(snap.get_by_natural_key(EntityKind::Catalog, "cat024").is_some());
}

#[tokio::test]
async fn delta_latest_version_is_correct_against_a_paginating_backend() {
    let log = truncating(10);
    let store = Store::open(log.clone()).await.unwrap();
    let t = Uuid::now_v7();
    for v in 0..25 {
        store.delta.append(t, v, delta_row(t, v)).await.unwrap();
    }

    // A cold replica has no hint, so it lists the whole partition from zero --
    // the case a single page would truncate. A short answer here would make a
    // Delta client commit at a version that already exists.
    let cold = Store::open(log).await.unwrap();
    assert_eq!(cold.delta.latest_version(t).await.unwrap(), Some(24));
    assert_eq!(cold.delta.versions(t, None, None).await.unwrap().len(), 25);
}

#[tokio::test]
async fn a_backend_that_ignores_start_after_is_refused_not_looped_forever() {
    struct IgnoresStartAfter(MemLog);

    #[async_trait::async_trait]
    impl ObjectLog for IgnoresStartAfter {
        async fn put_if_absent(&self, k: &str, b: Vec<u8>) -> Result<PutResult, UcError> {
            self.0.put_if_absent(k, b).await
        }
        async fn get(&self, k: &str) -> Result<Option<Vec<u8>>, UcError> {
            self.0.get(k).await
        }
        async fn list_after(&self, prefix: &str, _after: &str) -> Result<Vec<String>, UcError> {
            self.0.list_after(prefix, "").await
        }
        async fn put(&self, k: &str, b: Vec<u8>) -> Result<(), UcError> {
            self.0.put(k, b).await
        }
    }

    let seed = MemLog::default();
    seed.put_if_absent(&action::commit_key(1), b"{}".to_vec()).await.unwrap();
    let log = Arc::new(IgnoresStartAfter(seed));

    match Store::open(log).await {
        Err(e) => assert!(
            format!("{e:?}").contains("start_after"),
            "must name the broken contract: {e:?}"
        ),
        Ok(_) => panic!("a backend that ignores start_after must be refused"),
    }
}

// ── natural keys vs the real schema ─────────────────────────────────────────
//
// These pin `natural_key_for` to the UNIQUE constraints actually declared in
// migrations/sqlite/20240001_initial_schema.sql. Getting one wrong is silent:
// the store simply enforces a different constraint than SQLite did, or none.

#[test]
fn user_is_keyed_on_name_not_email() {
    // uc_users: `name TEXT NOT NULL UNIQUE`, `email TEXT` (nullable, no
    // constraint). Keying on email would lose the real uniqueness check and
    // drop every user with a null email out of the index entirely.
    let with_email = serde_json::json!({"name": "ada", "email": "ada@example.com"});
    let without = serde_json::json!({"name": "ada"});
    assert_eq!(natural_key_for(EntityKind::User, &with_email).as_deref(), Some("ada"));
    assert_eq!(
        natural_key_for(EntityKind::User, &without).as_deref(),
        Some("ada"),
        "a user with no email must still be indexed"
    );
}

#[test]
fn columns_are_keyed_by_table_and_ordinal() {
    // UNIQUE(table_id, ordinal_position)
    let t = Uuid::now_v7();
    let col = |n: i64| serde_json::json!({"table_id": t, "ordinal_position": n});
    let k = |n: i64| natural_key_for(EntityKind::Column, &col(n)).unwrap();

    assert_ne!(k(0), k(1));
    assert!(k(2) < k(10), "ordinals must sort numerically, not as text");
    assert!(k(9) < k(10));
}

#[test]
fn properties_are_keyed_by_entity_and_key() {
    // UNIQUE(entity_id, entity_type, property_key)
    let e = Uuid::now_v7();
    let p = |ty: &str, key: &str| {
        serde_json::json!({"entity_id": e, "entity_type": ty, "property_key": key})
    };
    let k = |ty: &str, key: &str| natural_key_for(EntityKind::Property, &p(ty, key)).unwrap();

    assert_ne!(k("TABLE", "a"), k("TABLE", "b"));
    assert_ne!(
        k("TABLE", "a"),
        k("SCHEMA", "a"),
        "the same key on different entity types must not collide"
    );
}

#[test]
fn entities_without_a_unique_constraint_are_id_addressed_only() {
    // Nothing in the schema constrains these, so inventing a key here would
    // reject writes SQLite accepts today.
    for kind in [
        EntityKind::Metastore,
        EntityKind::FunctionParameter,
        EntityKind::ModelVersion, // its (registered_model_id, version) index is not UNIQUE
        EntityKind::StagingTable,
        EntityKind::Dependency,
        EntityKind::CasbinRule,
    ] {
        let body = serde_json::json!({"name": "x", "version": 1, "schema_id": Uuid::nil()});
        assert_eq!(natural_key_for(kind, &body), None, "{kind:?} has no UNIQUE");
    }
}

#[test]
fn delta_commits_are_not_snapshot_entities() {
    // They live in per-table partitions where the version is the object key.
    let body = serde_json::json!({"table_id": Uuid::nil(), "commit_version": 3});
    assert_eq!(natural_key_for(EntityKind::DeltaCommit, &body), None);
}

/// NUL separation is unambiguous only while every component *except the last*
/// is NUL-free. That is not a property of the encoding, it is a precondition
/// on the inputs, so it is worth stating exactly rather than assuming.
///
/// It holds because user-supplied text is always the final component: names sit
/// last in every two-part key, and Property's middle component (`entity_type`)
/// is an internal literal -- 'table', 'schema', 'catalog' -- never client input.
/// Should a variable-length user-supplied field ever move out of last position,
/// this encoding needs escaping or length prefixes.
#[test]
fn nul_separation_is_ambiguous_if_a_non_final_component_contains_nul() {
    let key = |ty: &str, k: &str| {
        natural_key_for(
            EntityKind::Property,
            &serde_json::json!({"entity_id": "e", "entity_type": ty, "property_key": k}),
        )
    };
    // A NUL in the middle component collides. Unreachable today, but real.
    assert_eq!(key("A\u{0}B", "c"), key("A", "B\u{0}c"));

    // A NUL in the *final* component is harmless: nothing follows it to absorb.
    assert_ne!(key("A", "b\u{0}c"), key("A", "b"));
}

#[test]
fn padded_ints_order_numerically_across_zero() {
    let mut v = [5i64, -1, 0, 10, -10, i64::MAX, i64::MIN, 1];
    let mut by_pad = v;
    by_pad.sort_by_key(|n| pad_i64(*n));
    v.sort();
    assert_eq!(by_pad, v, "lexicographic order of pad_i64 must match numeric order");
}

// ── bounded range listings ──────────────────────────────────────────────────

/// Counts listing calls so a bounded read cannot quietly page the whole
/// partition.
struct CountingLog {
    inner: MemLog,
    page: usize,
    lists: std::sync::atomic::AtomicUsize,
}

#[async_trait::async_trait]
impl ObjectLog for CountingLog {
    async fn put_if_absent(&self, k: &str, b: Vec<u8>) -> Result<PutResult, UcError> {
        self.inner.put_if_absent(k, b).await
    }
    async fn get(&self, k: &str) -> Result<Option<Vec<u8>>, UcError> {
        self.inner.get(k).await
    }
    async fn list_after(&self, prefix: &str, after: &str) -> Result<Vec<String>, UcError> {
        self.lists.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let mut keys = self.inner.list_after(prefix, after).await?;
        keys.truncate(self.page);
        Ok(keys)
    }
    async fn put(&self, k: &str, b: Vec<u8>) -> Result<(), UcError> {
        self.inner.put(k, b).await
    }
}

#[tokio::test]
async fn a_bounded_range_does_not_page_the_whole_partition() {
    let log = Arc::new(CountingLog {
        inner: MemLog::default(),
        page: 10,
        lists: std::sync::atomic::AtomicUsize::new(0),
    });
    let store = Store::open(log.clone()).await.unwrap();
    let t = Uuid::now_v7();
    for v in 0..200 {
        store.delta.append(t, v, delta_row(t, v)).await.unwrap();
    }

    log.lists.store(0, std::sync::atomic::Ordering::SeqCst);
    let got = store.delta.versions(t, Some(5), Some(10)).await.unwrap();
    let pages = log.lists.load(std::sync::atomic::Ordering::SeqCst);

    assert_eq!(got, vec![5, 6, 7, 8, 9, 10]);
    assert!(
        pages <= 2,
        "reading 6 of 200 commits took {pages} listings; the range bound is not applied"
    );
}

// ── ported repos that tighten SQL behaviour ─────────────────────────────────
//
// These reach through `repos::*`, which resolves to the SQL bodies unless the
// `logstore` feature selects the ported ones — so they only compile there.
#[cfg(feature = "logstore")]
mod ported {
    use super::*;


    /// The SQL get_or_init is a read-then-insert with nothing between, and
    /// uc_metastore has no UNIQUE, so two replicas starting together could both
    /// insert. Here the check is inside the commit closure, so the loser re-runs
    /// it and adopts the winner's row.
    #[tokio::test]
    async fn concurrent_metastore_init_yields_one_row() {
        use crate::repos::metastore;

        let log = Arc::new(MemLog::default());
        let a = Store::open(log.clone()).await.unwrap();
        let b = Store::open(log.clone()).await.unwrap();

        // Both replicas observe an empty metastore before either writes.
        assert!(a.snapshot().await.iter(EntityKind::Metastore).next().is_none());
        assert!(b.snapshot().await.iter(EntityKind::Metastore).next().is_none());

        let first = metastore::get_or_init(&a, "unity-catalog").await.unwrap();
        let second = metastore::get_or_init(&b, "unity-catalog").await.unwrap();

        assert_eq!(first.id, second.id, "the loser must adopt the winner's row");

        let cold = Store::open(log).await.unwrap();
        assert_eq!(cold.snapshot().await.iter(EntityKind::Metastore).count(), 1);
    }

    #[tokio::test]
    async fn get_or_init_is_idempotent_and_commits_nothing_when_present() {
        use crate::repos::metastore;

        let log = Arc::new(MemLog::default());
        let store = Store::open(log).await.unwrap();

        let first = metastore::get_or_init(&store, "unity-catalog").await.unwrap();
        let v = store.snapshot().await.version;
        let again = metastore::get_or_init(&store, "unity-catalog").await.unwrap();

        assert_eq!(first.id, again.id);
        assert_eq!(store.snapshot().await.version, v, "a no-op must not append");
    }

    /// The SQL replace is DELETE + N INSERTs and documents itself as requiring an
    /// enclosing transaction. As one commit, a reader can never observe the
    /// entity mid-replace with its properties missing.
    #[tokio::test]
    async fn property_replace_is_one_atomic_commit() {
        use crate::repos::property;

        let log = Arc::new(MemLog::default());
        let store = Store::open(log.clone()).await.unwrap();
        let e = Uuid::now_v7();

        let props = |pairs: &[(&str, &str)]| -> std::collections::HashMap<String, String> {
            pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
        };

        property::replace(&store, e, "table", &props(&[("a", "1"), ("b", "2")]))
            .await
            .unwrap();
        let v1 = store.snapshot().await.version;

        property::replace(&store, e, "table", &props(&[("b", "22"), ("c", "3")]))
            .await
            .unwrap();

        assert_eq!(
            store.snapshot().await.version,
            v1 + 1,
            "delete-then-insert must be a single commit, not one per property"
        );

        let got = property::get_for_entity(&store, e, "table").await.unwrap();
        assert_eq!(got, props(&[("b", "22"), ("c", "3")]));

        // Replay agrees: no intermediate state was ever durable.
        let cold = Store::open(log).await.unwrap();
        assert_eq!(
            property::get_for_entity(&cold, e, "table").await.unwrap(),
            props(&[("b", "22"), ("c", "3")])
        );
    }

    #[tokio::test]
    async fn properties_are_scoped_by_entity_and_type() {
        use crate::repos::property;

        let log = Arc::new(MemLog::default());
        let store = Store::open(log).await.unwrap();
        let (e1, e2) = (Uuid::now_v7(), Uuid::now_v7());
        let one = |k: &str, v: &str| -> std::collections::HashMap<String, String> {
            std::iter::once((k.to_string(), v.to_string())).collect()
        };

        property::replace(&store, e1, "table", &one("k", "table-val")).await.unwrap();
        property::replace(&store, e1, "schema", &one("k", "schema-val")).await.unwrap();
        property::replace(&store, e2, "table", &one("k", "other-entity")).await.unwrap();

        assert_eq!(property::get_for_entity(&store, e1, "table").await.unwrap(), one("k", "table-val"));
        assert_eq!(property::get_for_entity(&store, e1, "schema").await.unwrap(), one("k", "schema-val"));
        assert_eq!(property::get_for_entity(&store, e2, "table").await.unwrap(), one("k", "other-entity"));

        // Deleting one group must not touch the others.
        property::delete_for_entity(&store, e1, "table").await.unwrap();
        assert!(property::get_for_entity(&store, e1, "table").await.unwrap().is_empty());
        assert_eq!(property::get_for_entity(&store, e1, "schema").await.unwrap(), one("k", "schema-val"));
        assert_eq!(property::get_for_entity(&store, e2, "table").await.unwrap(), one("k", "other-entity"));
    }

}


// ── multi-replica read freshness ────────────────────────────────────────────

/// Writes converge without help: the conditional PUT forces a stale writer to
/// replay before it can commit. This is what makes multi-replica *writes* safe
/// even with no refresh at all.
#[tokio::test]
async fn a_stale_replica_catches_up_when_it_writes() {
    let log = Arc::new(MemLog::default());
    let a = Store::open(log.clone()).await.unwrap();
    let b = Store::open(log).await.unwrap();

    put_catalog(&a, "written-by-a").await.unwrap();
    assert_eq!(b.snapshot().await.version, 0, "b has not seen it yet");

    put_catalog(&b, "written-by-b").await.unwrap();
    let snap = b.snapshot().await;
    assert_eq!(snap.version, 2);
    assert!(
        snap.get_by_natural_key(EntityKind::Catalog, "written-by-a").is_some(),
        "committing must have pulled in a's work"
    );
}

/// Reads do not converge on their own — this is the gap `catch_up` exists to
/// bound, and the reason multi-replica reads are only eventually consistent.
#[tokio::test]
async fn a_read_only_replica_is_stale_until_it_refreshes() {
    let log = Arc::new(MemLog::default());
    let writer = Store::open(log.clone()).await.unwrap();
    let reader = Store::open(log).await.unwrap();

    put_catalog(&writer, "fresh").await.unwrap();

    assert!(
        reader
            .snapshot()
            .await
            .get_by_natural_key(EntityKind::Catalog, "fresh")
            .is_none(),
        "a reader that never writes and never refreshes stays stale"
    );

    reader.catch_up().await.unwrap();

    assert!(
        reader
            .snapshot()
            .await
            .get_by_natural_key(EntityKind::Catalog, "fresh")
            .is_some(),
        "refreshing must pick up another replica's commits"
    );
}

#[tokio::test]
async fn refreshing_repeatedly_is_harmless() {
    let log = Arc::new(MemLog::default());
    let store = Store::open(log).await.unwrap();
    put_catalog(&store, "one").await.unwrap();

    for _ in 0..5 {
        store.catch_up().await.unwrap();
    }
    let snap = store.snapshot().await;
    assert_eq!(snap.version, 1, "a no-op refresh must not advance the version");
    assert_eq!(snap.scan(EntityKind::Catalog, None, 10).len(), 1);
}
