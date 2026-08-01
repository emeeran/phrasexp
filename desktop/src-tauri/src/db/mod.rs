//! Storage layer — rusqlite repository mirroring the FastAPI reference.

pub mod repo;

pub use repo::{expand, Db};
