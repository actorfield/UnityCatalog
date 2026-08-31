//! Casbin policy storage, SQL body.
//!
//! Lifted out of uc-auth's adapter, which spoke sqlx directly. The adapter now
//! calls these, so it works against either backend.

use crate::models::casbin::CasbinRule;
use crate::pool::AnyPool;
use crate::IntoUcResult;
use uc_errors::UcError;

/// All rules in insertion order (`ORDER BY id`). Casbin does not require an
/// order, but a stable one keeps `save_policy` round-trips reproducible.
pub async fn load_all(pool: &AnyPool) -> Result<Vec<CasbinRule>, UcError> {
    sqlx::query_as::<_, CasbinRule>(
        "SELECT ptype, v0, v1, v2, v3, v4, v5 FROM casbin_rule ORDER BY id",
    )
    .fetch_all(pool)
    .await
    .uc_err()
}

/// Returns false when the rule was already present.
pub async fn insert(pool: &AnyPool, rule: &CasbinRule) -> Result<bool, UcError> {
    let v = rule.values();
    let result = sqlx::query(
        "INSERT OR IGNORE INTO casbin_rule (ptype, v0, v1, v2, v3, v4, v5) \
         VALUES ($1,$2,$3,$4,$5,$6,$7)",
    )
    .bind(&rule.ptype)
    .bind(v[0])
    .bind(v[1])
    .bind(v[2])
    .bind(v[3])
    .bind(v[4])
    .bind(v[5])
    .execute(pool)
    .await
    .uc_err()?;
    Ok(result.rows_affected() > 0)
}

/// Returns false when no such rule existed.
pub async fn delete(pool: &AnyPool, rule: &CasbinRule) -> Result<bool, UcError> {
    let v = rule.values();
    let result = sqlx::query(
        "DELETE FROM casbin_rule WHERE ptype=$1 AND v0=$2 AND v1=$3 AND v2=$4 \
         AND v3=$5 AND v4=$6 AND v5=$7",
    )
    .bind(&rule.ptype)
    .bind(v[0])
    .bind(v[1])
    .bind(v[2])
    .bind(v[3])
    .bind(v[4])
    .bind(v[5])
    .execute(pool)
    .await
    .uc_err()?;
    Ok(result.rows_affected() > 0)
}

/// True if any rule was removed.
pub async fn delete_many(pool: &AnyPool, rules: &[CasbinRule]) -> Result<bool, UcError> {
    let mut any = false;
    for rule in rules {
        if delete(pool, rule).await? {
            any = true;
        }
    }
    Ok(any)
}

/// Replace the entire policy set.
///
/// Wrapped in a transaction to keep the window where the table is empty as
/// short as possible — an authorizer reading mid-replace would otherwise see no
/// policy and deny everything.
pub async fn replace_all(pool: &AnyPool, rules: &[CasbinRule]) -> Result<(), UcError> {
    let mut tx = pool.begin().await.uc_err()?;
    sqlx::query("DELETE FROM casbin_rule")
        .execute(&mut *tx)
        .await
        .uc_err()?;
    for rule in rules {
        let v = rule.values();
        sqlx::query(
            "INSERT OR IGNORE INTO casbin_rule (ptype, v0, v1, v2, v3, v4, v5) \
             VALUES ($1,$2,$3,$4,$5,$6,$7)",
        )
        .bind(&rule.ptype)
        .bind(v[0])
        .bind(v[1])
        .bind(v[2])
        .bind(v[3])
        .bind(v[4])
        .bind(v[5])
        .execute(&mut *tx)
        .await
        .uc_err()?;
    }
    tx.commit().await.uc_err()
}

pub async fn clear(pool: &AnyPool) -> Result<(), UcError> {
    sqlx::query("DELETE FROM casbin_rule")
        .execute(pool)
        .await
        .uc_err()?;
    Ok(())
}
