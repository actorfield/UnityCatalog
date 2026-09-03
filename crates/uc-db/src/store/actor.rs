//! Who is making the change.
//!
//! Carried as a task-local rather than threaded through every repo signature.
//! The alternative was an extra argument on ~40 repo functions and every call
//! site in uc-api, for a value that is request-scoped and diagnostic — the same
//! shape as a tracing span, and handled the same way.
//!
//! The tradeoff is that it is implicit: a commit made outside a request scope
//! records no actor. That is the honest answer for startup work (metastore
//! init, seeding) which no user performed, so the default is correct rather
//! than merely convenient.

use serde::{Deserialize, Serialize};
use std::future::Future;
use uuid::Uuid;

/// The identity behind a commit, captured at the time of the change.
///
/// Both halves on purpose. `id` is stable and joinable — addresses change, and
/// an audit entry keyed on a mutable string can attribute an action to the
/// wrong person after a rename. `name` is what was true when it happened, so
/// the record stays legible without a join and does not silently re-attribute
/// to whatever address that id now maps to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Actor {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Uuid>,
    pub name: String,
}

impl Actor {
    pub fn new(id: Option<Uuid>, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
        }
    }
}

tokio::task_local! {
    static CURRENT: Option<Actor>;
}

/// Run `f` with `actor` recorded on every commit it makes.
pub async fn scope<F: Future>(actor: Option<Actor>, f: F) -> F::Output {
    CURRENT.scope(actor, f).await
}

/// The actor for the current task, or None outside a scope.
pub fn current() -> Option<Actor> {
    CURRENT.try_with(Clone::clone).ok().flatten()
}
