use casbin::{CoreApi, Enforcer, MgmtApi};
use std::sync::Arc;
use tokio::sync::RwLock;
use uc_errors::{ErrorCode, UcError};
use uc_types::Privilege;
use uuid::Uuid;

// ── Authorizer trait ──────────────────────────────────────────────────────────

#[async_trait::async_trait]
pub trait Authorizer: Send + Sync {
    async fn authorize(&self, principal: Uuid, resource: Uuid, privilege: Privilege) -> Result<bool, UcError>;
    async fn authorize_any(&self, principal: Uuid, resource: Uuid, privileges: &[Privilege]) -> Result<bool, UcError>;
    async fn grant(&self, principal: Uuid, resource: Uuid, privilege: Privilege) -> Result<(), UcError>;
    async fn revoke(&self, principal: Uuid, resource: Uuid, privilege: Privilege) -> Result<(), UcError>;
    async fn add_hierarchy_child(&self, parent: Uuid, child: Uuid) -> Result<(), UcError>;
    async fn remove_hierarchy_children(&self, resource: Uuid) -> Result<(), UcError>;
    /// List all privileges a principal has on a specific resource.
    async fn list_privileges(&self, principal: Uuid, resource: Uuid) -> Result<Vec<Privilege>, UcError>;
    /// List all (principal, privileges) pairs for a resource.
    async fn list_grants_on_resource(&self, resource: Uuid) -> Result<Vec<(Uuid, Vec<Privilege>)>, UcError>;
}

// ── UcAuthorizer (JCasbin-backed) ─────────────────────────────────────────────

pub struct UcAuthorizer {
    enforcer: Arc<RwLock<Enforcer>>,
}

impl UcAuthorizer {
    /// The casbin model is embedded from the file copied from the Java project.
    const MODEL: &'static str = include_str!("../resources/jcasbin_auth_model.conf");

    /// Initialize with an in-memory adapter (for testing).
    pub async fn new_in_memory() -> Result<Self, UcError> {
        use casbin::{DefaultModel, MemoryAdapter};

        let model = DefaultModel::from_str(Self::MODEL)
            .await
            .map_err(|e| UcError::new(ErrorCode::Internal, format!("Casbin model load failed: {}", e)))?;

        let adapter = MemoryAdapter::default();

        let enforcer = Enforcer::new(model, adapter)
            .await
            .map_err(|e| UcError::new(ErrorCode::Internal, format!("Casbin enforcer init failed: {}", e)))?;

        Ok(Self { enforcer: Arc::new(RwLock::new(enforcer)) })
    }

    /// Initialize with a DB-backed adapter so policies survive restarts.
    /// Loads existing policies from `casbin_rule` table on startup.
    pub async fn new_with_db(pool: uc_db::AnyPool) -> Result<Self, UcError> {
        use casbin::DefaultModel;
        use crate::db_adapter::SqlxAdapter;

        let model = DefaultModel::from_str(Self::MODEL)
            .await
            .map_err(|e| UcError::new(ErrorCode::Internal, format!("Casbin model load failed: {}", e)))?;

        let adapter = SqlxAdapter::new(pool)
            .await
            .map_err(|e| UcError::new(ErrorCode::Internal, format!("Casbin DB adapter init failed: {}", e)))?;

        let enforcer = Enforcer::new(model, adapter)
            .await
            .map_err(|e| UcError::new(ErrorCode::Internal, format!("Casbin enforcer init failed: {}", e)))?;

        Ok(Self { enforcer: Arc::new(RwLock::new(enforcer)) })
    }

    /// Initialize the admin user with OWNER on the metastore.
    pub async fn init_admin(&self, admin_id: Uuid, metastore_id: Uuid) -> Result<(), UcError> {
        self.grant(admin_id, metastore_id, Privilege::Owner).await?;
        Ok(())
    }
}

#[async_trait::async_trait]
impl Authorizer for UcAuthorizer {
    async fn authorize(&self, principal: Uuid, resource: Uuid, privilege: Privilege) -> Result<bool, UcError> {
        let enforcer = self.enforcer.read().await;
        enforcer
            .enforce((principal.to_string(), resource.to_string(), privilege.as_casbin_str()))
            .map_err(|e| UcError::new(ErrorCode::Internal, format!("Casbin enforce failed: {}", e)))
    }

    async fn authorize_any(&self, principal: Uuid, resource: Uuid, privileges: &[Privilege]) -> Result<bool, UcError> {
        for p in privileges {
            if self.authorize(principal, resource, p.clone()).await? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    async fn grant(&self, principal: Uuid, resource: Uuid, privilege: Privilege) -> Result<(), UcError> {
        let mut enforcer = self.enforcer.write().await;
        enforcer
            .add_policy(vec![
                principal.to_string(),
                resource.to_string(),
                privilege.as_casbin_str().to_string(),
            ])
            .await
            .map_err(|e| UcError::new(ErrorCode::Internal, format!("Casbin add_policy failed: {}", e)))?;
        Ok(())
    }

    async fn revoke(&self, principal: Uuid, resource: Uuid, privilege: Privilege) -> Result<(), UcError> {
        let mut enforcer = self.enforcer.write().await;
        enforcer
            .remove_policy(vec![
                principal.to_string(),
                resource.to_string(),
                privilege.as_casbin_str().to_string(),
            ])
            .await
            .map_err(|e| UcError::new(ErrorCode::Internal, format!("Casbin remove_policy failed: {}", e)))?;
        Ok(())
    }

    async fn add_hierarchy_child(&self, parent: Uuid, child: Uuid) -> Result<(), UcError> {
        let mut enforcer = self.enforcer.write().await;
        // g2 grouping: child inherits permissions of parent
        enforcer
            .add_named_grouping_policy("g2", vec![child.to_string(), parent.to_string()])
            .await
            .map_err(|e| UcError::new(ErrorCode::Internal, format!("Casbin hierarchy failed: {}", e)))?;
        Ok(())
    }

    async fn remove_hierarchy_children(&self, resource: Uuid) -> Result<(), UcError> {
        let mut enforcer = self.enforcer.write().await;
        enforcer
            .remove_named_grouping_policy("g2", vec![resource.to_string(), "".to_string()])
            .await
            .map_err(|e| UcError::new(ErrorCode::Internal, format!("Casbin remove hierarchy failed: {}", e)))?;
        Ok(())
    }

    async fn list_privileges(&self, principal: Uuid, resource: Uuid) -> Result<Vec<Privilege>, UcError> {
        let enforcer = self.enforcer.read().await;
        let policies = enforcer.get_policy();
        let privs = policies.iter()
            .filter(|p| p.len() >= 3 && p[0] == principal.to_string() && p[1] == resource.to_string())
            .filter_map(|p| Privilege::from_casbin_str(&p[2]))
            .collect();
        Ok(privs)
    }

    async fn list_grants_on_resource(&self, resource: Uuid) -> Result<Vec<(Uuid, Vec<Privilege>)>, UcError> {
        let enforcer = self.enforcer.read().await;
        let policies = enforcer.get_policy();
        let mut map: std::collections::HashMap<Uuid, Vec<Privilege>> = std::collections::HashMap::new();
        for p in policies.iter().filter(|p| p.len() >= 3 && p[1] == resource.to_string()) {
            if let (Ok(principal), Some(p_priv)) = (p[0].parse::<Uuid>(), Privilege::from_casbin_str(&p[2])) {
                map.entry(principal).or_default().push(p_priv);
            }
        }
        Ok(map.into_iter().collect())
    }
}

// ── AllowingAuthorizer (auth disabled mode) ───────────────────────────────────

pub struct AllowingAuthorizer;

#[async_trait::async_trait]
impl Authorizer for AllowingAuthorizer {
    async fn authorize(&self, _p: Uuid, _r: Uuid, _priv: Privilege) -> Result<bool, UcError> { Ok(true) }
    async fn authorize_any(&self, _p: Uuid, _r: Uuid, _privs: &[Privilege]) -> Result<bool, UcError> { Ok(true) }
    async fn grant(&self, _p: Uuid, _r: Uuid, _priv: Privilege) -> Result<(), UcError> { Ok(()) }
    async fn revoke(&self, _p: Uuid, _r: Uuid, _priv: Privilege) -> Result<(), UcError> { Ok(()) }
    async fn add_hierarchy_child(&self, _parent: Uuid, _child: Uuid) -> Result<(), UcError> { Ok(()) }
    async fn remove_hierarchy_children(&self, _r: Uuid) -> Result<(), UcError> { Ok(()) }
    // No-auth mode has no policy data — return empty rather than fabricating OWNER grants
    async fn list_privileges(&self, _p: Uuid, _r: Uuid) -> Result<Vec<Privilege>, UcError> { Ok(vec![]) }
    async fn list_grants_on_resource(&self, _r: Uuid) -> Result<Vec<(Uuid, Vec<Privilege>)>, UcError> { Ok(vec![]) }
}
