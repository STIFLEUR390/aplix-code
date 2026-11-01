#![forbid(unsafe_code)]
//! Shared types, errors and configuration for Aplix Code.

pub mod chat;
pub mod config;
pub mod error;
pub mod tool;

pub use error::{Error, Result};
