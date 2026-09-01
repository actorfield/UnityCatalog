/// Casbin adapter over the uc-db repo layer.
///
/// Persists policies to `casbin_rule` so UcAuthorizer survives restarts. It
/// speaks no SQL of its own: every statement lives in `uc_db::repos::casbin`,
/// which has a body for each backend, so this adapter works against either.
use async_trait::async_trait;
use casbin::{Adapter, Filter, Model, Result as CasbinResult};
use uc_db::models::casbin::CasbinRule as CasbinRuleRow;
use uc_db::repos::casbin as casbin_repo;

/// Wrap a repo error as a casbin adapter error.
fn adapter_err(e: uc_errors::UcError) -> casbin::Error {
    casbin::Error::AdapterError(casbin::error::AdapterError(Box::new(e)))
}

pub struct SqlxAdapter {
    pool: uc_db::AnyPool,
    is_filtered: bool,
}

impl SqlxAdapter {
    pub async fn new(pool: uc_db::AnyPool) -> CasbinResult<Self> {
        Ok(Self {
            pool,
            is_filtered: false,
        })
    }

    async fn load_all(&self) -> CasbinResult<Vec<CasbinRuleRow>> {
        casbin_repo::load_all(&self.pool).await.map_err(adapter_err)
    }

    async fn insert_rule(&self, ptype: &str, rule: &[String]) -> CasbinResult<bool> {
        casbin_repo::insert(&self.pool, &CasbinRuleRow::from_parts(ptype, rule))
            .await
            .map_err(adapter_err)
    }

    async fn delete_rule(&self, ptype: &str, rule: &[String]) -> CasbinResult<bool> {
        casbin_repo::delete(&self.pool, &CasbinRuleRow::from_parts(ptype, rule))
            .await
            .map_err(adapter_err)
    }
}

#[async_trait]
impl Adapter for SqlxAdapter {
    async fn load_policy(&mut self, model: &mut dyn Model) -> CasbinResult<()> {
        let rows = self.load_all().await?;
        for row in rows {
            let policy = row.to_policy();
            // Casbin has exactly two policy sections: "p" (policies) and "g"
            // (role/grouping links). The *ptype* is the type key within the
            // section — "p"/"p2"… under "p", and "g"/"g2"/"g3"… under "g".
            // Every grouping ptype (g, g2 object-hierarchy, g3 privilege-
            // hierarchy) MUST load under section "g" so build_role_links wires
            // up its transitive closure. Mapping a ptype to a section that
            // doesn't exist (e.g. "g3") silently drops those rows on load, so
            // they appear to persist but are ignored on every restart — which
            // broke OWNER→ALL_PRIVILEGES→{specific} until re-seeded. Derive the
            // section from the ptype's family instead of enumerating variants.
            let sec = if row.ptype.starts_with('g') { "g" } else { "p" };
            let _ = model.add_policy(sec, &row.ptype, policy);
        }
        Ok(())
    }

    async fn load_filtered_policy<'a>(
        &mut self,
        model: &mut dyn Model,
        _filter: Filter<'a>,
    ) -> CasbinResult<()> {
        // Simplified: load all (filtering not needed for our use case)
        self.is_filtered = false;
        self.load_policy(model).await
    }

    async fn save_policy(&mut self, model: &mut dyn Model) -> CasbinResult<()> {
        // Collect all rules from the model first, then run delete+insert in a single
        // transaction to minimise the window where the table is empty.
        let mut all_rules: Vec<(String, Vec<String>)> = Vec::new();
        // casbin's model is keyed by SECTION ("p" for policies, "g" for
        // role/grouping links) at the top level, and by *ptype* inside each
        // section ("p"/"p2"… under "p"; "g"/"g2"/"g3"… under "g"). Persist the
        // real inner ptype, not the section name — otherwise g2/g3 rules would
        // be written as "g" (and g3, the privilege hierarchy, silently
        // dropped), the write-side twin of the load_policy sectioning bug.
        for sec in &["p", "g"] {
            if let Some(section) = model.get_model().get(*sec) {
                for (ptype, assertion) in section {
                    for rule in &assertion.policy {
                        all_rules.push((ptype.clone(), rule.clone()));
                    }
                }
            }
        }

        let rules: Vec<CasbinRuleRow> = all_rules
            .iter()
            .map(|(ptype, rule)| CasbinRuleRow::from_parts(ptype, rule))
            .collect();
        casbin_repo::replace_all(&self.pool, &rules)
            .await
            .map_err(adapter_err)?;
        Ok(())
    }

    async fn add_policy(
        &mut self,
        _sec: &str,
        ptype: &str,
        rule: Vec<String>,
    ) -> CasbinResult<bool> {
        self.insert_rule(ptype, &rule).await
    }

    async fn add_policies(
        &mut self,
        _sec: &str,
        ptype: &str,
        rules: Vec<Vec<String>>,
    ) -> CasbinResult<bool> {
        let mut all_ok = true;
        for rule in rules {
            if !self.insert_rule(ptype, &rule).await? {
                all_ok = false;
            }
        }
        Ok(all_ok)
    }

    async fn remove_policy(
        &mut self,
        _sec: &str,
        ptype: &str,
        rule: Vec<String>,
    ) -> CasbinResult<bool> {
        self.delete_rule(ptype, &rule).await
    }

    async fn remove_policies(
        &mut self,
        _sec: &str,
        ptype: &str,
        rules: Vec<Vec<String>>,
    ) -> CasbinResult<bool> {
        let mut all_ok = true;
        for rule in rules {
            if !self.delete_rule(ptype, &rule).await? {
                all_ok = false;
            }
        }
        Ok(all_ok)
    }

    async fn remove_filtered_policy(
        &mut self,
        _sec: &str,
        ptype: &str,
        field_index: usize,
        field_values: Vec<String>,
    ) -> CasbinResult<bool> {
        // The filter is evaluated here rather than in SQL: v0..v5 are fixed
        // columns but `field_index` is dynamic, so building the WHERE clause
        // would mean interpolating a column name.
        let rows = self.load_all().await?;
        let doomed: Vec<CasbinRuleRow> = rows
            .into_iter()
            .filter(|row| {
                if row.ptype != ptype {
                    return false;
                }
                let vals = row.values();
                // `.get`, not `[]`: field_index and field_values come from
                // casbin, and nothing constrains field_index + len() to the six
                // columns. Out of range means the rule has no such field, so it
                // does not match -- and crucially is not deleted.
                field_values.iter().enumerate().all(|(i, val)| {
                    val.is_empty() || vals.get(field_index + i).is_some_and(|v| v == val)
                })
            })
            .collect();
        // One call for the batch, so on the log store a filtered removal lands
        // as a single commit rather than a run of independent deletes.
        casbin_repo::delete_many(&self.pool, &doomed)
            .await
            .map_err(adapter_err)
    }

    fn is_filtered(&self) -> bool {
        self.is_filtered
    }

    async fn clear_policy(&mut self) -> CasbinResult<()> {
        casbin_repo::clear(&self.pool).await.map_err(adapter_err)
    }
}

#[cfg(test)]
mod tests {
    // Tests panic on purpose; see the note in the crate-level modules.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

    use crate::{Authorizer, UcAuthorizer};
    use uc_db::AnyPool;
    use uc_types::Privilege;
    use uuid::Uuid;

    /// A fresh, empty store for each test.
    async fn fresh_store() -> AnyPool {
        use std::sync::Arc;
        AnyPool::open(Arc::new(uc_db::store::memory::MemoryLog::new()))
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn policies_survive_restart() {
        let pool = fresh_store().await;

        let principal = Uuid::now_v7();
        let resource = Uuid::now_v7();

        // First "run" — grant Owner
        let auth1 = UcAuthorizer::new_with_db(pool.clone()).await.unwrap();
        auth1
            .grant(principal, resource, Privilege::Owner)
            .await
            .unwrap();
        assert!(auth1
            .authorize(principal, resource, Privilege::Owner)
            .await
            .unwrap());

        // Simulate restart — new authorizer, same DB
        let auth2 = UcAuthorizer::new_with_db(pool.clone()).await.unwrap();
        assert!(
            auth2
                .authorize(principal, resource, Privilege::Owner)
                .await
                .unwrap(),
            "Owner privilege must survive a restart (load_policy must use correct sec key)"
        );
    }

    #[tokio::test]
    async fn create_catalog_allowed_for_metastore_owner_after_restart() {
        let pool = fresh_store().await;

        let admin = Uuid::now_v7();
        let metastore = Uuid::now_v7();

        let auth1 = UcAuthorizer::new_with_db(pool.clone()).await.unwrap();
        auth1
            .grant(admin, metastore, Privilege::Owner)
            .await
            .unwrap();

        // Simulate restart
        let auth2 = UcAuthorizer::new_with_db(pool.clone()).await.unwrap();
        let allowed = auth2
            .authorize_any(
                admin,
                metastore,
                &[Privilege::CreateCatalog, Privilege::Owner],
            )
            .await
            .unwrap();
        assert!(
            allowed,
            "Admin with Owner on metastore must be allowed to create catalogs after restart"
        );
    }

    /// Regression for the g3 (privilege-hierarchy) load bug: after a restart,
    /// OWNER must still *imply* a specific privilege via g3
    /// (OWNER→ALL_PRIVILEGES→CREATE_SCHEMA). The earlier restart tests only
    /// checked a direct OWNER match (or authorize_any with OWNER in the list),
    /// so a dropped g3 section passed them while real schema/table/volume
    /// creation 403'd on every fresh uc-server pod. This asserts the transitive
    /// expansion specifically, with OWNER absent from the checked privilege.
    #[tokio::test]
    async fn owner_implies_specific_privilege_via_g3_after_restart() {
        let pool = fresh_store().await;

        let principal = Uuid::now_v7();
        let catalog = Uuid::now_v7();

        let auth1 = UcAuthorizer::new_with_db(pool.clone()).await.unwrap();
        auth1
            .grant(principal, catalog, Privilege::Owner)
            .await
            .unwrap();

        // Simulate restart — fresh enforcer loads g/g2/g3 from the same DB.
        let auth2 = UcAuthorizer::new_with_db(pool.clone()).await.unwrap();
        for req in [
            Privilege::CreateSchema,
            Privilege::CreateTable,
            Privilege::CreateVolume,
            Privilege::Select,
        ] {
            assert!(
                auth2
                    .authorize(principal, catalog, req.clone())
                    .await
                    .unwrap(),
                "OWNER must imply {:?} via g3 after restart (g3 must load into section \"g\")",
                req
            );
        }
    }

    /// Write-side twin of the sectioning bug: an explicit full `save_policy`
    /// snapshot must round-trip g2 (object hierarchy) and g3 (privilege
    /// hierarchy) with their real ptypes, not collapse them into "g" or drop
    /// them. Grant OWNER on a catalog, cascade a child schema, snapshot, then
    /// reload and confirm OWNER still cascades AND implies a specific privilege.
    #[tokio::test]
    async fn save_policy_snapshot_preserves_g2_and_g3() {
        let pool = fresh_store().await;
        let principal = Uuid::now_v7();
        let catalog = Uuid::now_v7();
        let schema = Uuid::now_v7();

        let auth1 = UcAuthorizer::new_with_db(pool.clone()).await.unwrap();
        auth1
            .grant(principal, catalog, Privilege::Owner)
            .await
            .unwrap();
        auth1.add_hierarchy_child(catalog, schema).await.unwrap();

        // Force a full snapshot save (the path that previously mislabeled ptypes).
        auth1.force_save_policy().await.unwrap();

        // Reload from the snapshot and verify both hierarchies survived.
        let auth2 = UcAuthorizer::new_with_db(pool.clone()).await.unwrap();
        assert!(
            auth2
                .authorize(principal, catalog, Privilege::CreateSchema)
                .await
                .unwrap(),
            "g3 (OWNER→CREATE_SCHEMA) must survive a save_policy snapshot"
        );
        assert!(
            auth2
                .authorize(principal, schema, Privilege::CreateTable)
                .await
                .unwrap(),
            "g2 (catalog→schema) + g3 must survive a save_policy snapshot"
        );
    }
}
