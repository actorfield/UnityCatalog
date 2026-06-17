mod common;
use common::*;
use axum::http::StatusCode;
use serde_json::json;

async fn setup(app: &axum::Router) {
    post(app, &format!("{UC}/catalogs"), json!({"name":"tbl_cat"})).await;
    post(app, &format!("{UC}/schemas"), json!({"name":"tbl_sch","catalog_name":"tbl_cat"})).await;
}

fn make_table(name: &str) -> serde_json::Value {
    json!({
        "name": name,
        "catalog_name": "tbl_cat",
        "schema_name": "tbl_sch",
        "table_type": "EXTERNAL",
        "data_source_format": "DELTA",
        "storage_location": format!("/tmp/tables/{}", name),
        "columns": [
            {"name":"id","type_text":"int","type_name":"INT","type_json":"{}","nullable":false,"position":0},
            {"name":"val","type_text":"string","type_name":"STRING","type_json":"{}","nullable":true,"position":1}
        ]
    })
}

#[tokio::test]
async fn table_create_and_get() {
    let (app, _) = build_test_app().await;
    setup(&app).await;
    let (status, body) = post(&app, &format!("{UC}/tables"), make_table("t1")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["name"], "t1");
    assert_eq!(body["table_type"], "EXTERNAL");
    assert_eq!(body["data_source_format"], "DELTA");
    assert_eq!(body["columns"].as_array().unwrap().len(), 2);
    assert!(body["table_id"].as_str().is_some());

    let (s, fetched) = get(&app, &format!("{UC}/tables/tbl_cat.tbl_sch.t1")).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(fetched["table_id"], body["table_id"]);
}

#[tokio::test]
async fn table_get_not_found() {
    let (app, _) = build_test_app().await;
    setup(&app).await;
    let (s, body) = get(&app, &format!("{UC}/tables/tbl_cat.tbl_sch.nope")).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    assert_eq!(body["error_code"], "TABLE_NOT_FOUND");
}

#[tokio::test]
async fn table_list() {
    let (app, _) = build_test_app().await;
    setup(&app).await;
    post(&app, &format!("{UC}/tables"), make_table("ta")).await;
    post(&app, &format!("{UC}/tables"), make_table("tb")).await;
    let (s, body) = get(&app, &format!("{UC}/tables?catalog_name=tbl_cat&schema_name=tbl_sch")).await;
    assert_eq!(s, StatusCode::OK);
    let names: Vec<&str> = body["tables"].as_array().unwrap()
        .iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"ta") && names.contains(&"tb"));
}

#[tokio::test]
async fn table_delete() {
    let (app, _) = build_test_app().await;
    setup(&app).await;
    post(&app, &format!("{UC}/tables"), make_table("del_t")).await;
    let s = delete(&app, &format!("{UC}/tables/tbl_cat.tbl_sch.del_t")).await;
    assert_eq!(s, StatusCode::OK);
    let (s404, _) = get(&app, &format!("{UC}/tables/tbl_cat.tbl_sch.del_t")).await;
    assert_eq!(s404, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn table_storage_location_normalized_to_file_uri() {
    let (app, _) = build_test_app().await;
    setup(&app).await;
    let (_, body) = post(&app, &format!("{UC}/tables"), make_table("file_t")).await;
    let loc = body["storage_location"].as_str().unwrap();
    assert!(loc.starts_with("file://"), "Expected file:// prefix, got: {loc}");
}

#[tokio::test]
async fn table_column_type_name_round_trips() {
    let (app, _) = build_test_app().await;
    setup(&app).await;
    let (_, body) = post(&app, &format!("{UC}/tables"), make_table("type_t")).await;
    let cols = body["columns"].as_array().unwrap();
    let id_col = cols.iter().find(|c| c["name"] == "id").unwrap();
    assert_eq!(id_col["type_name"], "INT");
    assert_eq!(id_col["type_text"], "int");
}

#[tokio::test]
async fn table_create_without_type_json_derives_it() {
    let (app, _) = build_test_app().await;
    setup(&app).await;
    let t = json!({
        "name": "no_json_t",
        "catalog_name": "tbl_cat",
        "schema_name": "tbl_sch",
        "table_type": "EXTERNAL",
        "data_source_format": "DELTA",
        "storage_location": "/tmp/no_json",
        "columns": [
            {"name":"x","type_text":"double","type_name":"DOUBLE","nullable":false,"position":0}
        ]
    });
    let (s, body) = post(&app, &format!("{UC}/tables"), t).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(body["columns"][0]["type_name"], "DOUBLE");
}

#[tokio::test]
async fn table_name_with_slash_rejected() {
    let (app, _) = build_test_app().await;
    setup(&app).await;
    let (s, _) = post(&app, &format!("{UC}/tables"), json!({
        "name":"bad/name","catalog_name":"tbl_cat","schema_name":"tbl_sch",
        "table_type":"EXTERNAL","data_source_format":"DELTA","storage_location":"/tmp/x","columns":[]
    })).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn managed_table_via_staging_flow() {
    let (app, _) = build_test_app().await;
    setup(&app).await;

    // Create staging table
    let (s, staging) = post(&app, &format!("{UC}/staging-tables"), json!({
        "name": "staged_t",
        "catalog_name": "tbl_cat",
        "schema_name": "tbl_sch"
    })).await;
    assert_eq!(s, StatusCode::OK);
    let staging_id = staging["table_id"].as_str().unwrap().to_string();
    let staging_loc = staging["staging_location"].as_str().unwrap().to_string();

    // Commit as MANAGED table
    let (s, tbl) = post(&app, &format!("{UC}/tables"), json!({
        "name": "staged_t",
        "catalog_name": "tbl_cat",
        "schema_name": "tbl_sch",
        "table_type": "MANAGED",
        "data_source_format": "DELTA",
        "storage_location": staging_loc,
        "columns": []
    })).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(tbl["table_id"].as_str().unwrap(), staging_id, "Table ID must match staging UUID");
    assert_eq!(tbl["table_type"], "MANAGED");
}

#[tokio::test]
async fn staging_commit_twice_returns_error() {
    let (app, _) = build_test_app().await;
    setup(&app).await;
    let (_, staging) = post(&app, &format!("{UC}/staging-tables"), json!({
        "name":"double_stage","catalog_name":"tbl_cat","schema_name":"tbl_sch"
    })).await;
    let loc = staging["staging_location"].as_str().unwrap().to_string();

    let commit = json!({
        "name":"double_stage","catalog_name":"tbl_cat","schema_name":"tbl_sch",
        "table_type":"MANAGED","data_source_format":"DELTA","storage_location":loc,"columns":[]
    });
    post(&app, &format!("{UC}/tables"), commit.clone()).await;
    let (s, _) = post(&app, &format!("{UC}/tables"), commit).await;
    assert!(s == StatusCode::CONFLICT || s == StatusCode::BAD_REQUEST);
}
