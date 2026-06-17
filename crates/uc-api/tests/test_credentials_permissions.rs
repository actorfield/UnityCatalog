mod common;
use common::*;
use axum::http::StatusCode;
use serde_json::json;

// ── Credentials ───────────────────────────────────────────────────────────────

fn make_cred(name: &str, arn: &str) -> serde_json::Value {
    json!({
        "name": name,
        "purpose": "AWS_IAM_ROLE",
        "aws_iam_role": {"role_arn": arn}
    })
}

#[tokio::test]
async fn credential_create_and_get() {
    let (app, _) = build_test_app().await;
    let (s, body) = post(&app, &format!("{UC}/credentials"), make_cred("c1","arn:aws:iam::123:role/r")).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(body["name"], "c1");
    assert!(body["id"].as_str().is_some());
    assert_eq!(body["full_name"].as_str().unwrap(), "c1");
    assert_eq!(body["aws_iam_role"]["role_arn"], "arn:aws:iam::123:role/r");

    let (s, fetched) = get(&app, &format!("{UC}/credentials/c1")).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(fetched["id"], body["id"]);
}

#[tokio::test]
async fn credential_list() {
    let (app, _) = build_test_app().await;
    post(&app, &format!("{UC}/credentials"), make_cred("cl1","arn:aws:iam::1:role/a")).await;
    post(&app, &format!("{UC}/credentials"), make_cred("cl2","arn:aws:iam::1:role/b")).await;
    let (s, body) = get(&app, &format!("{UC}/credentials")).await;
    assert_eq!(s, StatusCode::OK);
    let names: Vec<&str> = body["credentials"].as_array().unwrap()
        .iter().map(|c| c["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"cl1") && names.contains(&"cl2"));
}

#[tokio::test]
async fn credential_update_role_arn() {
    let (app, _) = build_test_app().await;
    post(&app, &format!("{UC}/credentials"), make_cred("upd_c","arn:old")).await;
    let (s, _) = patch(&app, &format!("{UC}/credentials/upd_c"),
        json!({"aws_iam_role":{"role_arn":"arn:new"}})).await;
    assert_eq!(s, StatusCode::OK);
    let (_, fetched) = get(&app, &format!("{UC}/credentials/upd_c")).await;
    assert_eq!(fetched["aws_iam_role"]["role_arn"], "arn:new");
}

#[tokio::test]
async fn credential_update_rename() {
    let (app, _) = build_test_app().await;
    post(&app, &format!("{UC}/credentials"), make_cred("ren_c","arn:x")).await;
    let (s, _) = patch(&app, &format!("{UC}/credentials/ren_c"), json!({"new_name":"ren_c2"})).await;
    assert_eq!(s, StatusCode::OK);
    let (s404, _) = get(&app, &format!("{UC}/credentials/ren_c")).await;
    assert_eq!(s404, StatusCode::NOT_FOUND);
    let (s200, _) = get(&app, &format!("{UC}/credentials/ren_c2")).await;
    assert_eq!(s200, StatusCode::OK);
}

#[tokio::test]
async fn credential_delete() {
    let (app, _) = build_test_app().await;
    post(&app, &format!("{UC}/credentials"), make_cred("del_c","arn:del")).await;
    let s = delete(&app, &format!("{UC}/credentials/del_c")).await;
    assert_eq!(s, StatusCode::OK);
    let (s404, _) = get(&app, &format!("{UC}/credentials/del_c")).await;
    assert_eq!(s404, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn credential_duplicate_rejected() {
    let (app, _) = build_test_app().await;
    post(&app, &format!("{UC}/credentials"), make_cred("dup_c","arn:a")).await;
    let (s, _) = post(&app, &format!("{UC}/credentials"), make_cred("dup_c","arn:b")).await;
    assert!(s == StatusCode::BAD_REQUEST || s == StatusCode::CONFLICT);
}

// ── External Locations ────────────────────────────────────────────────────────

#[tokio::test]
async fn external_location_crud_and_credential_name() {
    let (app, _) = build_test_app().await;
    post(&app, &format!("{UC}/credentials"), make_cred("el_cred","arn:el")).await;

    // Create
    let (s, body) = post(&app, &format!("{UC}/external-locations"), json!({
        "name":"el1","url":"s3://bucket/path","credential_name":"el_cred"
    })).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(body["name"], "el1");

    // GET returns credential_name (not empty string)
    let (s, fetched) = get(&app, &format!("{UC}/external-locations/el1")).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(fetched["credential_name"].as_str().unwrap(), "el_cred",
        "GET must return credential_name, got: {}", fetched["credential_name"]);

    // LIST returns credential_name
    let (_, list) = get(&app, &format!("{UC}/external-locations")).await;
    let matching = list["external_locations"].as_array().unwrap()
        .iter().find(|e| e["name"] == "el1").unwrap();
    assert_eq!(matching["credential_name"].as_str().unwrap(), "el_cred");

    // Update credential
    post(&app, &format!("{UC}/credentials"), make_cred("el_cred2","arn:el2")).await;
    let (s, _) = patch(&app, &format!("{UC}/external-locations/el1"),
        json!({"credential_name":"el_cred2"})).await;
    assert_eq!(s, StatusCode::OK);
    let (_, after) = get(&app, &format!("{UC}/external-locations/el1")).await;
    assert_eq!(after["credential_name"].as_str().unwrap(), "el_cred2");

    // Delete
    let s = delete(&app, &format!("{UC}/external-locations/el1")).await;
    assert_eq!(s, StatusCode::OK);
}

// ── Permissions ───────────────────────────────────────────────────────────────

#[tokio::test]
async fn permissions_get_returns_list() {
    let (app, _) = build_test_app().await;
    post(&app, &format!("{UC}/catalogs"), json!({"name":"perm_cat"})).await;
    let (s, body) = get(&app, &format!("{UC}/permissions/catalog/perm_cat")).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(body["securable_type"], "CATALOG");
    assert!(body["privilege_assignments"].is_array());
}

#[tokio::test]
async fn permissions_unknown_securable_returns_400() {
    let (app, _) = build_test_app().await;
    post(&app, &format!("{UC}/catalogs"), json!({"name":"permu_cat"})).await;
    let (s, body) = get(&app, &format!("{UC}/permissions/spaceship/permu_cat")).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(body["message"].as_str().unwrap().contains("Unknown securable"));
}

#[tokio::test]
async fn permissions_patch_unknown_privilege_returns_400() {
    let (app, _) = build_test_app().await;
    post(&app, &format!("{UC}/catalogs"), json!({"name":"permp_cat"})).await;
    // Create a user first
    post(&app, &format!("{CTRL}/scim2/Users"), json!({"userName":"perm_user@x.com","active":true})).await;
    let (s, body) = patch(&app, &format!("{UC}/permissions/catalog/permp_cat"), json!({
        "changes":[{"principal":"perm_user@x.com","add":["INVALID_PRIV"],"remove":[]}]
    })).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(body["message"].as_str().unwrap().contains("Unknown privilege"));
}

#[tokio::test]
async fn metastore_summary_returns_id() {
    let (app, _) = build_test_app().await;
    let (s, body) = get(&app, &format!("{UC}/metastore_summary")).await;
    assert_eq!(s, StatusCode::OK);
    assert!(body["metastore_id"].as_str().is_some());
}
