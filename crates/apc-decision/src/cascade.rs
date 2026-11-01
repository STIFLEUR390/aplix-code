//! Fail-open pairing: the local model answers when it can, the heuristics otherwise.
//!
//! The CLI must never be blocked by a missing or wedged model server (SPEC.md §2.3), so a failed
//! call is not an error the user sees — it just changes who answered.

use std::sync::Arc;

use apc_core::Result;
use apc_traits::DecisionProvider;

/// Try `primary`, fall back to `fallback` on any error.
pub struct Cascade {
    primary: Arc<dyn DecisionProvider>,
    fallback: Arc<dyn DecisionProvider>,
}

impl Cascade {
    /// `primary` is the model backend, `fallback` the heuristic one.
    pub fn new(primary: Arc<dyn DecisionProvider>, fallback: Arc<dyn DecisionProvider>) -> Self {
        Self { primary, fallback }
    }
}

#[async_trait::async_trait]
impl DecisionProvider for Cascade {
    async fn noul(&self, state: &str, question: &str) -> Result<f64> {
        match self.primary.noul(state, question).await {
            Ok(risk) => Ok(risk),
            Err(_) => self.fallback.noul(state, question).await,
        }
    }

    async fn choice(&self, state: &str, question: &str, options: &[&str]) -> Result<(String, f64)> {
        match self.primary.choice(state, question, options).await {
            Ok(choice) => Ok(choice),
            Err(_) => self.fallback.choice(state, question, options).await,
        }
    }
}
