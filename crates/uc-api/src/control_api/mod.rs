pub mod auth;
pub mod scim2;

use axum::{middleware, routing::{delete, get, patch, post, put}, Router};
use crate::{error_ext::inject_control_format, state::AppState};

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/1.0/unity-control/auth/tokens", post(auth::token_exchange))
        .route("/api/1.0/unity-control/auth/logout", post(auth::logout))
        .route("/.well-known/jwks.json", get(auth::jwks))
        .route("/api/1.0/unity-control/scim2/Users", post(scim2::create_user).get(scim2::list_users))
        .route("/api/1.0/unity-control/scim2/Users/:id", get(scim2::get_user).put(scim2::update_user).delete(scim2::delete_user).patch(scim2::patch_user))
        .route("/api/1.0/unity-control/scim2/Me", get(scim2::get_me).patch(scim2::patch_me))
        .layer(middleware::from_fn(inject_control_format))
        .with_state(state)
}
