#![forbid(unsafe_code)]
//! Static plugin registry: what a fresh `apc` can do with no configuration.
//!
//! Dynamic loading arrives in v1.2 (SPEC.md §8.7); until then the CLI asks here instead of
//! wiring tools itself.

use std::path::PathBuf;
use std::sync::Arc;

use apc_tools::{ListDir, ReadFile, RunShell, WriteFile};
use apc_traits::{Approver, DecisionProvider, Tool};

/// The built-in tool set, rooted at `root`.
pub fn default_tools(
    root: PathBuf,
    decision: Arc<dyn DecisionProvider>,
    approver: Arc<dyn Approver>,
    threshold: f64,
) -> Vec<Arc<dyn Tool>> {
    vec![
        Arc::new(ReadFile::new(root.clone())),
        Arc::new(WriteFile::new(root.clone())),
        Arc::new(ListDir::new(root.clone())),
        Arc::new(RunShell::new(root, decision, approver, threshold)),
    ]
}
