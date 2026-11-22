//! Domain layer — pure models, expansion logic, and errors.

pub mod citation;
pub mod error;
pub mod expand;
pub mod models;

pub use error::{ApiError, DomainError, DomainResult};
pub(crate) use expand::preview_text;
pub use expand::{case_adapt, evaluate_macro};

/// Canonical UTC timestamp (RFC 3339) for record creation/updates.
pub(crate) fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339()
}
