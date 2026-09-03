//! The handle every repo function takes.
//!
//! Kept as its own module, and still named `AnyPool`, so the call sites in
//! uc-api do not have to change. There is only one backend now.

pub type AnyPool = crate::store::Store;
