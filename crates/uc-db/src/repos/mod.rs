//! Repo layer. Each module has two bodies with identical signatures: the SQL
//! one (`catalog.rs`) and the log-structured one (`catalog_store.rs`). The
//! `logstore` feature selects between them under the same module path, so
//! callers say `repos::catalog::create` either way.

macro_rules! repo {
    ($name:ident, $store:literal) => {
        #[cfg(not(feature = "logstore"))]
        pub mod $name;
        #[cfg(feature = "logstore")]
        #[path = $store]
        pub mod $name;
    };
}

repo!(catalog, "catalog_store.rs");
repo!(schema, "schema_store.rs");
repo!(delta, "delta_store.rs");
repo!(metastore, "metastore_store.rs");
repo!(property, "property_store.rs");
repo!(table, "table_store.rs");
repo!(volume, "volume_store.rs");
repo!(function, "function_store.rs");
repo!(model, "model_store.rs");
repo!(user, "user_store.rs");
repo!(credential, "credential_store.rs");
repo!(external_location, "external_location_store.rs");
repo!(staging, "staging_store.rs");

