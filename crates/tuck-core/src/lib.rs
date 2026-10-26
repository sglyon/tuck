//! Portable meeting rules and workflows. Concrete platform adapters are wired by the host.
pub mod domain;
pub mod dictation_cleanup;
pub mod library;
pub mod meeting_day;
pub mod ports;
pub mod service;
pub mod store;
pub use domain::AppError;
pub use service::{Adapters, MeetingService};
