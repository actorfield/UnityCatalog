mod common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

#[tokio::test]
async fn catalog_list_empty() {
    let (app, _) = build_test_app().await;
    let (status, body) = get(&app, &format!("{UC}/catalogs")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["catalogs"], json!([]));
}

#[tokio::test]
async fn catalog_create_and_get() {
    let (app, _) = build_test_app().await;
    let (status, body) = post(&app, &format!("{UC}/catalogs"), json!({"name":"test_cat"})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["name"], "test_cat");
    assert!(body["id"].as_str().is_some());

    let (status, body) = get(&app, &format!("{UC}/catalogs/test_cat")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["name"], "test_cat");
}

#[tokio::test]
async fn catalog_create_with_comment_and_properties() {
    let (app, _) = build_test_app().await;
    let (status, body) = post(
        &app,
        &format!("{UC}/catalogs"),
        json!({
            "name": "cat_with_props",
            "comment": "my catalog",
            "properties": {"env": "test", "team": "platform"}
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["comment"], "my catalog");

    let (_, fetched) = get(&app, &format!("{UC}/catalogs/cat_with_props")).await;
    assert_eq!(fetched["properties"]["env"], "test");
}

#[tokio::test]
async fn catalog_get_not_found() {
    let (app, _) = build_test_app().await;
    let (status, body) = get(&app, &format!("{UC}/catalogs/does_not_exist")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error_code"], "CATALOG_NOT_FOUND");
}

#[tokio::test]
async fn catalog_update_comment() {
    let (app, _) = build_test_app().await;
    post(&app, &format!("{UC}/catalogs"), json!({"name":"upd_cat"})).await;
    let (status, _body) = patch(
        &app,
        &format!("{UC}/catalogs/upd_cat"),
        json!({"comment":"updated"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, fetched) = get(&app, &format!("{UC}/catalogs/upd_cat")).await;
    assert_eq!(fetched["comment"], "updated");
}

#[tokio::test]
async fn catalog_update_rename() {
    let (app, _) = build_test_app().await;
    post(&app, &format!("{UC}/catalogs"), json!({"name":"old_name"})).await;
    let (status, _) = patch(
        &app,
        &format!("{UC}/catalogs/old_name"),
        json!({"new_name":"new_name"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (s404, _) = get(&app, &format!("{UC}/catalogs/old_name")).await;
    assert_eq!(s404, StatusCode::NOT_FOUND);
    let (s200, _) = get(&app, &format!("{UC}/catalogs/new_name")).await;
    assert_eq!(s200, StatusCode::OK);
}

#[tokio::test]
async fn catalog_delete_empty() {
    let (app, _) = build_test_app().await;
    post(&app, &format!("{UC}/catalogs"), json!({"name":"del_cat"})).await;
    let status = delete(&app, &format!("{UC}/catalogs/del_cat")).await;
    assert_eq!(status, StatusCode::OK);
    let (s, _) = get(&app, &format!("{UC}/catalogs/del_cat")).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn catalog_delete_nonempty_without_force_returns_409() {
    let (app, _) = build_test_app().await;
    post(
        &app,
        &format!("{UC}/catalogs"),
        json!({"name":"nonempty_cat"}),
    )
    .await;
    post(
        &app,
        &format!("{UC}/schemas"),
        json!({"name":"s1","catalog_name":"nonempty_cat"}),
    )
    .await;
    let status = delete(&app, &format!("{UC}/catalogs/nonempty_cat")).await;
    assert!(
        status == StatusCode::CONFLICT || status == StatusCode::BAD_REQUEST,
        "Expected 409/400, got {status}"
    );
}

#[tokio::test]
async fn catalog_delete_nonempty_with_force() {
    let (app, _) = build_test_app().await;
    post(&app, &format!("{UC}/catalogs"), json!({"name":"force_cat"})).await;
    post(
        &app,
        &format!("{UC}/schemas"),
        json!({"name":"s1","catalog_name":"force_cat"}),
    )
    .await;
    let status = delete_with_query(&app, &format!("{UC}/catalogs/force_cat"), "force=true").await;
    assert_eq!(status, StatusCode::OK);
    let (s, _) = get(&app, &format!("{UC}/catalogs/force_cat")).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn catalog_duplicate_name_rejected() {
    let (app, _) = build_test_app().await;
    post(&app, &format!("{UC}/catalogs"), json!({"name":"dup_cat"})).await;
    let (status, body) = post(&app, &format!("{UC}/catalogs"), json!({"name":"dup_cat"})).await;
    assert!(status == StatusCode::BAD_REQUEST || status == StatusCode::CONFLICT);
    let code = body["error_code"].as_str().unwrap_or("");
    assert!(code.contains("ALREADY_EXISTS") || code.contains("CATALOG_ALREADY"));
}

#[tokio::test]
async fn catalog_name_with_dot_rejected() {
    let (app, _) = build_test_app().await;
    let (status, _) = post(&app, &format!("{UC}/catalogs"), json!({"name":"cat.dot"})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn catalog_empty_name_rejected() {
    let (app, _) = build_test_app().await;
    let (status, _) = post(&app, &format!("{UC}/catalogs"), json!({"name":""})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn catalog_patch_empty_properties_preserves_existing() {
    let (app, _) = build_test_app().await;
    post(
        &app,
        &format!("{UC}/catalogs"),
        json!({"name":"prop_cat","properties":{"k":"v"}}),
    )
    .await;
    patch(
        &app,
        &format!("{UC}/catalogs/prop_cat"),
        json!({"properties":{}}),
    )
    .await;
    let (_, body) = get(&app, &format!("{UC}/catalogs/prop_cat")).await;
    assert_eq!(
        body["properties"]["k"], "v",
        "Empty PATCH should not wipe properties"
    );
}
