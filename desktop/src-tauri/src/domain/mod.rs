//! Domain layer — pure models, expansion logic, and errors.
//! Ported from the FastAPI reference (`backend/app.py`) so the data model is identical.

pub mod error;
pub mod expand;
pub mod models;

pub use error::{ApiError, DomainError, DomainResult};
pub use expand::{expand_phrase, literal_preview};
