#![forbid(unsafe_code)]
//! Tool declaration: what a provider is told about a tool.

use serde::{Deserialize, Serialize};

/// Name, description and JSON-schema parameters of one tool.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolSpec {
    /// Stable name the model calls.
    pub name: String,
    /// One-line purpose, shown to the model.
    pub description: String,
    /// JSON schema of the arguments object.
    pub parameters: serde_json::Value,
}
