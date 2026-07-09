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

// ── Auth Tokens ───────────────────────────────────────────────────────────────

#[tokio::test]
async fn auth_token_exchange_returns_jwt() {
    let (app, _) = build_test_app().await;
    let (s, body) = post(
        &app,
        &format!("{CTRL}/auth/tokens"),
        json!({
            "grant_type": "urn:ietf:params:oauth:grant-type:token-exchange",
            "subject_token": "any@user.com",
            "subject_token_type": "urn:ietf:params:oauth:token-type:access_token"
        }),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let token = body["access_token"].as_str().unwrap();
    assert_eq!(token.split('.').count(), 3, "Should be a 3-part JWT");
    assert_eq!(body["token_type"], "Bearer");
    assert_eq!(
        body["issued_token_type"],
        "urn:ietf:params:oauth:token-type:access_token"
    );
}

#[tokio::test]
async fn auth_token_wrong_grant_type_returns_400() {
    let (app, _) = build_test_app().await;
    let (s, _) = post(
        &app,
        &format!("{CTRL}/auth/tokens"),
        json!({
            "grant_type": "password",
            "subject_token": "user@x.com",
            "subject_token_type": "urn:ietf:params:oauth:token-type:access_token"
        }),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn auth_logout_returns_200() {
    let (app, _) = build_test_app().await;
    let (s, _) = post(&app, &format!("{CTRL}/auth/logout"), json!({})).await;
    assert_eq!(s, StatusCode::OK);
}

#[tokio::test]
async fn jwks_endpoint_returns_valid_json() {
    let (app, _) = build_test_app().await;
    let (s, body) = get(&app, "/.well-known/jwks.json").await;
    assert_eq!(s, StatusCode::OK);
    assert!(body["keys"].is_array());
    let keys = body["keys"].as_array().unwrap();
    assert!(!keys.is_empty());
    assert_eq!(keys[0]["kty"], "RSA");
    assert!(keys[0]["kid"].as_str().is_some());
    // n and e must be base64url, not raw DER
    let n = keys[0]["n"].as_str().unwrap();
    assert!(!n.contains('+'), "n should be base64url (no +)");
    assert!(!n.contains('/'), "n should be base64url (no /)");
}
