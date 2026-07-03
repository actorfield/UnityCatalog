use axum::{extract::Request, middleware::Next, response::Response};
use uc_errors::{error_into_response, ErrorFormat, UcError};

/// Layer injected on each sub-router to tag requests with the error format.
/// This is read by UcError::into_response() to pick the right wire shape.
pub async fn inject_catalog_format(mut req: Request, next: Next) -> Response {
    req.extensions_mut().insert(ErrorFormat::Catalog);
    next.run(req).await
}

pub async fn inject_control_format(mut req: Request, next: Next) -> Response {
    req.extensions_mut().insert(ErrorFormat::Control);
    next.run(req).await
}

pub async fn inject_delta_format(mut req: Request, next: Next) -> Response {
    req.extensions_mut().insert(ErrorFormat::Delta);
    next.run(req).await
}

/// Helper: convert a UcError using the format extension stored on the response.
/// Used by handlers that need to return errors after the response is built.
pub fn uc_error_response(err: UcError, format: ErrorFormat) -> Response {
    error_into_response(err, format)
}
