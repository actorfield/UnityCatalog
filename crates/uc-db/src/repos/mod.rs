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

// Not yet ported — SQL only, so the logstore build does not yet link them.
#[cfg(not(feature = "logstore"))]
pub mod credential;
#[cfg(not(feature = "logstore"))]
pub mod delta;
#[cfg(not(feature = "logstore"))]
pub mod external_location;
#[cfg(not(feature = "logstore"))]
pub mod function;
#[cfg(not(feature = "logstore"))]
pub mod metastore;
#[cfg(not(feature = "logstore"))]
pub mod model;
#[cfg(not(feature = "logstore"))]
pub mod property;
#[cfg(not(feature = "logstore"))]
pub mod staging;
#[cfg(not(feature = "logstore"))]
pub mod table;
#[cfg(not(feature = "logstore"))]
pub mod user;
#[cfg(not(feature = "logstore"))]
pub mod volume;
