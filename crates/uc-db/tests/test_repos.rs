/// Integration tests for uc-db repositories using in-memory SQLite.
/// These test every repo method directly — no HTTP layer.
use uc_db::{
    AnyPool,
    models::{
        catalog::CatalogRow, schema::SchemaRow, table::{TableRow, ColumnRow},
        volume::VolumeRow, credential::CredentialRow, external_location::ExternalLocationRow,
        user::UserRow, staging::StagingTableRow, delta::DeltaCommitRow,
        metastore::MetastoreRow,
    },
    repos::{
        CatalogRepo, SchemaRepo, TableRepo, VolumeRepo, CredentialRepo,
        ExternalLocationRepo, UserRepo, StagingTableRepo, DeltaCommitRepo,
        MetastoreRepo, PropertyRepo,
    },
    pool::run_migrations,
};
use uuid::Uuid;

async fn setup_pool() -> AnyPool {
    let pool = AnyPool::connect("sqlite::memory:").await.expect("in-memory pool");
    run_migrations(&pool).await.expect("migrations");
    pool
}

fn now() -> i64 { chrono::Utc::now().timestamp_millis() }

// ── MetastoreRepo ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn metastore_get_or_init() {
    let pool = setup_pool().await;
    let m1 = MetastoreRepo::get_or_init(&pool, "test").await.unwrap();
    let m2 = MetastoreRepo::get_or_init(&pool, "test").await.unwrap();
    assert_eq!(m1.id, m2.id, "get_or_init must be idempotent");
    let got = MetastoreRepo::get(&pool).await.unwrap();
    assert_eq!(got.id, m1.id);
}

// ── CatalogRepo ───────────────────────────────────────────────────────────────

#[tokio::test]
async fn catalog_crud() {
    let pool = setup_pool().await;
    let id = Uuid::new_v4();
    let created = CatalogRepo::create(&pool, id, "cat1", Some("comment"), None, None, None, now()).await.unwrap();
    assert_eq!(created.name, "cat1");
    assert_eq!(created.id, id);

    let fetched = CatalogRepo::get_by_name(&pool, "cat1").await.unwrap();
    assert_eq!(fetched.id, id);

    let by_id = CatalogRepo::get_by_id(&pool, id).await.unwrap();
    assert_eq!(by_id.name, "cat1");

    let updated = CatalogRepo::update(&pool, "cat1", None, Some("new comment"), None, None, now()).await.unwrap();
    assert_eq!(updated.comment.as_deref(), Some("new comment"));

    CatalogRepo::delete(&pool, "cat1").await.unwrap();
    assert!(CatalogRepo::get_by_name(&pool, "cat1").await.is_err());
}

#[tokio::test]
async fn catalog_list_pagination() {
    let pool = setup_pool().await;
    for i in 0..5 {
        CatalogRepo::create(&pool, Uuid::new_v4(), &format!("cat_{:02}", i), None, None, None, None, now()).await.unwrap();
    }
    let (page1, token) = CatalogRepo::list(&pool, None, 3).await.unwrap();
    assert_eq!(page1.len(), 3);
    assert!(token.is_some());
    let (page2, token2) = CatalogRepo::list(&pool, token.as_deref(), 3).await.unwrap();
    assert_eq!(page2.len(), 2);
    assert!(token2.is_none());
}

#[tokio::test]
async fn catalog_not_found_returns_error() {
    let pool = setup_pool().await;
    assert!(CatalogRepo::get_by_name(&pool, "missing").await.is_err());
    assert!(CatalogRepo::get_by_id(&pool, Uuid::new_v4()).await.is_err());
}

// ── SchemaRepo ────────────────────────────────────────────────────────────────

async fn make_catalog(pool: &AnyPool, name: &str) -> Uuid {
    let id = Uuid::new_v4();
    CatalogRepo::create(pool, id, name, None, None, None, None, now()).await.unwrap();
    id
}

#[tokio::test]
async fn schema_crud() {
    let pool = setup_pool().await;
    let cat_id = make_catalog(&pool, "schema_cat").await;
    let id = Uuid::new_v4();
    let s = SchemaRepo::create(&pool, id, cat_id, "sch1", None, None, None, None, now()).await.unwrap();
    assert_eq!(s.name, "sch1");

    let fetched = SchemaRepo::get_by_full_name(&pool, "schema_cat", "sch1").await.unwrap();
    assert_eq!(fetched.id, id);

    let by_id = SchemaRepo::get_by_id(&pool, id).await.unwrap();
    assert_eq!(by_id.catalog_id, cat_id);

    let updated = SchemaRepo::update(&pool, id, Some("sch_renamed"), None, None, None, now()).await.unwrap();
    assert_eq!(updated.name, "sch_renamed");

    SchemaRepo::delete(&pool, id).await.unwrap();
    assert!(SchemaRepo::get_by_id(&pool, id).await.is_err());
}

#[tokio::test]
async fn schema_list() {
    let pool = setup_pool().await;
    let cat_id = make_catalog(&pool, "schema_list_cat").await;
    SchemaRepo::create(&pool, Uuid::new_v4(), cat_id, "a", None, None, None, None, now()).await.unwrap();
    SchemaRepo::create(&pool, Uuid::new_v4(), cat_id, "b", None, None, None, None, now()).await.unwrap();
    let (rows, _) = SchemaRepo::list(&pool, cat_id, None, 10).await.unwrap();
    assert_eq!(rows.len(), 2);
    let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    assert!(names.contains(&"a") && names.contains(&"b"));
}

// ── TableRepo ─────────────────────────────────────────────────────────────────

async fn make_schema(pool: &AnyPool, cat: &str, sch: &str) -> (Uuid, Uuid) {
    let cat_id = make_catalog(pool, cat).await;
    let sch_id = Uuid::new_v4();
    SchemaRepo::create(pool, sch_id, cat_id, sch, None, None, None, None, now()).await.unwrap();
    (cat_id, sch_id)
}

#[tokio::test]
async fn table_crud_with_columns() {
    let pool = setup_pool().await;
    let (_, sch_id) = make_schema(&pool, "tbl_cat", "tbl_sch").await;

    let id = Uuid::new_v4();
    let row = TableRow {
        id, schema_id: sch_id, name: "t1".into(), r#type: "EXTERNAL".into(),
        owner: None, created_at: now(), created_by: None, updated_at: None, updated_by: None,
        data_source_format: Some("DELTA".into()), comment: None, url: Some("/tmp/t1".into()),
        column_count: Some(1), view_definition: None,
        uniform_iceberg_metadata_location: None,
        uniform_iceberg_converted_delta_version: None,
        uniform_iceberg_converted_delta_timestamp: None,
    };
    let created = TableRepo::create(&pool, &row).await.unwrap();
    assert_eq!(created.name, "t1");

    let col = ColumnRow {
        id: Uuid::new_v4(), table_id: id, name: "col1".into(),
        ordinal_position: 0, type_text: "int".into(), type_json: "{}".into(),
        type_name: "INT".into(), type_precision: None, type_scale: None,
        type_interval_type: None, nullable: false, comment: None, partition_index: None,
    };
    TableRepo::insert_columns(&pool, &[col]).await.unwrap();

    let cols = TableRepo::get_columns(&pool, id).await.unwrap();
    assert_eq!(cols.len(), 1);
    assert_eq!(cols[0].name, "col1");

    let by_id = TableRepo::get_by_id(&pool, id).await.unwrap();
    assert_eq!(by_id.name, "t1");

    let by_name = TableRepo::get_by_schema_and_name(&pool, sch_id, "t1").await.unwrap();
    assert_eq!(by_name.id, id);

    let (rows, _) = TableRepo::list(&pool, sch_id, None, 10).await.unwrap();
    assert_eq!(rows.len(), 1);

    TableRepo::delete_columns(&pool, id).await.unwrap();
    let cols_after = TableRepo::get_columns(&pool, id).await.unwrap();
    assert!(cols_after.is_empty());

    TableRepo::delete(&pool, id).await.unwrap();
    assert!(TableRepo::get_by_id(&pool, id).await.is_err());
}

// ── VolumeRepo ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn volume_crud() {
    let pool = setup_pool().await;
    let (_, sch_id) = make_schema(&pool, "vol_cat", "vol_sch").await;
    let id = Uuid::new_v4();
    let row = VolumeRow {
        id, schema_id: sch_id, name: "v1".into(), comment: None,
        storage_location: Some("/tmp/v1".into()), owner: None,
        created_at: now(), created_by: None, updated_at: None, updated_by: None,
        volume_type: "EXTERNAL".into(),
    };
    VolumeRepo::create(&pool, &row).await.unwrap();
    VolumeRepo::get_by_id(&pool, id).await.unwrap();
    VolumeRepo::get_by_schema_and_name(&pool, sch_id, "v1").await.unwrap();
    let (vols, _) = VolumeRepo::list(&pool, sch_id, None, 10).await.unwrap();
    assert_eq!(vols.len(), 1);
    VolumeRepo::update(&pool, id, None, Some("updated"), None, now(), None).await.unwrap();
    VolumeRepo::delete(&pool, id).await.unwrap();
    assert!(VolumeRepo::get_by_id(&pool, id).await.is_err());
}

// ── PropertyRepo ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn property_replace_and_get() {
    let pool = setup_pool().await;
    let entity_id = Uuid::new_v4();
    let mut props = std::collections::HashMap::new();
    props.insert("k1".to_string(), "v1".to_string());
    props.insert("k2".to_string(), "v2".to_string());

    PropertyRepo::replace(&pool, entity_id, "catalog", &props).await.unwrap();
    let got = PropertyRepo::get_for_entity(&pool, entity_id, "catalog").await.unwrap();
    assert_eq!(got.get("k1").map(|s| s.as_str()), Some("v1"));
    assert_eq!(got.get("k2").map(|s| s.as_str()), Some("v2"));

    // Replace with new set (old values gone)
    let mut new_props = std::collections::HashMap::new();
    new_props.insert("k3".to_string(), "v3".to_string());
    PropertyRepo::replace(&pool, entity_id, "catalog", &new_props).await.unwrap();
    let got2 = PropertyRepo::get_for_entity(&pool, entity_id, "catalog").await.unwrap();
    assert!(!got2.contains_key("k1"));
    assert_eq!(got2.get("k3").map(|s| s.as_str()), Some("v3"));

    PropertyRepo::delete_for_entity(&pool, entity_id, "catalog").await.unwrap();
    let empty = PropertyRepo::get_for_entity(&pool, entity_id, "catalog").await.unwrap();
    assert!(empty.is_empty());
}

// ── UserRepo ──────────────────────────────────────────────────────────────────

#[tokio::test]
async fn user_crud() {
    let pool = setup_pool().await;
    let id = Uuid::new_v4();
    UserRepo::create(&pool, id, "alice@x.com", Some("alice@x.com"), None, "ENABLED", now()).await.unwrap();

    let by_id = UserRepo::get_by_id(&pool, id).await.unwrap();
    assert_eq!(by_id.name, "alice@x.com");
    assert!(by_id.is_enabled());

    let by_name = UserRepo::get_by_name(&pool, "alice@x.com").await.unwrap();
    assert_eq!(by_name.id, id);

    let by_email = UserRepo::get_by_email(&pool, "alice@x.com").await.unwrap();
    assert!(by_email.is_some());

    let (users, _) = UserRepo::list(&pool, None, 10).await.unwrap();
    assert!(!users.is_empty());

    let updated = UserRepo::update(&pool, id, None, None, Some("DISABLED"), now()).await.unwrap();
    assert!(!updated.is_enabled());

    UserRepo::delete(&pool, id).await.unwrap();
    assert!(UserRepo::get_by_id(&pool, id).await.is_err());
}

#[tokio::test]
async fn user_get_by_email_unknown_returns_none() {
    let pool = setup_pool().await;
    let result = UserRepo::get_by_email(&pool, "nobody@x.com").await.unwrap();
    assert!(result.is_none());
}

// ── CredentialRepo ────────────────────────────────────────────────────────────

#[tokio::test]
async fn credential_crud() {
    let pool = setup_pool().await;
    let id = Uuid::new_v4();
    let row = CredentialRow {
        id, name: "cred1".into(), credential_type: "AWS_IAM_ROLE".into(),
        credential: r#"{"role_arn":"arn:aws:iam::123:role/r"}"#.into(),
        purpose: "AWS_IAM_ROLE".into(), comment: None, owner: None,
        created_at: now(), created_by: None, updated_at: None, updated_by: None,
    };
    CredentialRepo::create(&pool, &row).await.unwrap();
    CredentialRepo::get_by_name(&pool, "cred1").await.unwrap();
    CredentialRepo::get_by_id(&pool, id).await.unwrap();
    let (creds, _) = CredentialRepo::list(&pool, None, 10).await.unwrap();
    assert!(!creds.is_empty());
    CredentialRepo::delete(&pool, "cred1").await.unwrap();
    assert!(CredentialRepo::get_by_name(&pool, "cred1").await.is_err());
}

// ── ExternalLocationRepo ──────────────────────────────────────────────────────

#[tokio::test]
async fn external_location_crud() {
    let pool = setup_pool().await;
    let cred_id = Uuid::new_v4();
    let cred_row = CredentialRow {
        id: cred_id, name: "el_cred".into(), credential_type: "AWS_IAM_ROLE".into(),
        credential: "{}".into(), purpose: "AWS_IAM_ROLE".into(),
        comment: None, owner: None, created_at: now(), created_by: None, updated_at: None, updated_by: None,
    };
    CredentialRepo::create(&pool, &cred_row).await.unwrap();

    let id = Uuid::new_v4();
    let row = ExternalLocationRow {
        id, name: "el1".into(), url: "s3://bucket/path".into(),
        comment: None, owner: None, credential_id: cred_id,
        created_at: Some(now()), created_by: None, updated_at: None, updated_by: None,
    };
    ExternalLocationRepo::create(&pool, &row).await.unwrap();
    ExternalLocationRepo::get_by_name(&pool, "el1").await.unwrap();
    let (locs, _) = ExternalLocationRepo::list(&pool, None, 10).await.unwrap();
    assert!(!locs.is_empty());

    // find_by_path_prefix
    let found = ExternalLocationRepo::find_by_path_prefix(&pool, "s3://bucket/path/subdir").await.unwrap();
    assert!(found.is_some());
    let not_found = ExternalLocationRepo::find_by_path_prefix(&pool, "s3://other-bucket/x").await.unwrap();
    assert!(not_found.is_none());

    ExternalLocationRepo::delete(&pool, "el1").await.unwrap();
    assert!(ExternalLocationRepo::get_by_name(&pool, "el1").await.is_err());
}

// ── StagingTableRepo ──────────────────────────────────────────────────────────

#[tokio::test]
async fn staging_table_create_get_commit() {
    let pool = setup_pool().await;
    let (_, sch_id) = make_schema(&pool, "stg_cat", "stg_sch").await;
    let id = Uuid::new_v4();
    let row = StagingTableRow {
        id, schema_id: sch_id, name: "stg1".into(),
        staging_location: "/tmp/staging/stg1".into(),
        created_at: now(), created_by: None, accessed_at: now(),
        stage_committed: false, stage_committed_at: None,
        purge_state: 0, num_cleanup_retries: 0, last_cleanup_at: None,
    };
    StagingTableRepo::create(&pool, &row).await.unwrap();
    StagingTableRepo::get_by_id(&pool, id).await.unwrap();
    StagingTableRepo::get_by_location(&pool, "/tmp/staging/stg1").await.unwrap();

    StagingTableRepo::mark_committed(&pool, id, now()).await.unwrap();
    let committed = StagingTableRepo::get_by_id(&pool, id).await.unwrap();
    assert!(committed.stage_committed);
}

// ── DeltaCommitRepo ───────────────────────────────────────────────────────────

#[tokio::test]
async fn delta_commit_insert_list_latest() {
    let pool = setup_pool().await;
    let (_, sch_id) = make_schema(&pool, "dc_cat", "dc_sch").await;
    let tbl_id = Uuid::new_v4();
    let tbl_row = TableRow {
        id: tbl_id, schema_id: sch_id, name: "dc_t".into(), r#type: "EXTERNAL".into(),
        owner: None, created_at: now(), created_by: None, updated_at: None, updated_by: None,
        data_source_format: Some("DELTA".into()), comment: None, url: None,
        column_count: None, view_definition: None,
        uniform_iceberg_metadata_location: None,
        uniform_iceberg_converted_delta_version: None,
        uniform_iceberg_converted_delta_timestamp: None,
    };
    TableRepo::create(&pool, &tbl_row).await.unwrap();

    for v in [1i64, 2, 3] {
        let row = DeltaCommitRow {
            id: Uuid::new_v4(), table_id: tbl_id, commit_version: v,
            commit_filename: format!("{:020}.json", v),
            commit_filesize: 100 * v, commit_file_modification_timestamp: v * 1000,
            commit_timestamp: v * 1000, is_backfilled_latest_commit: false,
        };
        DeltaCommitRepo::insert(&pool, &row).await.unwrap();
    }

    let all = DeltaCommitRepo::list_for_table(&pool, tbl_id, None, None).await.unwrap();
    assert_eq!(all.len(), 3);
    assert_eq!(all[0].commit_version, 1);
    assert_eq!(all[2].commit_version, 3);

    let range = DeltaCommitRepo::list_for_table(&pool, tbl_id, Some(2), Some(3)).await.unwrap();
    assert_eq!(range.len(), 2);

    let latest = DeltaCommitRepo::latest_version(&pool, tbl_id).await.unwrap();
    assert_eq!(latest, Some(3));

    // Duplicate version → CommitVersionConflict
    let dup = DeltaCommitRow {
        id: Uuid::new_v4(), table_id: tbl_id, commit_version: 2,
        commit_filename: "dup.json".into(), commit_filesize: 50,
        commit_file_modification_timestamp: 1000, commit_timestamp: 1000,
        is_backfilled_latest_commit: false,
    };
    let result = DeltaCommitRepo::insert(&pool, &dup).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.code, uc_errors::ErrorCode::CommitVersionConflict);
}
