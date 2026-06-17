/// Shared test infrastructure for in-process axum handler tests.
///
/// Uses tower::ServiceExt::oneshot() to send requests directly to the axum Router
/// without starting a TCP server. SQLite in-memory database per test.
use axum::{
    body::Body,
    http::{Request, StatusCode, header},
    Router,
};
use axum::body::to_bytes;
use serde_json::Value;
use std::sync::Arc;
use tower::ServiceExt;
use uc_auth::{AllowingAuthorizer, JwtConfig, KeyManager};
use uc_credentials::CloudCredentialVendor;
use uc_db::{pool::run_migrations, AnyPool};
use uc_api::{
    catalog_api, control_api, delta_api,
    middleware::auth_middleware,
    state::AppState,
};

/// Build a test app with in-memory SQLite and AllowingAuthorizer (no-auth mode).
pub async fn build_test_app() -> (Router, AnyPool) {
    let pool = AnyPool::connect("sqlite::memory:").await.expect("in-memory sqlite");
    run_migrations(&pool).await.expect("migrations");

    let metastore = uc_db::repos::MetastoreRepo::get_or_init(&pool, "test-metastore")
        .await
        .expect("metastore init");

    // Write keys to a temp dir so JWKS endpoint can serve certs.json
    let config_dir = std::env::temp_dir().join(format!("uc_test_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&config_dir).expect("create config dir");
    let km = KeyManager::load_or_generate(&config_dir).expect("key gen");
    let jwt_config = JwtConfig::from_der(&km.private_key_der, &km.public_key_der, km.key_id.clone())
        .expect("jwt config");

    let state = AppState::new(
        pool.clone(),
        Arc::new(AllowingAuthorizer),
        CloudCredentialVendor::new(),
        jwt_config,
        metastore.id,
        false, // no-auth
        config_dir,
    );

    let app = Router::new()
        .merge(catalog_api::router(state.clone()))
        .merge(control_api::router(state.clone()))
        .merge(delta_api::router(state.clone()))
        .route("/", axum::routing::get(|| async { "ok" }))
        .layer(axum::middleware::from_fn_with_state(state.clone(), auth_middleware));

    (app, pool)
}

/// Send a GET request, return (status, body as serde_json::Value).
pub async fn get(app: &Router, uri: &str) -> (StatusCode, Value) {
    let req = Request::builder()
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let json: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    (status, json)
}

/// Send a POST with JSON body, return (status, body as serde_json::Value).
pub async fn post(app: &Router, uri: &str, body: Value) -> (StatusCode, Value) {
    let req = Request::builder()
        .method("POST")
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let bytes = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let json: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, json)
}

/// Send a PATCH with JSON body.
pub async fn patch(app: &Router, uri: &str, body: Value) -> (StatusCode, Value) {
    let req = Request::builder()
        .method("PATCH")
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let bytes = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let json: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, json)
}

/// Send a DELETE, return status code.
pub async fn delete(app: &Router, uri: &str) -> StatusCode {
    delete_with_query(app, uri, "").await
}

pub async fn delete_with_query(app: &Router, uri: &str, query: &str) -> StatusCode {
    let full = if query.is_empty() { uri.to_string() } else { format!("{}?{}", uri, query) };
    let req = Request::builder()
        .method("DELETE")
        .uri(&full)
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    res.status()
}

pub const UC: &str = "/api/2.1/unity-catalog";
pub const CTRL: &str = "/api/1.0/unity-control";
pub const DELTA: &str = "/delta/v1";
