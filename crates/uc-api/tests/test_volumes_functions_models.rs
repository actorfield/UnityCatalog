mod common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

async fn setup(app: &axum::Router) {
    post(app, &format!("{UC}/catalogs"), json!({"name":"vfm_cat"})).await;
    post(
        app,
        &format!("{UC}/schemas"),
        json!({"name":"vfm_sch","catalog_name":"vfm_cat"}),
    )
    .await;
}

// ── Volumes ───────────────────────────────────────────────────────────────────

#[tokio::test]
async fn volume_create_and_get() {
    let (app, _) = build_test_app().await;
    setup(&app).await;
    let (s, body) = post(
        &app,
        &format!("{UC}/volumes"),
        json!({
            "name":"v1","catalog_name":"vfm_cat","schema_name":"vfm_sch",
            "volume_type":"EXTERNAL","storage_location":"/tmp/v1"
        }),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(body["name"], "v1");
    assert_eq!(body["volume_type"], "EXTERNAL");
    assert!(body["volume_id"].as_str().is_some());

    let (s, fetched) = get(&app, &format!("{UC}/volumes/vfm_cat.vfm_sch.v1")).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(fetched["volume_id"], body["volume_id"]);
}

#[tokio::test]
async fn volume_list_and_update_and_delete() {
    let (app, _) = build_test_app().await;
    setup(&app).await;
    post(
        &app,
        &format!("{UC}/volumes"),
        json!({
            "name":"vol_lud","catalog_name":"vfm_cat","schema_name":"vfm_sch",
            "volume_type":"MANAGED","storage_location":"/tmp/lud"
        }),
    )
    .await;

    // List
    let (s, body) = get(
        &app,
        &format!("{UC}/volumes?catalog_name=vfm_cat&schema_name=vfm_sch"),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert!(body["volumes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v["name"] == "vol_lud"));

    // Update
    let (s, _) = patch(
        &app,
        &format!("{UC}/volumes/vfm_cat.vfm_sch.vol_lud"),
        json!({"comment":"patched"}),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (_, fetched) = get(&app, &format!("{UC}/volumes/vfm_cat.vfm_sch.vol_lud")).await;
    assert_eq!(fetched["comment"], "patched");

    // Delete
    let s = delete(&app, &format!("{UC}/volumes/vfm_cat.vfm_sch.vol_lud")).await;
    assert_eq!(s, StatusCode::OK);
    let (s404, _) = get(&app, &format!("{UC}/volumes/vfm_cat.vfm_sch.vol_lud")).await;
    assert_eq!(s404, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn volume_storage_normalized() {
    let (app, _) = build_test_app().await;
    setup(&app).await;
    let (_, body) = post(
        &app,
        &format!("{UC}/volumes"),
        json!({
            "name":"vol_norm","catalog_name":"vfm_cat","schema_name":"vfm_sch",
            "volume_type":"EXTERNAL","storage_location":"/local/path"
        }),
    )
    .await;
    assert!(body["storage_location"]
        .as_str()
        .unwrap()
        .starts_with("file://"));
}

// ── Functions ─────────────────────────────────────────────────────────────────

fn make_function(name: &str) -> serde_json::Value {
    json!({
        "function_info": {
            "name": name,
            "catalog_name": "vfm_cat",
            "schema_name": "vfm_sch",
            "data_type": "INT",
            "full_data_type": "int",
            "routine_body": "EXTERNAL",
            "routine_definition": "return 42",
            "parameter_style": "S",
            "is_deterministic": true,
            "sql_data_access": "NO_SQL",
            "is_null_call": false,
            "security_type": "DEFINER",
            "specific_name": name,
            "external_language": "python",
            "input_params": {"parameters": [
                {"name":"x","type_text":"int","type_name":"INT","type_json":"{}","position":0,"parameter_mode":"IN"}
            ]}
        }
    })
}

#[tokio::test]
async fn function_create_and_get() {
    let (app, _) = build_test_app().await;
    setup(&app).await;
    let (s, body) = post(&app, &format!("{UC}/functions"), make_function("fn1")).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(body["name"], "fn1");
    assert_eq!(body["external_language"], "python");
    assert!(body["input_params"]["parameters"].as_array().unwrap().len() == 1);

    let (s, fetched) = get(&app, &format!("{UC}/functions/vfm_cat.vfm_sch.fn1")).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(fetched["function_id"], body["function_id"]);
}

#[tokio::test]
async fn function_list_and_delete() {
    let (app, _) = build_test_app().await;
    setup(&app).await;
    post(&app, &format!("{UC}/functions"), make_function("fna")).await;
    post(&app, &format!("{UC}/functions"), make_function("fnb")).await;

    let (s, body) = get(
        &app,
        &format!("{UC}/functions?catalog_name=vfm_cat&schema_name=vfm_sch"),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let names: Vec<&str> = body["functions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"fna") && names.contains(&"fnb"));

    let s = delete(&app, &format!("{UC}/functions/vfm_cat.vfm_sch.fna")).await;
    assert_eq!(s, StatusCode::OK);
}

#[tokio::test]
async fn function_param_type_name_returned() {
    let (app, _) = build_test_app().await;
    setup(&app).await;
    let (_, body) = post(&app, &format!("{UC}/functions"), make_function("fn_typed")).await;
    let params = body["input_params"]["parameters"].as_array().unwrap();
    assert_eq!(params[0]["type_name"], "INT");
}

// ── Models ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn model_lifecycle() {
    let (app, _) = build_test_app().await;
    setup(&app).await;

    // Create
    let (s, model) = post(
        &app,
        &format!("{UC}/models"),
        json!({
            "name":"mdl1","catalog_name":"vfm_cat","schema_name":"vfm_sch","comment":"v1"
        }),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let model_id = model["id"].as_str().unwrap();

    // Get
    let (s, fetched) = get(&app, &format!("{UC}/models/vfm_cat.vfm_sch.mdl1")).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(fetched["id"].as_str().unwrap(), model_id);

    // Create version
    let (s, ver) = post(
        &app,
        &format!("{UC}/models/versions"),
        json!({
            "model_name":"mdl1","catalog_name":"vfm_cat","schema_name":"vfm_sch",
            "source":"s3://ml/run1","run_id":"run_abc"
        }),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(ver["version"], 1);
    assert_eq!(ver["status"], "PENDING_REGISTRATION");

    // Second version gets version=2
    let (_, ver2) = post(
        &app,
        &format!("{UC}/models/versions"),
        json!({
            "model_name":"mdl1","catalog_name":"vfm_cat","schema_name":"vfm_sch",
            "source":"s3://ml/run2"
        }),
    )
    .await;
    assert_eq!(ver2["version"], 2);

    // List versions
    let (s, versions) = get(&app, &format!("{UC}/models/vfm_cat.vfm_sch.mdl1/versions")).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(versions["model_versions"].as_array().unwrap().len(), 2);

    // Update version comment
    let (s, _) = patch(
        &app,
        &format!("{UC}/models/vfm_cat.vfm_sch.mdl1/versions/1"),
        json!({"comment":"updated"}),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (_, v1) = get(
        &app,
        &format!("{UC}/models/vfm_cat.vfm_sch.mdl1/versions/1"),
    )
    .await;
    assert_eq!(v1["comment"], "updated");

    // Finalize
    let (s, finalized) = patch(
        &app,
        &format!("{UC}/models/vfm_cat.vfm_sch.mdl1/versions/1/finalize"),
        json!({"status":"READY"}),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(finalized["status"], "READY");

    // Delete version
    let s = delete(
        &app,
        &format!("{UC}/models/vfm_cat.vfm_sch.mdl1/versions/2"),
    )
    .await;
    assert_eq!(s, StatusCode::OK);

    // Rename model
    let (s, _) = patch(
        &app,
        &format!("{UC}/models/vfm_cat.vfm_sch.mdl1"),
        json!({"new_name":"mdl_renamed"}),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (s404, _) = get(&app, &format!("{UC}/models/vfm_cat.vfm_sch.mdl1")).await;
    assert_eq!(s404, StatusCode::NOT_FOUND);
    let (s200, _) = get(&app, &format!("{UC}/models/vfm_cat.vfm_sch.mdl_renamed")).await;
    assert_eq!(s200, StatusCode::OK);

    // Delete model (and remaining versions)
    let s = delete(&app, &format!("{UC}/models/vfm_cat.vfm_sch.mdl_renamed")).await;
    assert_eq!(s, StatusCode::OK);
}
