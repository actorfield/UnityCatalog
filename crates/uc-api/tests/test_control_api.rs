// Tests panic on purpose: unwrap/expect/indexing are the idiom for
// asserting, and a failed assertion should abort the test.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

mod common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;
use tower::ServiceExt;

// ── SCIM2 Users ───────────────────────────────────────────────────────────────

#[tokio::test]
async fn scim2_user_create_list_get_delete() {
    let (app, _) = build_test_app().await;

    // Create
    let (s, user) = post(
        &app,
        &format!("{CTRL}/scim2/Users"),
        json!({"userName":"alice@test.com","active":true}),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(user["userName"], "alice@test.com");
    assert_eq!(user["active"], true);
    let uid = user["id"].as_str().unwrap().to_string();

    // List
    let (s, list) = get(&app, &format!("{CTRL}/scim2/Users")).await;
    assert_eq!(s, StatusCode::OK);
    assert!(list["Resources"]
        .as_array()
        .unwrap()
        .iter()
        .any(|u| u["userName"] == "alice@test.com"));

    // Get by ID
    let (s, fetched) = get(&app, &format!("{CTRL}/scim2/Users/{uid}")).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(fetched["id"].as_str().unwrap(), uid);

    // PUT update
    let req = axum::http::Request::builder()
        .method("PUT")
        .uri(format!("{CTRL}/scim2/Users/{uid}"))
        .header("content-type", "application/json")
        .body(axum::body::Body::from(
            serde_json::to_vec(&json!({"userName":"alice_new@test.com","active":true})).unwrap(),
        ))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    // PATCH disable
    let req2 = axum::http::Request::builder()
        .method("PATCH")
        .uri(format!("{CTRL}/scim2/Users/{uid}"))
        .header("content-type", "application/json")
        .body(axum::body::Body::from(
            serde_json::to_vec(&json!({
                "schemas":["urn:ietf:params:scim:api:messages:2.0:PatchOp"],
                "Operations":[{"op":"replace","value":{"active":false}}]
            }))
            .unwrap(),
        ))
        .unwrap();
    let res2 = app.clone().oneshot(req2).await.unwrap();
    assert_eq!(res2.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(res2.into_body(), usize::MAX)
        .await
        .unwrap();
    let patched: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(patched["active"], false);

    // Delete
    let req3 = axum::http::Request::builder()
        .method("DELETE")
        .uri(format!("{CTRL}/scim2/Users/{uid}"))
        .body(axum::body::Body::empty())
        .unwrap();
    let res3 = app.clone().oneshot(req3).await.unwrap();
    assert_eq!(res3.status(), StatusCode::NO_CONTENT);

    // Get after delete = 404
    let (s404, _) = get(&app, &format!("{CTRL}/scim2/Users/{uid}")).await;
    assert_eq!(s404, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn scim2_get_me_no_auth_returns_anonymous() {
    let (app, _) = build_test_app().await;
    let (s, body) = get(&app, &format!("{CTRL}/scim2/Me")).await;
    assert_eq!(s, StatusCode::OK);
    assert!(body["userName"].as_str().is_some());
}

// ── Auth surface ──────────────────────────────────────────────────────────────
//
// UC signs no tokens, so it has nothing to exchange for and no key to publish.
// These assert the endpoints are *gone*: the whole point of the change was to
// shed auth surface, and a route quietly reappearing would put a signing key
// back with it.

#[tokio::test]
async fn token_exchange_endpoint_no_longer_exists() {
    let (app, _) = build_test_app().await;
    let (s, _) = post(
        &app,
        &format!("{CTRL}/auth/tokens"),
        json!({
            "grant_type": "urn:ietf:params:oauth:grant-type:token-exchange",
            "subject_token": "any@user.com",
            "subject_token_type": "urn:ietf:params:oauth:token-type:access_token"
        }),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn jwks_endpoint_no_longer_exists() {
    let (app, _) = build_test_app().await;
    let (s, _) = get(&app, "/.well-known/jwks.json").await;
    assert_eq!(
        s,
        StatusCode::NOT_FOUND,
        "UC holds no signing key, so it publishes no JWKS"
    );
}

#[tokio::test]
async fn auth_logout_returns_200() {
    let (app, _) = build_test_app().await;
    let (s, _) = post(&app, &format!("{CTRL}/auth/logout"), json!({})).await;
    assert_eq!(s, StatusCode::OK);
}

