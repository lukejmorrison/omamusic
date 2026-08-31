//! Oma Music — YouTube Music playback library for Omarchy.
//!
//! Crate name: `omamusic` (Cargo convention: lowercase, one word).
//! The binary is also `omamusic`. On Linux a `cdylib` would produce
//! `libomamusic.so`; this crate ships as an `rlib` plus binary.

pub mod auth;
pub mod catalog;
pub mod cli;
pub mod error;
pub mod innertube;
pub mod oauth;
pub mod json_util;
pub mod paths;
pub mod play_history;
pub mod player;
pub mod protocol;
pub mod queue_session;
pub mod server;
pub mod spectrum;

pub use error::{Error, Result};
pub use paths::{AppPaths, APP_ID};
pub use protocol::{BACKEND_VERSION, PROTOCOL_VERSION};
