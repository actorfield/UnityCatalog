use serde::{Deserialize, Serialize};

/// One row of the casbin policy table.
///
/// Deliberately carries no id. The SQL table uses an INTEGER AUTOINCREMENT
/// surrogate and the log store uses a UUID; neither is meaningful to casbin,
/// whose identity for a rule is the (ptype, v0..v5) tuple. Keeping the id out
/// of the shared type is what lets one repo API serve both.
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
        // Padded by iterator rather than `resize` + indexing: a rule longer or
        // shorter than six columns is then structurally impossible to mishandle
        // instead of relying on the resize two lines up.
        let mut v = rule.iter().cloned().chain(std::iter::repeat(String::new()));
        let mut next = || v.next().unwrap_or_default();
        Self {
            ptype: ptype.to_string(),
            v0: next(),
            v1: next(),
            v2: next(),
            v3: next(),
            v4: next(),
            v5: next(),
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
