#![forbid(unsafe_code)]
//! Static plugin registry: what a fresh `apc` can do with no configuration.
//!
//! Dynamic loading arrives later (SPEC.md §8.7); until then the CLI asks here instead of
//! wiring tools itself.

use std::path::PathBuf;
use std::sync::Arc;

use apc_core::config::{Config, Mode};
use apc_tools::{FetchPage, ListDir, ReadFile, RunShell, WebSearch, WriteFile};
use apc_traits::{Approver, DecisionProvider, Tool};

/// The built-in tool set, rooted at `root`.
///
/// A tool whose service has no URL is left out on purpose: the model must not be offered a
/// tool that cannot answer (v1.2). `Mode::Plan` drops the two tools that touch the project, so
/// planning cannot write a file or run a command by mistake.
pub fn default_tools(
    root: PathBuf,
    decision: Arc<dyn DecisionProvider>,
    approver: Arc<dyn Approver>,
    config: &Config,
    mode: Mode,
) -> Vec<Arc<dyn Tool>> {
    let max_bytes = config.tools.max_output_bytes;
    let mut tools: Vec<Arc<dyn Tool>> = vec![
        Arc::new(ReadFile::new(root.clone(), max_bytes)),
        Arc::new(ListDir::new(root.clone(), max_bytes)),
        Arc::new(FetchPage::new(config.tools.fetch_max_bytes)),
    ];
    if mode == Mode::Build {
        tools.push(Arc::new(WriteFile::new(root.clone())));
        tools.push(Arc::new(RunShell::new(
            root,
            decision,
            approver,
            config.decision.threshold,
            max_bytes,
        )));
    }
    if let Some(url) = &config.tools.searxng_url {
        tools.push(Arc::new(WebSearch::new(url)));
    }
    tools
}

#[cfg(test)]
mod tests {
    use super::*;
    use apc_core::Result;

    use apc_decision::HeuristicProvider;

    struct Calm;

    impl Approver for Calm {
        fn confirm(&self, _command: &str, _risk: f64) -> Result<bool> {
            Ok(false)
        }
    }

    fn names(config: &Config) -> Vec<String> {
        names_in(config, Mode::Build)
    }

    fn names_in(config: &Config, mode: Mode) -> Vec<String> {
        let tools = default_tools(
            PathBuf::from("."),
            Arc::new(HeuristicProvider),
            Arc::new(Calm),
            config,
            mode,
        );
        tools.iter().map(|tool| tool.name().to_owned()).collect()
    }

    #[test]
    fn an_unconfigured_search_tool_is_not_offered() {
        let config = Config::default();
        assert!(!names(&config).iter().any(|name| name == "web_search"));
    }

    #[test]
    fn plan_mode_offers_neither_writes_nor_a_shell() {
        let config = Config::default();
        let plan = names_in(&config, Mode::Plan);
        assert!(!plan.iter().any(|name| name == "write_file"));
        assert!(!plan.iter().any(|name| name == "run_shell"));
        assert!(plan.iter().any(|name| name == "read_file"));

        let build = names(&config);
        assert!(build.iter().any(|name| name == "write_file"));
        assert!(build.iter().any(|name| name == "run_shell"));
    }

    #[test]
    fn a_configured_search_tool_is_offered() {
        let mut config = Config::default();
        config.tools.searxng_url = Some("http://127.0.0.1:8888".to_owned());
        assert!(names(&config).iter().any(|name| name == "web_search"));
    }
}
