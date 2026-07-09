use serde::{Deserialize, Serialize};

// ── Privilege ─────────────────────────────────────────────────────────────────

/// Mirrors Java's io.unitycatalog.server.persist.model.Privileges enum exactly.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Privilege {
    Owner,
    /// Databricks umbrella privilege: implies every concrete privilege on a
    /// securable (but not OWNER's grant-management authority).
    AllPrivileges,
    CreateCatalog,
    UseCatalog,
    CreateSchema,
    UseSchema,
    CreateTable,
    Select,
    Modify,
    CreateFunction,
    Execute,
    CreateVolume,
    ReadVolume,
    CreateModel,
    CreateExternalLocation,
    ReadFiles,
    WriteFiles,
    CreateExternalTable,
    CreateExternalVolume,
    CreateManagedStorage,
    CreateStorageCredential,
}

impl Privilege {
    /// String stored in casbin policy rows (v2 column).
    pub fn as_casbin_str(&self) -> &'static str {
        match self {
            Self::Owner => "OWNER",
            Self::AllPrivileges => "ALL_PRIVILEGES",
            Self::CreateCatalog => "CREATE_CATALOG",
            Self::UseCatalog => "USE_CATALOG",
            Self::CreateSchema => "CREATE_SCHEMA",
            Self::UseSchema => "USE_SCHEMA",
            Self::CreateTable => "CREATE_TABLE",
            Self::Select => "SELECT",
            Self::Modify => "MODIFY",
            Self::CreateFunction => "CREATE_FUNCTION",
            Self::Execute => "EXECUTE",
            Self::CreateVolume => "CREATE_VOLUME",
            Self::ReadVolume => "READ_VOLUME",
            Self::CreateModel => "CREATE_MODEL",
            Self::CreateExternalLocation => "CREATE_EXTERNAL_LOCATION",
            Self::ReadFiles => "READ_FILES",
            Self::WriteFiles => "WRITE_FILES",
            Self::CreateExternalTable => "CREATE_EXTERNAL_TABLE",
            Self::CreateExternalVolume => "CREATE_EXTERNAL_VOLUME",
            Self::CreateManagedStorage => "CREATE_MANAGED_STORAGE",
            Self::CreateStorageCredential => "CREATE_STORAGE_CREDENTIAL",
        }
    }

    pub fn from_casbin_str(s: &str) -> Option<Self> {
        match s {
            "OWNER" => Some(Self::Owner),
            "ALL_PRIVILEGES" => Some(Self::AllPrivileges),
            "CREATE_CATALOG" => Some(Self::CreateCatalog),
            "USE_CATALOG" => Some(Self::UseCatalog),
            "CREATE_SCHEMA" => Some(Self::CreateSchema),
            "USE_SCHEMA" => Some(Self::UseSchema),
            "CREATE_TABLE" => Some(Self::CreateTable),
            "SELECT" => Some(Self::Select),
            "MODIFY" => Some(Self::Modify),
            "CREATE_FUNCTION" => Some(Self::CreateFunction),
            "EXECUTE" => Some(Self::Execute),
            "CREATE_VOLUME" => Some(Self::CreateVolume),
            "READ_VOLUME" => Some(Self::ReadVolume),
            "CREATE_MODEL" => Some(Self::CreateModel),
            "CREATE_EXTERNAL_LOCATION" => Some(Self::CreateExternalLocation),
            "READ_FILES" => Some(Self::ReadFiles),
            "WRITE_FILES" => Some(Self::WriteFiles),
            "CREATE_EXTERNAL_TABLE" => Some(Self::CreateExternalTable),
            "CREATE_EXTERNAL_VOLUME" => Some(Self::CreateExternalVolume),
            "CREATE_MANAGED_STORAGE" => Some(Self::CreateManagedStorage),
            "CREATE_STORAGE_CREDENTIAL" => Some(Self::CreateStorageCredential),
            _ => None,
        }
    }

    /// Every concrete privilege — excludes the umbrella `Owner`/`AllPrivileges`,
    /// which imply these via the hierarchy rather than being requested directly.
    pub fn specific() -> &'static [Privilege] {
        use Privilege::*;
        &[
            CreateCatalog,
            UseCatalog,
            CreateSchema,
            UseSchema,
            CreateTable,
            Select,
            Modify,
            CreateFunction,
            Execute,
            CreateVolume,
            ReadVolume,
            CreateModel,
            CreateExternalLocation,
            ReadFiles,
            WriteFiles,
            CreateExternalTable,
            CreateExternalVolume,
            CreateManagedStorage,
            CreateStorageCredential,
        ]
    }

    /// Privilege-implication edges for the casbin `g3` grouping, expressed as
    /// data (seeded into `casbin_rule`, not hardcoded in the matcher):
    ///   `OWNER -> ALL_PRIVILEGES -> {every specific privilege}`.
    /// A grant of a parent action satisfies a request for any descendant, so
    /// e.g. an OWNER grant authorizes SELECT/MODIFY/READ_VOLUME/… centrally.
    pub fn hierarchy_edges() -> Vec<(&'static str, &'static str)> {
        let mut edges = vec![(
            Privilege::Owner.as_casbin_str(),
            Privilege::AllPrivileges.as_casbin_str(),
        )];
        for p in Privilege::specific() {
            edges.push((Privilege::AllPrivileges.as_casbin_str(), p.as_casbin_str()));
        }
        edges
    }
}

// ── UriScheme ─────────────────────────────────────────────────────────────────

/// Storage location URI schemes, used to dispatch to the correct credential vendor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UriScheme {
    S3,
    Abfs,
    Abfss,
    Gs,
    File,
    Null,
}

impl UriScheme {
    pub fn from_url(url: &str) -> Self {
        let lower = url.to_lowercase();
        if lower.starts_with("s3://") || lower.starts_with("s3a://") {
            Self::S3
        } else if lower.starts_with("abfss://") {
            Self::Abfss
        } else if lower.starts_with("abfs://") {
            Self::Abfs
        } else if lower.starts_with("gs://") {
            Self::Gs
        } else if lower.starts_with("file://") || lower.starts_with('/') {
            Self::File
        } else {
            Self::Null
        }
    }
}

// ── TokenType ─────────────────────────────────────────────────────────────────

/// JWT token_type claim — mirrors Java's JwtTokenType enum.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TokenType {
    #[serde(rename = "ACCESS")]
    Access,
    #[serde(rename = "SERVICE")]
    Service,
}

// ── SecurableType ─────────────────────────────────────────────────────────────

/// Resource types that can have permissions attached.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SecurableType {
    Metastore,
    Catalog,
    Schema,
    Table,
    Volume,
    Function,
    RegisteredModel,
    ExternalLocation,
    StorageCredential,
}

impl SecurableType {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_uppercase().as_str() {
            "METASTORE" => Some(Self::Metastore),
            "CATALOG" => Some(Self::Catalog),
            "SCHEMA" => Some(Self::Schema),
            "TABLE" => Some(Self::Table),
            "VOLUME" => Some(Self::Volume),
            "FUNCTION" => Some(Self::Function),
            "REGISTERED_MODEL" | "MODEL" => Some(Self::RegisteredModel),
            "EXTERNAL_LOCATION" => Some(Self::ExternalLocation),
            "STORAGE_CREDENTIAL" => Some(Self::StorageCredential),
            _ => None,
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn privilege_casbin_round_trip() {
        let privs = [
            Privilege::Owner,
            Privilege::UseCatalog,
            Privilege::CreateTable,
            Privilege::WriteFiles,
            Privilege::CreateStorageCredential,
        ];
        for p in &privs {
            let s = p.as_casbin_str();
            let back = Privilege::from_casbin_str(s).unwrap();
            assert_eq!(*p, back, "round-trip failed for {:?}", p);
        }
    }

    #[test]
    fn uri_scheme_detection() {
        assert_eq!(UriScheme::from_url("s3://my-bucket/path"), UriScheme::S3);
        assert_eq!(UriScheme::from_url("s3a://bucket/path"), UriScheme::S3);
        assert_eq!(
            UriScheme::from_url("abfs://container@account"),
            UriScheme::Abfs
        );
        assert_eq!(
            UriScheme::from_url("abfss://container@account"),
            UriScheme::Abfss
        );
        assert_eq!(UriScheme::from_url("gs://bucket/path"), UriScheme::Gs);
        assert_eq!(UriScheme::from_url("/local/path"), UriScheme::File);
        assert_eq!(UriScheme::from_url("file:///local/path"), UriScheme::File);
        assert_eq!(UriScheme::from_url("unknown://x"), UriScheme::Null);
    }

    #[test]
    fn all_privileges_round_trip_casbin() {
        // Every privilege must survive as_casbin_str → from_casbin_str
        let all = [
            Privilege::Owner,
            Privilege::AllPrivileges,
            Privilege::CreateCatalog,
            Privilege::UseCatalog,
            Privilege::CreateSchema,
            Privilege::UseSchema,
            Privilege::CreateTable,
            Privilege::Select,
            Privilege::Modify,
            Privilege::CreateFunction,
            Privilege::Execute,
            Privilege::CreateVolume,
            Privilege::ReadVolume,
            Privilege::CreateModel,
            Privilege::CreateExternalLocation,
            Privilege::ReadFiles,
            Privilege::WriteFiles,
            Privilege::CreateExternalTable,
            Privilege::CreateExternalVolume,
            Privilege::CreateManagedStorage,
            Privilege::CreateStorageCredential,
        ];
        for p in &all {
            let s = p.as_casbin_str();
            let back = Privilege::from_casbin_str(s)
                .unwrap_or_else(|| panic!("from_casbin_str failed for {:?} -> '{}'", p, s));
            assert_eq!(*p, back);
        }
    }

    #[test]
    fn hierarchy_edges_link_owner_all_and_specifics() {
        let edges = Privilege::hierarchy_edges();
        // OWNER -> ALL_PRIVILEGES
        assert!(edges.contains(&("OWNER", "ALL_PRIVILEGES")));
        // ALL_PRIVILEGES -> each specific (spot-check a few)
        assert!(edges.contains(&("ALL_PRIVILEGES", "SELECT")));
        assert!(edges.contains(&("ALL_PRIVILEGES", "MODIFY")));
        assert!(edges.contains(&("ALL_PRIVILEGES", "READ_VOLUME")));
        // one edge for OWNER->ALL plus one per specific privilege
        assert_eq!(edges.len(), 1 + Privilege::specific().len());
        // umbrella privileges are not themselves "specific"
        assert!(!Privilege::specific().contains(&Privilege::Owner));
        assert!(!Privilege::specific().contains(&Privilege::AllPrivileges));
    }

    #[test]
    fn from_casbin_str_unknown_returns_none() {
        assert!(Privilege::from_casbin_str("UNKNOWN_PRIV").is_none());
        assert!(Privilege::from_casbin_str("").is_none());
    }

    #[test]
    fn securable_type_from_str() {
        assert!(matches!(
            SecurableType::parse("CATALOG"),
            Some(SecurableType::Catalog)
        ));
        assert!(matches!(
            SecurableType::parse("catalog"),
            Some(SecurableType::Catalog)
        ));
        assert!(matches!(
            SecurableType::parse("SCHEMA"),
            Some(SecurableType::Schema)
        ));
        assert!(matches!(
            SecurableType::parse("TABLE"),
            Some(SecurableType::Table)
        ));
        assert!(matches!(
            SecurableType::parse("VOLUME"),
            Some(SecurableType::Volume)
        ));
        assert!(matches!(
            SecurableType::parse("FUNCTION"),
            Some(SecurableType::Function)
        ));
        assert!(matches!(
            SecurableType::parse("MODEL"),
            Some(SecurableType::RegisteredModel)
        ));
        assert!(matches!(
            SecurableType::parse("REGISTERED_MODEL"),
            Some(SecurableType::RegisteredModel)
        ));
        assert!(matches!(
            SecurableType::parse("METASTORE"),
            Some(SecurableType::Metastore)
        ));
        assert!(matches!(
            SecurableType::parse("EXTERNAL_LOCATION"),
            Some(SecurableType::ExternalLocation)
        ));
        assert!(matches!(
            SecurableType::parse("STORAGE_CREDENTIAL"),
            Some(SecurableType::StorageCredential)
        ));
        assert!(SecurableType::parse("UNKNOWN").is_none());
    }

    #[test]
    fn token_type_variants_exist() {
        // Verify both variants exist and are distinct
        let access = TokenType::Access;
        let service = TokenType::Service;
        assert!(access != service);
        // serde round-trip is tested in uc-openapi/uc-auth which depend on serde
    }
}
