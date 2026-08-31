//! Store tests against an in-memory `ObjectLog`.
//!
//! The fake enforces the one guarantee the design rests on — `put_if_absent`
//! never overwrites — so a backend that quietly lost that property would fail
//! here rather than in production.

use super::action::{Action, EntityKind};
use super::log::{ObjectLog, PutResult};
use super::*;
use std::collections::BTreeMap;
use std::sync::Mutex;

#[derive(Default)]
struct MemLog {
    objects: Mutex<BTreeMap<String, Vec<u8>>>,
}

#[async_trait::async_trait]
impl ObjectLog for MemLog {
    async fn put_if_absent(&self, key: &str, body: Vec<u8>) -> Result<PutResult, UcError> {
        let mut o = self.objects.lock().unwrap();
        if o.contains_key(key) {
            return Ok(PutResult::AlreadyExists);
        }
        o.insert(key.to_string(), body);
        Ok(PutResult::Created)
    }
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, UcError> {
        Ok(self.objects.lock().unwrap().get(key).cloned())
    }
    async fn list_after(&self, prefix: &str, start_after: &str) -> Result<Vec<String>, UcError> {
        Ok(self
            .objects
            .lock()
            .unwrap()
            .keys()
            .filter(|k| k.starts_with(prefix) && k.as_str() > start_after)
            .cloned()
            .collect())
    }
    async fn put(&self, key: &str, body: Vec<u8>) -> Result<(), UcError> {
        self.objects.lock().unwrap().insert(key.to_string(), body);
        Ok(())
    }
}

use put_catalog_helper as put_catalog;

fn catalog(name: &str) -> serde_json::Value {
    serde_json::json!({ "name": name })
}

pub(super) async fn put_catalog_helper(store: &Store, name: &str) -> Result<Uuid, UcError> {
    let id = Uuid::new_v4();
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
        serde_json::to_vec(&log::LastCheckpoint { version: 2, size: 2 }).unwrap(),
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
        serde_json::to_vec(&log::LastCheckpoint { version: 2, size: 99 }).unwrap(),
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
    log.objects.lock().unwrap().remove(&action::commit_key(1));

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
    let t = Uuid::new_v4();

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
    let (a, b) = (Uuid::new_v4(), Uuid::new_v4());

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
    let t = Uuid::new_v4();
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
    assert_eq!(store.delta.latest_version(Uuid::new_v4()).await.unwrap(), None);
}

/// The hint is an optimisation, never a source of truth: a replica that never
/// saw another's commits must still find them.
#[tokio::test]
async fn a_stale_latest_hint_is_corrected_by_listing() {
    let log = Arc::new(MemLog::default());
    let a = Store::open(log.clone()).await.unwrap();
    let b = Store::open(log).await.unwrap();
    let t = Uuid::new_v4();

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

    let t = Uuid::new_v4();
    for v in 0..3 {
        store.delta.append(t, v, delta_row(t, v)).await.unwrap();
    }

    let cold = Store::open(log).await.unwrap();
    let snap = cold.snapshot().await;
    assert_eq!(snap.version, 1, "only the catalog commit counts");
    assert!(snap.get_by_natural_key(EntityKind::Catalog, "alpha").is_some());
}
