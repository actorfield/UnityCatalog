use serde::{Deserialize, Serialize};

/// One row of the casbin policy table.
///
/// Deliberately carries no id. The SQL table uses an INTEGER AUTOINCREMENT
/// surrogate and the log store uses a UUID; neither is meaningful to casbin,
/// whose identity for a rule is the (ptype, v0..v5) tuple. Keeping the id out
/// of the shared type is what lets one repo API serve both.
#[cfg_attr(feature = "sql", derive(sqlx::FromRow))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CasbinRule {
    pub ptype: String,
    pub v0: String,
    pub v1: String,
    pub v2: String,
    pub v3: String,
    pub v4: String,
    pub v5: String,
}

impl CasbinRule {
    /// Build from casbin's `(ptype, rule)` pair, padding to six columns.
    pub fn from_parts(ptype: &str, rule: &[String]) -> Self {
        let mut v: Vec<String> = rule.to_vec();
        v.resize(6, String::new());
        Self {
            ptype: ptype.to_string(),
            v0: v[0].clone(),
            v1: v[1].clone(),
            v2: v[2].clone(),
            v3: v[3].clone(),
            v4: v[4].clone(),
            v5: v[5].clone(),
        }
    }

    pub fn values(&self) -> [&str; 6] {
        [&self.v0, &self.v1, &self.v2, &self.v3, &self.v4, &self.v5]
    }

    /// Back to casbin's form: trailing empty columns dropped.
    pub fn to_policy(&self) -> Vec<String> {
        self.values()
            .iter()
            .rev()
            .skip_while(|s| s.is_empty())
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .map(|s| s.to_string())
            .collect()
    }
}
