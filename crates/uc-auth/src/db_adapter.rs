/// Custom casbin adapter backed by the existing sqlx pool.
/// Persists policies to the `casbin_rule` table (already created by migrations).
/// Implements casbin's Adapter trait so UcAuthorizer survives restarts.
use async_trait::async_trait;
use casbin::{Adapter, Filter, Model, Result as CasbinResult};

/// A single row in the casbin_rule table.
#[derive(sqlx::FromRow, Debug, Clone)]
struct CasbinRuleRow {
    pub ptype: String,
    pub v0: String,
    pub v1: String,
    pub v2: String,
    pub v3: String,
    pub v4: String,
    pub v5: String,
}

impl CasbinRuleRow {
    fn to_policy(&self) -> Vec<String> {
        let vals = [&self.v0, &self.v1, &self.v2, &self.v3, &self.v4, &self.v5];
        vals.iter()
            .map(|s| s.as_str())
            .rev()
            .skip_while(|s| s.is_empty())
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .map(|s| s.to_string())
            .collect()
    }
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
        sqlx::query_as::<_, CasbinRuleRow>(
            "SELECT ptype, v0, v1, v2, v3, v4, v5 FROM casbin_rule ORDER BY id",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| casbin::Error::AdapterError(casbin::error::AdapterError(Box::new(e))))
    }

    async fn insert_rule(&self, ptype: &str, rule: &[String]) -> CasbinResult<bool> {
        let vals: Vec<&str> = rule.iter().map(|s| s.as_str()).collect();
        let v: Vec<&str> = {
            let mut v = vals.clone();
            v.resize(6, "");
            v
        };
        let result = sqlx::query(
            "INSERT OR IGNORE INTO casbin_rule (ptype, v0, v1, v2, v3, v4, v5) VALUES ($1,$2,$3,$4,$5,$6,$7)",
        )
        .bind(ptype)
        .bind(v[0]).bind(v[1]).bind(v[2]).bind(v[3]).bind(v[4]).bind(v[5])
        .execute(&self.pool)
        .await
        .map_err(|e| casbin::Error::AdapterError(casbin::error::AdapterError(Box::new(e))))?;
        Ok(result.rows_affected() > 0)
    }

    async fn delete_rule(&self, ptype: &str, rule: &[String]) -> CasbinResult<bool> {
        let vals: Vec<&str> = rule.iter().map(|s| s.as_str()).collect();
        let v: Vec<&str> = {
            let mut v = vals.clone();
            v.resize(6, "");
            v
        };
        let result = sqlx::query(
            "DELETE FROM casbin_rule WHERE ptype=$1 AND v0=$2 AND v1=$3 AND v2=$4 AND v3=$5 AND v4=$6 AND v5=$7",
        )
        .bind(ptype)
        .bind(v[0]).bind(v[1]).bind(v[2]).bind(v[3]).bind(v[4]).bind(v[5])
        .execute(&self.pool)
        .await
        .map_err(|e| casbin::Error::AdapterError(casbin::error::AdapterError(Box::new(e))))?;
        Ok(result.rows_affected() > 0)
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

        let mut tx =
            self.pool.begin().await.map_err(|e| {
                casbin::Error::AdapterError(casbin::error::AdapterError(Box::new(e)))
            })?;
        sqlx::query("DELETE FROM casbin_rule")
            .execute(&mut *tx)
            .await
            .map_err(|e| casbin::Error::AdapterError(casbin::error::AdapterError(Box::new(e))))?;
        for (ptype, rule) in &all_rules {
            let vals: Vec<&str> = rule.iter().map(|s| s.as_str()).collect();
            let v: Vec<&str> = {
                let mut v = vals.clone();
                v.resize(6, "");
                v
            };
            sqlx::query(
                "INSERT OR IGNORE INTO casbin_rule (ptype, v0, v1, v2, v3, v4, v5) VALUES ($1,$2,$3,$4,$5,$6,$7)",
            )
            .bind(ptype.as_str())
            .bind(v[0]).bind(v[1]).bind(v[2]).bind(v[3]).bind(v[4]).bind(v[5])
            .execute(&mut *tx)
            .await
            .map_err(|e| casbin::Error::AdapterError(casbin::error::AdapterError(Box::new(e))))?;
        }
        tx.commit()
            .await
            .map_err(|e| casbin::Error::AdapterError(casbin::error::AdapterError(Box::new(e))))?;
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
        // Use fully parameterized DELETE to prevent SQL injection.
        // Columns v0..v5 are fixed schema — we select the right WHERE clause
        // by fetching all matching rows and deleting by their IDs.
        let rows = self.load_all().await?;
        let _col_names = ["v0", "v1", "v2", "v3", "v4", "v5"];
        let mut deleted = false;
        for row in &rows {
            if row.ptype != ptype {
                continue;
            }
            let row_vals = [&row.v0, &row.v1, &row.v2, &row.v3, &row.v4, &row.v5];
            let matches = field_values.iter().enumerate().all(|(i, val)| {
                if val.is_empty() {
                    true
                } else {
                    row_vals[field_index + i] == val
                }
            });
            if matches {
                let result = sqlx::query(
                    "DELETE FROM casbin_rule WHERE ptype=$1 AND v0=$2 AND v1=$3 AND v2=$4 AND v3=$5 AND v4=$6 AND v5=$7"
                )
                .bind(&row.ptype).bind(&row.v0).bind(&row.v1).bind(&row.v2)
                .bind(&row.v3).bind(&row.v4).bind(&row.v5)
                .execute(&self.pool)
                .await
                .map_err(|e| casbin::Error::AdapterError(casbin::error::AdapterError(Box::new(e))))?;
                if result.rows_affected() > 0 {
                    deleted = true;
                }
            }
        }
        Ok(deleted)
    }

    fn is_filtered(&self) -> bool {
        self.is_filtered
    }

    async fn clear_policy(&mut self) -> CasbinResult<()> {
        sqlx::query("DELETE FROM casbin_rule")
            .execute(&self.pool)
            .await
            .map_err(|e| casbin::Error::AdapterError(casbin::error::AdapterError(Box::new(e))))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Authorizer, UcAuthorizer};
    use uc_db::AnyPool;
    use uc_types::Privilege;
    use uuid::Uuid;

    async fn in_memory_sqlite() -> AnyPool {
        let pool = AnyPool::connect("sqlite::memory:").await.unwrap();
        uc_db::pool::run_migrations(&pool).await.unwrap();
        pool
    }

    /// Grant a privilege, simulate restart by creating a fresh authorizer
    /// backed by the same DB, then verify the privilege is still enforced.
    /// This is the regression test for the load_policy sec/"" bug.
    #[tokio::test]
    async fn policies_survive_restart() {
        let pool = in_memory_sqlite().await;

        let principal = Uuid::new_v4();
        let resource = Uuid::new_v4();

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
        let pool = in_memory_sqlite().await;

        let admin = Uuid::new_v4();
        let metastore = Uuid::new_v4();

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
        let pool = in_memory_sqlite().await;

        let principal = Uuid::new_v4();
        let catalog = Uuid::new_v4();

        let auth1 = UcAuthorizer::new_with_db(pool.clone()).await.unwrap();
        auth1.grant(principal, catalog, Privilege::Owner).await.unwrap();

        // Simulate restart — fresh enforcer loads g/g2/g3 from the same DB.
        let auth2 = UcAuthorizer::new_with_db(pool.clone()).await.unwrap();
        for req in [
            Privilege::CreateSchema,
            Privilege::CreateTable,
            Privilege::CreateVolume,
            Privilege::Select,
        ] {
            assert!(
                auth2.authorize(principal, catalog, req.clone()).await.unwrap(),
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
        let pool = in_memory_sqlite().await;
        let principal = Uuid::new_v4();
        let catalog = Uuid::new_v4();
        let schema = Uuid::new_v4();

        let auth1 = UcAuthorizer::new_with_db(pool.clone()).await.unwrap();
        auth1.grant(principal, catalog, Privilege::Owner).await.unwrap();
        auth1.add_hierarchy_child(catalog, schema).await.unwrap();

        // Force a full snapshot save (the path that previously mislabeled ptypes).
        auth1.force_save_policy().await.unwrap();

        // Reload from the snapshot and verify both hierarchies survived.
        let auth2 = UcAuthorizer::new_with_db(pool.clone()).await.unwrap();
        assert!(
            auth2.authorize(principal, catalog, Privilege::CreateSchema).await.unwrap(),
            "g3 (OWNER→CREATE_SCHEMA) must survive a save_policy snapshot"
        );
        assert!(
            auth2.authorize(principal, schema, Privilege::CreateTable).await.unwrap(),
            "g2 (catalog→schema) + g3 must survive a save_policy snapshot"
        );
    }
}
