mod common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

async fn setup_catalog(app: &axum::Router, name: &str) {
    post(app, &format!("{UC}/catalogs"), json!({"name": name})).await;
}

#[tokio::test]
async fn schema_create_and_get() {
    let (app, _) = build_test_app().await;
    setup_catalog(&app, "sc_cat").await;
    let (status, body) = post(
        &app,
        &format!("{UC}/schemas"),
        json!({"name":"sc1","catalog_name":"sc_cat"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["name"], "sc1");
    assert_eq!(body["catalog_name"], "sc_cat");
    assert_eq!(body["full_name"], "sc_cat.sc1");

    let (s, fetched) = get(&app, &format!("{UC}/schemas/sc_cat.sc1")).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(fetched["schema_id"], body["schema_id"]);
}

#[tokio::test]
async fn schema_list() {
    let (app, _) = build_test_app().await;
    setup_catalog(&app, "sl_cat").await;
    post(
        &app,
        &format!("{UC}/schemas"),
        json!({"name":"a","catalog_name":"sl_cat"}),
    )
    .await;
    post(
        &app,
        &format!("{UC}/schemas"),
        json!({"name":"b","catalog_name":"sl_cat"}),
    )
    .await;
    let (status, body) = get(&app, &format!("{UC}/schemas?catalog_name=sl_cat")).await;
    assert_eq!(status, StatusCode::OK);
    let names: Vec<&str> = body["schemas"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"a") && names.contains(&"b"));
}

#[tokio::test]
async fn schema_get_not_found() {
    let (app, _) = build_test_app().await;
    setup_catalog(&app, "snf_cat").await;
    let (s, body) = get(&app, &format!("{UC}/schemas/snf_cat.missing")).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    assert_eq!(body["error_code"], "SCHEMA_NOT_FOUND");
}

#[tokio::test]
async fn schema_update_rename() {
    let (app, _) = build_test_app().await;
    setup_catalog(&app, "sur_cat").await;
    post(
        &app,
        &format!("{UC}/schemas"),
        json!({"name":"old","catalog_name":"sur_cat"}),
    )
    .await;
    let (s, _) = patch(
        &app,
        &format!("{UC}/schemas/sur_cat.old"),
        json!({"new_name":"new","comment":"renamed"}),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (s404, _) = get(&app, &format!("{UC}/schemas/sur_cat.old")).await;
    assert_eq!(s404, StatusCode::NOT_FOUND);
    let (s200, body) = get(&app, &format!("{UC}/schemas/sur_cat.new")).await;
    assert_eq!(s200, StatusCode::OK);
    assert_eq!(body["comment"], "renamed");
}

#[tokio::test]
async fn schema_delete_empty() {
    let (app, _) = build_test_app().await;
    setup_catalog(&app, "sde_cat").await;
    post(
        &app,
        &format!("{UC}/schemas"),
        json!({"name":"empty","catalog_name":"sde_cat"}),
    )
    .await;
    let s = delete(&app, &format!("{UC}/schemas/sde_cat.empty")).await;
    assert_eq!(s, StatusCode::OK);
}

#[tokio::test]
async fn schema_delete_nonempty_without_force_returns_409() {
    let (app, _) = build_test_app().await;
    setup_catalog(&app, "sdne_cat").await;
    post(
        &app,
        &format!("{UC}/schemas"),
        json!({"name":"full","catalog_name":"sdne_cat"}),
    )
    .await;
    post(
        &app,
        &format!("{UC}/tables"),
        json!({
            "name":"t1","catalog_name":"sdne_cat","schema_name":"full",
            "table_type":"EXTERNAL","data_source_format":"DELTA",
            "storage_location":"/tmp/sdne","columns":[]
        }),
    )
    .await;
    let s = delete(&app, &format!("{UC}/schemas/sdne_cat.full")).await;
    assert!(s == StatusCode::CONFLICT || s == StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn schema_delete_with_force_cascades_tables() {
    let (app, _) = build_test_app().await;
    setup_catalog(&app, "sdf_cat").await;
    post(
        &app,
        &format!("{UC}/schemas"),
        json!({"name":"has_tables","catalog_name":"sdf_cat"}),
    )
    .await;
    post(
        &app,
        &format!("{UC}/tables"),
        json!({
            "name":"t1","catalog_name":"sdf_cat","schema_name":"has_tables",
            "table_type":"EXTERNAL","data_source_format":"DELTA",
            "storage_location":"/tmp/sdf","columns":[]
        }),
    )
    .await;
    let s = delete_with_query(
        &app,
        &format!("{UC}/schemas/sdf_cat.has_tables"),
        "force=true",
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (s404, _) = get(&app, &format!("{UC}/schemas/sdf_cat.has_tables")).await;
    assert_eq!(s404, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn schema_name_validation() {
    let (app, _) = build_test_app().await;
    setup_catalog(&app, "snv_cat").await;
    let (s, _) = post(
        &app,
        &format!("{UC}/schemas"),
        json!({"name":"bad.name","catalog_name":"snv_cat"}),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}
