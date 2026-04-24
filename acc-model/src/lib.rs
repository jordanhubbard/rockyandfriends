//! Shared ACC domain types used by both `acc-server` and `acc-client`.
//!
//! Types are organized by resource. Each module mirrors the corresponding
//! `/api/<resource>` route group on the server. Keep definitions minimal
//! and wire-focused — server-internal representations belong in `acc-server`.

pub mod error;
pub mod task;

pub use error::ApiError;
pub use task::{
    ClaimRequest, CompleteRequest, CreateTaskRequest, ReviewResult, ReviewResultRequest, Task,
    TaskStatus, TaskType, UnclaimRequest,
};
