use std::sync::Arc;
use uc_auth::{Authorizer, JwtConfig};
use uc_credentials::CloudCredentialVendor;
use uc_db::AnyPool;
use uuid::Uuid;

/// Shared application state cloned into every axum handler.
#[derive(Clone)]
pub struct AppState {
    pub pool: Arc<AnyPool>,
    pub authorizer: Arc<dyn Authorizer>,
    pub credential_vendor: Arc<CloudCredentialVendor>,
    pub jwt_config: Arc<JwtConfig>,
    pub metastore_id: Uuid,
    pub auth_enabled: bool,
    pub config_dir: std::path::PathBuf,
}

impl AppState {
    pub fn new(
        pool: AnyPool,
        authorizer: Arc<dyn Authorizer>,
        credential_vendor: CloudCredentialVendor,
        jwt_config: JwtConfig,
        metastore_id: Uuid,
        auth_enabled: bool,
        config_dir: std::path::PathBuf,
    ) -> Self {
        Self {
            pool: Arc::new(pool),
            authorizer,
            credential_vendor: Arc::new(credential_vendor),
            jwt_config: Arc::new(jwt_config),
            metastore_id,
            auth_enabled,
            config_dir,
        }
    }
}
