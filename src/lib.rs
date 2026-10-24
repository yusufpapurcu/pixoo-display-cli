//! Divoom Bluetooth wire codecs, checked commands, native media and exclusive sessions.
//!
//! Device compatibility and verification evidence are listed in `docs/coverage.md`.
#![doc = include_str!("../README.md")]
pub mod commands;
mod error;
#[cfg(feature = "media")]
pub mod media;
pub mod profile;
pub mod protocol;
pub mod session;
pub mod transport;
pub use error::{Error, Result};
