use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Generic paginated list wrapper used by several list endpoints.
#[derive(Debug, Serialize, Deserialize)]
pub struct PagedList<T> {
    #[serde(flatten)]
    pub items_field: T,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
}

/// Key/value property map used across all entity types.
pub type PropertyMap = HashMap<String, String>;
