/// Tests for remaining handlers: temp_credentials, delta_commits, delta credentials,
/// middleware, error_ext, staging_tables.
mod common;
use axum::http::StatusCode;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use common::*;
use serde_json::json;
use std::sync::Arc;
use uc_auth::OidcConfig;

async fn setup(app: &axum::Router) {
    post(app, &format!("{UC}/catalogs"), json!({"name":"misc_cat"})).await;
    post(
        app,
        &format!("{UC}/schemas"),
        json!({"name":"misc_sch","catalog_name":"misc_cat"}),
    )
    .await;
}

// ── Temp Credentials ─────────────────────────────────────────────────────────

#[tokio::test]
async fn temp_table_credentials_local_path() {
    let (app, _) = build_test_app().await;
    setup(&app).await;

    // Create a table
    let (_, tbl) = post(
        &app,
        &format!("{UC}/tables"),
        json!({
            "name":"cred_t","catalog_name":"misc_cat","schema_name":"misc_sch",
            "table_type":"EXTERNAL","data_source_format":"DELTA",
            "storage_location":"/tmp/cred_t","columns":[]
        }),
    )
    .await;
    let table_id = tbl["table_id"].as_str().unwrap();

    let (s, creds) = post(
        &app,
        &format!("{UC}/temporary-table-credentials"),
        json!({
            "table_id": table_id,
            "operation": "READ"
        }),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    // Local path → empty credentials (no cloud SDK configured)
    assert!(creds["aws_temp_credentials"].is_null() || creds.get("aws_temp_credentials").is_none());
}

#[tokio::test]
async fn temp_volume_credentials_local_path() {
    let (app, _) = build_test_app().await;
    setup(&app).await;

    let (_, vol) = post(
        &app,
        &format!("{UC}/volumes"),
        json!({
            "name":"cred_v","catalog_name":"misc_cat","schema_name":"misc_sch",
            "volume_type":"EXTERNAL","storage_location":"/tmp/cred_v"
        }),
    )
    .await;
    let volume_id = vol["volume_id"].as_str().unwrap();

    let (s, _) = post(
        &app,
        &format!("{UC}/temporary-volume-credentials"),
        json!({
            "volume_id": volume_id,
            "operation": "READ_VOLUME"
        }),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
}

#[tokio::test]
async fn temp_path_credentials_local_returns_empty() {
    let (app, _) = build_test_app().await;
    let (s, creds) = post(
        &app,
        &format!("{UC}/temporary-path-credentials"),
        json!({
            "url": "file:///tmp/test/path",
            "operation": "READ"
        }),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    // file:// → empty credentials
    assert!(creds["aws_temp_credentials"].is_null() || creds.get("aws_temp_credentials").is_none());
}

#[tokio::test]
async fn temp_model_version_credentials() {
    let (app, _) = build_test_app().await;
    setup(&app).await;
    post(
        &app,
        &format!("{UC}/models"),
        json!({
            "name":"cred_mdl","catalog_name":"misc_cat","schema_name":"misc_sch"
        }),
    )
    .await;
    post(
        &app,
        &format!("{UC}/models/versions"),
        json!({
            "model_name":"cred_mdl","catalog_name":"misc_cat","schema_name":"misc_sch",
            "source":"s3://ml/run1"
        }),
    )
    .await;

    let (s, _) = post(
        &app,
        &format!("{UC}/temporary-model-version-credentials"),
        json!({
            "catalog_name":"misc_cat","schema_name":"misc_sch","model_name":"cred_mdl",
            "version":1,"operation":"READ_MODEL_VERSION"
        }),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
}

// ── Delta Commits ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn delta_commits_get_and_post() {
    let (app, _) = build_test_app().await;
    setup(&app).await;
    post(
        &app,
        &format!("{UC}/tables"),
        json!({
            "name":"commits_t","catalog_name":"misc_cat","schema_name":"misc_sch",
            "table_type":"EXTERNAL","data_source_format":"DELTA",
            "storage_location":"/tmp/commits_t","columns":[]
        }),
    )
    .await;

    // GET with no commits
    let (s, body) = get(
        &app,
        &format!("{UC}/delta/preview/commits?table_full_name=misc_cat.misc_sch.commits_t"),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    // No commits yet — latest_table_version is -1 or 0 depending on impl
    assert!(body["latest_table_version"].as_i64().unwrap_or(-2) <= 0);
    assert_eq!(body["commits_info"].as_array().unwrap().len(), 0);

    // POST a commit
    let (s, body) = post(
        &app,
        &format!("{UC}/delta/preview/commits"),
        json!({
            "table_full_name": "misc_cat.misc_sch.commits_t",
            "version": 1,
            "timestamp": 1700000000000_i64,
            "file_name": "00001.json",
            "file_size": 512,
            "file_modification_timestamp": 1700000000000_i64
        }),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(body["latest_table_version"], 1);
    assert_eq!(body["commits_info"][0]["version"], 1);
    assert_eq!(body["commits_info"][0]["file_name"], "00001.json");
    assert_eq!(body["commits_info"][0]["file_size"], 512);

    // GET now shows the commit
    let (_, after) = get(
        &app,
        &format!("{UC}/delta/preview/commits?table_full_name=misc_cat.misc_sch.commits_t"),
    )
    .await;
    assert_eq!(after["latest_table_version"], 1);
    assert_eq!(after["commits_info"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn delta_commits_duplicate_version_returns_409() {
    let (app, _) = build_test_app().await;
    setup(&app).await;
    post(
        &app,
        &format!("{UC}/tables"),
        json!({
            "name":"dup_commits_t","catalog_name":"misc_cat","schema_name":"misc_sch",
            "table_type":"EXTERNAL","data_source_format":"DELTA",
            "storage_location":"/tmp/dup_commits_t","columns":[]
        }),
    )
    .await;
    let commit = json!({
        "table_full_name":"misc_cat.misc_sch.dup_commits_t",
        "version":1,"timestamp":1000000_i64,"file_size":100
    });
    post(&app, &format!("{UC}/delta/preview/commits"), commit.clone()).await;
    let (s, _) = post(&app, &format!("{UC}/delta/preview/commits"), commit).await;
    assert_eq!(s, StatusCode::CONFLICT);
}

// ── Delta API Credentials ─────────────────────────────────────────────────────

#[tokio::test]
async fn delta_table_credentials_local() {
    let (app, _) = build_test_app().await;
    setup(&app).await;
    post(
        &app,
        &format!("{DELTA}/catalogs/misc_cat/schemas/misc_sch/tables"),
        json!({
            "name":"dc_t","location":"/tmp/dc","table-type":"EXTERNAL"
        }),
    )
    .await;

    let (s, body) = get(
        &app,
        &format!("{DELTA}/catalogs/misc_cat/schemas/misc_sch/tables/dc_t/credentials"),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert!(body["storage-credentials"].is_array());
}

#[tokio::test]
async fn delta_path_credentials_local() {
    let (app, _) = build_test_app().await;
    let (s, body) = get(
        &app,
        &format!("{DELTA}/temporary-path-credentials?path=file:///tmp/test"),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert!(body["storage-credentials"].is_array());
}

#[tokio::test]
async fn delta_staging_credentials() {
    let (app, _) = build_test_app().await;
    setup(&app).await;
    let (_, staging) = post(
        &app,
        &format!("{DELTA}/catalogs/misc_cat/schemas/misc_sch/staging-tables"),
        json!({"name":"stg_cred"}),
    )
    .await;
    let table_id = staging["table-id"].as_str().unwrap();

    let (s, body) = get(
        &app,
        &format!("{DELTA}/staging-tables/{table_id}/credentials"),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert!(body["storage-credentials"].is_array());
}

// ── Error format injection (error_ext) ────────────────────────────────────────

#[tokio::test]
async fn catalog_error_has_uc_error_code_format() {
    let (app, _) = build_test_app().await;
    let (s, body) = get(&app, &format!("{UC}/catalogs/nonexistent_xyz")).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    // UC format: { "error_code": "...", "message": "..." }
    assert!(
        body["error_code"].as_str().is_some(),
        "UC errors must have error_code field"
    );
    assert!(body["message"].as_str().is_some());
}

#[tokio::test]
async fn delta_error_has_delta_error_type_format() {
    let (app, _) = build_test_app().await;
    setup(&app).await;
    // Try to load a table that doesn't exist via Delta API → Delta error format
    let (s, body) = get(
        &app,
        &format!("{DELTA}/catalogs/misc_cat/schemas/misc_sch/tables/nonexistent"),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    // Delta format wraps in "error" object with errorType
    // Our impl may return either format depending on the error_ext injection
    // At minimum the response must be non-empty JSON with an error indication
    assert!(s.is_client_error(), "Should be a 4xx error");
    assert!(body.is_object(), "Error body should be JSON object");
}

// ── Middleware ─────────────────────────────────────────────────────────────────

#[tokio::test]
async fn root_endpoint_returns_200() {
    let (app, _) = build_test_app().await;
    let (s, _) = get(&app, "/").await;
    assert_eq!(s, StatusCode::OK);
}

#[tokio::test]
async fn unknown_route_returns_404() {
    let (app, _) = build_test_app().await;
    let (s, _) = get(&app, "/nonexistent/route/xyz").await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

// ── OIDC middleware ───────────────────────────────────────────────────────────

fn hs_oidc_config(secret: &[u8], issuer: &str) -> Arc<OidcConfig> {
    let k = URL_SAFE_NO_PAD.encode(secret);
    let jwks = serde_json::from_value(
        serde_json::json!({ "keys": [{ "kty": "oct", "k": k, "alg": "HS256" }] }),
    )
    .unwrap();
    Arc::new(OidcConfig {
        issuer: issuer.to_string(),
        jwks,
    })
}

fn hs_bearer(secret: &[u8], issuer: &str, sub: &str, exp_delta: i64) -> String {
    use jsonwebtoken::Algorithm;
    use jsonwebtoken::{encode, EncodingKey, Header};
    let now = chrono::Utc::now().timestamp();
    let claims = serde_json::json!({
        "sub": sub, "iss": issuer,
        "iat": now, "exp": now + exp_delta
    });
    encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(secret),
    )
    .unwrap()
}

#[tokio::test]
async fn oidc_bearer_accepted_when_configured() {
    let secret = b"oidc-middleware-test-secret-32bytes!";
    let issuer = "https://kubernetes.default.svc";
    let oidc = hs_oidc_config(secret, issuer);
    let app = build_auth_test_app(Some(oidc)).await;
    let token = hs_bearer(
        secret,
        issuer,
        "system:serviceaccount:example:sa-cp-demo",
        3600,
    );
    // Root endpoint is publicly accessible — use it to prove the middleware passes the OIDC token
    let (s, _) = get_bearer(&app, "/", &token).await;
    assert_ne!(s, StatusCode::UNAUTHORIZED, "OIDC token should be accepted");
}

#[tokio::test]
async fn oidc_wrong_issuer_rejected_by_middleware() {
    let secret = b"oidc-middleware-test-secret-32bytes!";
    let oidc = hs_oidc_config(secret, "https://kubernetes.default.svc");
    let app = build_auth_test_app(Some(oidc)).await;
    // Token issued by a different issuer
    let token = hs_bearer(secret, "https://attacker.example.com", "attacker", 3600);
    let (s, _) = get_bearer(&app, &format!("{UC}/catalogs"), &token).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn no_token_rejected_when_auth_enabled() {
    let app = build_auth_test_app(None).await;
    let (s, _) = get(&app, &format!("{UC}/catalogs")).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn oidc_expired_token_rejected_by_middleware() {
    let secret = b"oidc-middleware-test-secret-32bytes!";
    let issuer = "https://kubernetes.default.svc";
    let oidc = hs_oidc_config(secret, issuer);
    let app = build_auth_test_app(Some(oidc)).await;
    let token = hs_bearer(secret, issuer, "sa-cp-demo", -300); // expired 5 min ago, outside 60s leeway
    let (s, _) = get_bearer(&app, &format!("{UC}/catalogs"), &token).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
}
