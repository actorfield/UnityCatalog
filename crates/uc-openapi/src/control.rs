//! Types derived from api/control.yaml — SCIM2 users and OAuth2 auth.
use serde::{Deserialize, Serialize};

// ── OAuth2 Token Exchange (RFC 8693) ──────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct OAuthTokenExchangeForm {
    pub grant_type: String,
    pub subject_token: String,
    pub subject_token_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requested_token_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actor_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actor_token_type: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OAuthTokenExchangeResponse {
    pub access_token: String,
    pub token_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_in: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    pub issued_token_type: String,
}

// ── SCIM2 User ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum UserState {
    #[serde(rename = "enabled")]
    Enabled,
    #[serde(rename = "disabled")]
    Disabled,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ScimEmail {
    pub value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ScimName {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub given_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub family_name: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UserResource {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(rename = "userName")]
    pub user_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub emails: Option<Vec<ScimEmail>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<ScimName>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UserResourceList {
    #[serde(rename = "Resources")]
    pub resources: Vec<UserResource>,
    pub total_results: i32,
    pub start_index: i32,
    pub items_per_page: i32,
    pub schemas: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ScimPatchOp {
    pub schemas: Vec<String>,
    #[serde(rename = "Operations")]
    pub operations: Vec<ScimPatchOperation>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ScimPatchOperation {
    pub op: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<serde_json::Value>,
}
