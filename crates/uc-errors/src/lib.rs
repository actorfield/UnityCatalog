use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;

// ── Error codes ──────────────────────────────────────────────────────────────

/// Maps 1:1 to Java's ErrorCode enum.
/// Tuple fields: (uc_http_status, delta_http_status, delta_error_type_str)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    InvalidArgument,
    UnsupportedTableFormat,
    NotFound,
    CatalogNotFound,
    SchemaNotFound,
    TableNotFound,
    AlreadyExists,
    PermissionDenied,
    Unauthenticated,
    ResourceExhausted,
    FailedPrecondition,
    Aborted,
    CommitVersionConflict,
    UpdateRequirementConflict,
    OutOfRange,
    Unimplemented,
    Internal,
    DataLoss,
    ResourceAlreadyExists,
    CatalogAlreadyExists,
    SchemaAlreadyExists,
    TableAlreadyExists,
    StorageCredentialAlreadyExists,
    ExternalLocationAlreadyExists,
}

impl ErrorCode {
    /// HTTP status for the UC catalog/control API.
    pub fn uc_status(&self) -> StatusCode {
        match self {
            Self::InvalidArgument => StatusCode::BAD_REQUEST,
            Self::UnsupportedTableFormat => StatusCode::BAD_REQUEST,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::CatalogNotFound => StatusCode::NOT_FOUND,
            Self::SchemaNotFound => StatusCode::NOT_FOUND,
            Self::TableNotFound => StatusCode::NOT_FOUND,
            Self::AlreadyExists => StatusCode::CONFLICT,
            Self::PermissionDenied => StatusCode::FORBIDDEN,
            Self::Unauthenticated => StatusCode::UNAUTHORIZED,
            Self::ResourceExhausted => StatusCode::TOO_MANY_REQUESTS,
            Self::FailedPrecondition => StatusCode::BAD_REQUEST,
            Self::Aborted => StatusCode::CONFLICT,
            Self::CommitVersionConflict => StatusCode::CONFLICT,
            Self::UpdateRequirementConflict => StatusCode::CONFLICT,
            Self::OutOfRange => StatusCode::BAD_REQUEST,
            Self::Unimplemented => StatusCode::NOT_IMPLEMENTED,
            Self::Internal => StatusCode::INTERNAL_SERVER_ERROR,
            Self::DataLoss => StatusCode::INTERNAL_SERVER_ERROR,
            // Legacy UC "already exists" codes return 400 for backwards compat
            Self::ResourceAlreadyExists => StatusCode::BAD_REQUEST,
            Self::CatalogAlreadyExists => StatusCode::BAD_REQUEST,
            Self::SchemaAlreadyExists => StatusCode::BAD_REQUEST,
            Self::TableAlreadyExists => StatusCode::BAD_REQUEST,
            Self::StorageCredentialAlreadyExists => StatusCode::BAD_REQUEST,
            Self::ExternalLocationAlreadyExists => StatusCode::BAD_REQUEST,
        }
    }

    /// HTTP status for the Delta API (spec-correct; differs for AlreadyExists variants).
    pub fn delta_status(&self) -> StatusCode {
        match self {
            Self::ResourceAlreadyExists
            | Self::CatalogAlreadyExists
            | Self::SchemaAlreadyExists
            | Self::TableAlreadyExists
            | Self::StorageCredentialAlreadyExists
            | Self::ExternalLocationAlreadyExists => StatusCode::CONFLICT,
            other => other.uc_status(),
        }
    }

    /// Delta error type string as used in the Delta API error response body.
    pub fn delta_error_type(&self) -> &'static str {
        match self {
            Self::InvalidArgument => "InvalidParameterValueException",
            Self::UnsupportedTableFormat => "UnsupportedTableFormatException",
            Self::NotFound => "NotFoundException",
            Self::CatalogNotFound => "NoSuchCatalogException",
            Self::SchemaNotFound => "NoSuchSchemaException",
            Self::TableNotFound => "NoSuchTableException",
            Self::AlreadyExists
            | Self::ResourceAlreadyExists
            | Self::CatalogAlreadyExists
            | Self::SchemaAlreadyExists
            | Self::TableAlreadyExists
            | Self::StorageCredentialAlreadyExists
            | Self::ExternalLocationAlreadyExists => "AlreadyExistsException",
            Self::PermissionDenied => "PermissionDeniedException",
            Self::Unauthenticated => "NotAuthorizedException",
            Self::ResourceExhausted => "ResourceExhaustedException",
            Self::FailedPrecondition => "InvalidParameterValueException",
            Self::Aborted | Self::CommitVersionConflict => "CommitVersionConflictException",
            Self::UpdateRequirementConflict => "UpdateRequirementConflictException",
            Self::OutOfRange => "BadRequestException",
            Self::Unimplemented => "NotImplementedException",
            Self::Internal | Self::DataLoss => "InternalServerErrorException",
        }
    }

    /// String representation used in UC catalog API error_code field.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::InvalidArgument => "INVALID_ARGUMENT",
            Self::UnsupportedTableFormat => "UNSUPPORTED_TABLE_FORMAT",
            Self::NotFound => "NOT_FOUND",
            Self::CatalogNotFound => "CATALOG_NOT_FOUND",
            Self::SchemaNotFound => "SCHEMA_NOT_FOUND",
            Self::TableNotFound => "TABLE_NOT_FOUND",
            Self::AlreadyExists => "ALREADY_EXISTS",
            Self::PermissionDenied => "PERMISSION_DENIED",
            Self::Unauthenticated => "UNAUTHENTICATED",
            Self::ResourceExhausted => "RESOURCE_EXHAUSTED",
            Self::FailedPrecondition => "FAILED_PRECONDITION",
            Self::Aborted => "ABORTED",
            Self::CommitVersionConflict => "COMMIT_VERSION_CONFLICT",
            Self::UpdateRequirementConflict => "UPDATE_REQUIREMENT_CONFLICT",
            Self::OutOfRange => "OUT_OF_RANGE",
            Self::Unimplemented => "UNIMPLEMENTED",
            Self::Internal => "INTERNAL",
            Self::DataLoss => "DATA_LOSS",
            Self::ResourceAlreadyExists => "RESOURCE_ALREADY_EXISTS",
            Self::CatalogAlreadyExists => "CATALOG_ALREADY_EXISTS",
            Self::SchemaAlreadyExists => "SCHEMA_ALREADY_EXISTS",
            Self::TableAlreadyExists => "TABLE_ALREADY_EXISTS",
            Self::StorageCredentialAlreadyExists => "STORAGE_CREDENTIAL_ALREADY_EXISTS",
            Self::ExternalLocationAlreadyExists => "EXTERNAL_LOCATION_ALREADY_EXISTS",
        }
    }
}

// ── UcError ───────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct UcError {
    pub code: ErrorCode,
    pub message: String,
}

impl UcError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self { code, message: message.into() }
    }

    pub fn not_found(entity: &str, name: &str) -> Self {
        Self::new(ErrorCode::NotFound, format!("{} '{}' not found", entity, name))
    }

    pub fn already_exists(entity: &str, name: &str) -> Self {
        Self::new(ErrorCode::AlreadyExists, format!("{} '{}' already exists", entity, name))
    }

    pub fn invalid_argument(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidArgument, msg)
    }

    pub fn internal(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::Internal, msg)
    }

    pub fn permission_denied(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::PermissionDenied, msg)
    }

    pub fn unauthenticated(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::Unauthenticated, msg)
    }
}

impl std::fmt::Display for UcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code.as_str(), self.message)
    }
}

impl std::error::Error for UcError {}

// ── Wire response shapes ──────────────────────────────────────────────────────

/// UC catalog/control API error body.
#[derive(Serialize)]
pub struct ErrorResponse {
    pub error_code: String,
    pub message: String,
}

/// Delta API error body.
#[derive(Serialize)]
pub struct DeltaErrorResponse {
    pub error: DeltaErrorModel,
}

#[derive(Serialize)]
pub struct DeltaErrorModel {
    pub message: String,
    #[serde(rename = "errorType")]
    pub error_type: String,
}

// ── Which error format to use (injected per-router as an axum Extension) ─────

#[derive(Clone, Copy, Debug)]
pub enum ErrorFormat {
    Catalog,
    Control,
    Delta,
}

// ── axum IntoResponse ─────────────────────────────────────────────────────────

impl IntoResponse for UcError {
    fn into_response(self) -> Response {
        // Default to Catalog format if no extension is present.
        // Handlers in delta_api inject ErrorFormat::Delta via a layer.
        let format = ErrorFormat::Catalog;
        error_into_response(self, format)
    }
}

pub fn error_into_response(err: UcError, format: ErrorFormat) -> Response {
    match format {
        ErrorFormat::Delta => {
            let status = err.code.delta_status();
            let body = DeltaErrorResponse {
                error: DeltaErrorModel {
                    message: err.message,
                    error_type: err.code.delta_error_type().to_string(),
                },
            };
            (status, Json(body)).into_response()
        }
        ErrorFormat::Catalog | ErrorFormat::Control => {
            let status = err.code.uc_status();
            let body = ErrorResponse {
                error_code: err.code.as_str().to_string(),
                message: err.message,
            };
            (status, Json(body)).into_response()
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uc_status_legacy_already_exists_is_400() {
        assert_eq!(ErrorCode::CatalogAlreadyExists.uc_status(), StatusCode::BAD_REQUEST);
        assert_eq!(ErrorCode::SchemaAlreadyExists.uc_status(), StatusCode::BAD_REQUEST);
        assert_eq!(ErrorCode::TableAlreadyExists.uc_status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn delta_status_legacy_already_exists_is_409() {
        assert_eq!(ErrorCode::CatalogAlreadyExists.delta_status(), StatusCode::CONFLICT);
        assert_eq!(ErrorCode::SchemaAlreadyExists.delta_status(), StatusCode::CONFLICT);
    }

    #[test]
    fn standard_already_exists_is_409_in_both() {
        assert_eq!(ErrorCode::AlreadyExists.uc_status(), StatusCode::CONFLICT);
        assert_eq!(ErrorCode::AlreadyExists.delta_status(), StatusCode::CONFLICT);
    }

    #[test]
    fn error_display() {
        let e = UcError::not_found("Catalog", "my_catalog");
        assert!(e.to_string().contains("NOT_FOUND"));
        assert!(e.to_string().contains("my_catalog"));
    }

    #[test]
    fn uc_error_constructors() {
        let e = UcError::new(ErrorCode::Internal, "oops");
        assert_eq!(e.code, ErrorCode::Internal);
        assert_eq!(e.message, "oops");

        let e = UcError::already_exists("Schema", "s1");
        assert_eq!(e.code, ErrorCode::AlreadyExists);
        assert!(e.message.contains("s1"));

        let e = UcError::invalid_argument("bad input");
        assert_eq!(e.code, ErrorCode::InvalidArgument);

        let e = UcError::internal("crash");
        assert_eq!(e.code, ErrorCode::Internal);

        let e = UcError::permission_denied("no access");
        assert_eq!(e.code, ErrorCode::PermissionDenied);

        let e = UcError::unauthenticated("no token");
        assert_eq!(e.code, ErrorCode::Unauthenticated);
    }

    #[test]
    fn all_error_codes_have_uc_status() {
        let codes = [
            ErrorCode::InvalidArgument, ErrorCode::UnsupportedTableFormat,
            ErrorCode::NotFound, ErrorCode::CatalogNotFound, ErrorCode::SchemaNotFound,
            ErrorCode::TableNotFound, ErrorCode::AlreadyExists, ErrorCode::PermissionDenied,
            ErrorCode::Unauthenticated, ErrorCode::ResourceExhausted, ErrorCode::FailedPrecondition,
            ErrorCode::Aborted, ErrorCode::CommitVersionConflict, ErrorCode::UpdateRequirementConflict,
            ErrorCode::OutOfRange, ErrorCode::Unimplemented, ErrorCode::Internal, ErrorCode::DataLoss,
            ErrorCode::ResourceAlreadyExists, ErrorCode::CatalogAlreadyExists,
            ErrorCode::SchemaAlreadyExists, ErrorCode::TableAlreadyExists,
            ErrorCode::StorageCredentialAlreadyExists, ErrorCode::ExternalLocationAlreadyExists,
        ];
        for code in &codes {
            let status = code.uc_status();
            assert!(status.as_u16() >= 400, "{:?} should be 4xx/5xx", code);
        }
    }

    #[test]
    fn all_error_codes_have_as_str() {
        let codes = [ErrorCode::InvalidArgument, ErrorCode::NotFound, ErrorCode::Internal];
        for code in &codes {
            assert!(!code.as_str().is_empty());
        }
    }

    #[test]
    fn all_error_codes_have_delta_error_type() {
        let codes = [ErrorCode::CommitVersionConflict, ErrorCode::UpdateRequirementConflict,
                     ErrorCode::Unimplemented, ErrorCode::DataLoss];
        for code in &codes {
            assert!(!code.delta_error_type().is_empty());
        }
    }

    #[test]
    fn error_format_variants_exist() {
        let _ = ErrorFormat::Catalog;
        let _ = ErrorFormat::Control;
        let _ = ErrorFormat::Delta;
    }
}
