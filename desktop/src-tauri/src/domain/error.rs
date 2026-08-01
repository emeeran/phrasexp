//! Domain errors — mirror the FastAPI error catalog (SPEC.md §8).
//!
//! `DomainError` is used internally with `?` (thiserror). It converts into `ApiError`
//! at the Tauri command boundary; `ApiError` serializes to `{ "code", "message" }`,
//! which is what the frontend inspects (Tauri IPC has no HTTP status — the `code` carries it).

use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum DomainError {
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Conflict(String),
    #[error("{0}")]
    FolderNotEmpty(String),
    #[error("{0}")]
    MissingInput(String),
    #[error("{0}")]
    Cycle(String),
    #[error("{0}")]
    ReservedContentType(String),
    #[error("{0}")]
    Validation(String),
}

impl DomainError {
    pub fn code(&self) -> &'static str {
        match self {
            DomainError::NotFound(_) => "not_found",
            DomainError::Conflict(_) => "conflict",
            DomainError::FolderNotEmpty(_) => "folder_not_empty",
            DomainError::MissingInput(_) => "missing_input",
            DomainError::Cycle(_) => "cycle",
            DomainError::ReservedContentType(_) => "reserved_content_type",
            DomainError::Validation(_) => "validation",
        }
    }
}

/// Serializable error returned to the frontend via Tauri commands.
#[derive(Debug, Serialize)]
pub struct ApiError {
    pub code: String,
    pub message: String,
}

impl From<DomainError> for ApiError {
    fn from(e: DomainError) -> Self {
        ApiError {
            code: e.code().to_string(),
            message: e.to_string(),
        }
    }
}

pub type DomainResult<T> = Result<T, DomainError>;
