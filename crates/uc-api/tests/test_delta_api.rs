mod common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;
use tower::ServiceExt;

async fn setup(app: &axum::Router) {
    post(app, &format!("{UC}/catalogs"), json!({"name":"d_cat"})).await;
    post(
        app,
        &format!("{UC}/schemas"),
        json!({"name":"d_sch","catalog_name":"d_cat"}),
    )
    .await;
}

fn delta_tables(name: &str) -> String {
    format!("{DELTA}/catalogs/d_cat/schemas/d_sch/tables/{}", name)
}

#[tokio::test]
async fn delta_config() {
    let (app, _) = build_test_app().await;
    let (s, body) = get(&app, &format!("{DELTA}/config")).await;
    assert_eq!(s, StatusCode::OK);
    assert!(body["protocol-version"].as_str().is_some());
    assert!(body["endpoints"].is_array());
}

#[tokio::test]
async fn delta_create_and_load_table() {
    let (app, _) = build_test_app().await;
    setup(&app).await;
    let (s, body) = post(
        &app,
        &format!("{DELTA}/catalogs/d_cat/schemas/d_sch/tables"),
        json!({
            "name":"dt1","location":"s3://b/dt1","table-type":"EXTERNAL"
        }),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let uuid = body["metadata"]["table-uuid"].as_str().unwrap().to_string();
    assert!(!uuid.is_empty());

    let (s, loaded) = get(&app, &delta_tables("dt1")).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(loaded["metadata"]["table-uuid"].as_str().unwrap(), uuid);
    assert_eq!(loaded["latest-table-version"], 0);
}

#[tokio::test]
async fn delta_table_exists_head() {
    let (app, _) = build_test_app().await;
    setup(&app).await;
    post(
        &app,
        &format!("{DELTA}/catalogs/d_cat/schemas/d_sch/tables"),
        json!({
            "name":"head_t","location":"s3://b/head","table-type":"EXTERNAL"
        }),
    )
    .await;

    let req = axum::http::Request::builder()
        .method("HEAD")
        .uri(delta_tables("head_t"))
        .body(axum::body::Body::empty())
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let req2 = axum::http::Request::builder()
        .method("HEAD")
        .uri(delta_tables("no_such_table"))
        .body(axum::body::Body::empty())
        .unwrap();
    let res2 = app.clone().oneshot(req2).await.unwrap();
    assert_eq!(res2.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn delta_add_commit_increments_version() {
    let (app, _) = build_test_app().await;
    setup(&app).await;
    post(
        &app,
        &format!("{DELTA}/catalogs/d_cat/schemas/d_sch/tables"),
        json!({
            "name":"commit_t","location":"s3://b/commit","table-type":"EXTERNAL"
        }),
    )
    .await;

    let (s, r1) = post(&app, &delta_tables("commit_t"), json!({"updates":[{"action":"add-commit","commit":{
        "version":1,"timestamp":1000000,"file-name":"0001.json","file-size":512,"file-modification-timestamp":1000000
    }}]})).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(r1["latest-table-version"], 1);

    let (_, r2) = post(&app, &delta_tables("commit_t"), json!({"updates":[{"action":"add-commit","commit":{
        "version":2,"timestamp":2000000,"file-name":"0002.json","file-size":1024,"file-modification-timestamp":2000000
    }}]})).await;
    assert_eq!(r2["latest-table-version"], 2);
}

#[tokio::test]
async fn delta_commit_version_conflict() {
    let (app, _) = build_test_app().await;
    setup(&app).await;
    post(
        &app,
        &format!("{DELTA}/catalogs/d_cat/schemas/d_sch/tables"),
        json!({
            "name":"conf_t","location":"s3://b/conf","table-type":"EXTERNAL"
        }),
    )
    .await;
    post(&app, &delta_tables("conf_t"), json!({"updates":[{"action":"add-commit","commit":{
        "version":5,"timestamp":1000,"file-name":"0005.json","file-size":100,"file-modification-timestamp":1000
    }}]})).await;

    let (s, _) = post(&app, &delta_tables("conf_t"), json!({"updates":[{"action":"add-commit","commit":{
        "version":3,"timestamp":1000,"file-name":"0003.json","file-size":100,"file-modification-timestamp":1000
    }}]})).await;
    assert_eq!(s, StatusCode::CONFLICT);
}

#[tokio::test]
async fn delta_assert_table_uuid_mismatch_returns_409() {
    let (app, _) = build_test_app().await;
    setup(&app).await;
    post(
        &app,
        &format!("{DELTA}/catalogs/d_cat/schemas/d_sch/tables"),
        json!({
            "name":"req_t","location":"s3://b/req","table-type":"EXTERNAL"
        }),
    )
    .await;

    let wrong_uuid = uuid::Uuid::new_v4().to_string();
    let (s, _) = post(
        &app,
        &delta_tables("req_t"),
        json!({
            "requirements":[{"type":"assert-table-uuid","uuid":wrong_uuid}],
            "updates":[{"action":"set-properties","updates":{"k":"v"}}]
        }),
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT);
}

#[tokio::test]
async fn delta_assert_table_uuid_match_succeeds() {
    let (app, _) = build_test_app().await;
    setup(&app).await;
    let (_, created) = post(
        &app,
        &format!("{DELTA}/catalogs/d_cat/schemas/d_sch/tables"),
        json!({
            "name":"req_ok_t","location":"s3://b/req_ok","table-type":"EXTERNAL"
        }),
    )
    .await;
    let table_uuid = created["metadata"]["table-uuid"]
        .as_str()
        .unwrap()
        .to_string();

    let (s, _) = post(
        &app,
        &delta_tables("req_ok_t"),
        json!({
            "requirements":[{"type":"assert-table-uuid","uuid":table_uuid}],
            "updates":[{"action":"set-properties","updates":{"k":"v"}}]
        }),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
}

#[tokio::test]
async fn delta_set_properties_update() {
    let (app, _) = build_test_app().await;
    setup(&app).await;
    post(
        &app,
        &format!("{DELTA}/catalogs/d_cat/schemas/d_sch/tables"),
        json!({
            "name":"props_t","location":"s3://b/props","table-type":"EXTERNAL"
        }),
    )
    .await;
    let (s, _) = post(
        &app,
        &delta_tables("props_t"),
        json!({"updates":[
            {"action":"set-properties","updates":{"k1":"v1","k2":"v2"}}
        ]}),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
}

#[tokio::test]
async fn delta_set_table_comment_update() {
    let (app, _) = build_test_app().await;
    setup(&app).await;
    post(
        &app,
        &format!("{DELTA}/catalogs/d_cat/schemas/d_sch/tables"),
        json!({
            "name":"comm_t","location":"s3://b/comm","table-type":"EXTERNAL"
        }),
    )
    .await;
    let (s, _) = post(
        &app,
        &delta_tables("comm_t"),
        json!({"updates":[
            {"action":"set-table-comment","comment":"hello"}
        ]}),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
}

#[tokio::test]
async fn delta_rename_table() {
    let (app, _) = build_test_app().await;
    setup(&app).await;
    post(
        &app,
        &format!("{DELTA}/catalogs/d_cat/schemas/d_sch/tables"),
        json!({
            "name":"ren_src","location":"s3://b/ren","table-type":"EXTERNAL"
        }),
    )
    .await;

    let (s, _) = post(
        &app,
        &delta_tables("ren_src/rename"),
        json!({"new-name":"ren_dst"}),
    )
    .await;
    assert_eq!(s, StatusCode::OK);

    let req = axum::http::Request::builder()
        .method("GET")
        .uri(delta_tables("ren_src"))
        .body(axum::body::Body::empty())
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn delta_delete_table() {
    let (app, _) = build_test_app().await;
    setup(&app).await;
    post(
        &app,
        &format!("{DELTA}/catalogs/d_cat/schemas/d_sch/tables"),
        json!({
            "name":"del_dt","location":"s3://b/del","table-type":"EXTERNAL"
        }),
    )
    .await;
    let req = axum::http::Request::builder()
        .method("DELETE")
        .uri(delta_tables("del_dt"))
        .body(axum::body::Body::empty())
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert!(res.status() == StatusCode::OK || res.status() == StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn delta_staging_table_create() {
    let (app, _) = build_test_app().await;
    setup(&app).await;
    let (s, body) = post(
        &app,
        &format!("{DELTA}/catalogs/d_cat/schemas/d_sch/staging-tables"),
        json!({"name":"stg1"}),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert!(body["table-id"].as_str().is_some());
    assert!(body["location"].as_str().is_some());
}
