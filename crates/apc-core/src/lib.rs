#![forbid(unsafe_code)]
//! Shared types, errors and configuration for Aplix Code.

pub mod config;
pub mod error;

pub use error::{Error, Result};
