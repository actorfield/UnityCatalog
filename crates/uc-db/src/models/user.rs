use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, FromRow)]
pub struct UserRow {
    pub id: Uuid,
    pub name: String,
    pub email: Option<String>,
    pub external_id: Option<String>,
    pub state: Option<String>,
    pub created_at: Option<i64>,
    pub updated_at: Option<i64>,
    pub picture_url: Option<String>,
}

impl UserRow {
    pub fn is_enabled(&self) -> bool {
        self.state.as_deref().map(|s| s.eq_ignore_ascii_case("enabled")).unwrap_or(false)
    }
}
