//! Native outbound adapters. Domain rules and use cases live in tuck-core.
pub mod archive;
pub mod audio;
pub mod backup;
pub mod calendar;
pub mod capture;
pub mod chatgpt;
pub mod dictation_target;
pub mod import_audio;
pub mod intelligence;
pub mod models;
pub mod notifications;
pub mod notion;
pub mod notion_auth;
pub mod playback;
pub mod refinement;
pub mod secrets;
pub mod speech;
pub mod voice_memory;
pub mod live_speakers;

#[cfg(target_os = "macos")]
pub mod microphone_permission;
