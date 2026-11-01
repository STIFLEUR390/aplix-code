#![forbid(unsafe_code)]
//! Local decision engine.
//!
//! The heuristic backend ships before any model (SPEC.md §2.3, §8.1): it must make the Permission
//! Guard work on a fresh clone, with nothing downloaded, and stay as the fallback once `open-jev`
//! exists. Recall is chosen over precision — a false alarm costs one confirmation, a miss costs a
//! wiped disk.

mod cascade;
#[cfg(feature = "open-jev")]
mod jev;
pub mod store;

pub use cascade::Cascade;
#[cfg(feature = "open-jev")]
pub use jev::JevProvider;
pub use store::{find, ModelEntry, Status, Store, CATALOG};

use apc_core::Result;
use apc_traits::DecisionProvider;

/// `(pattern, destructive probability)`, matched case-insensitively as a substring.
///
/// Ordered by severity only for readability; the score is the maximum of the matches.
const PATTERNS: &[(&str, f64)] = &[
    ("rm -rf /", 0.99),
    (":(){ :|:& };:", 0.99),
    ("mkfs", 0.98),
    ("dd of=/dev/", 0.97),
    ("rm -rf", 0.95),
    ("rm -fr", 0.95),
    (" > /dev/sd", 0.95),
    ("drop table", 0.93),
    ("drop database", 0.93),
    ("truncate table", 0.9),
    ("terraform destroy", 0.9),
    ("shutdown", 0.9),
    ("sudo rm", 0.9),
    ("curl | sh", 0.85),
    ("wget | sh", 0.85),
    ("kubectl delete", 0.85),
    ("reboot", 0.85),
    ("git push --force", 0.82),
    ("git reset --hard", 0.78),
    ("chmod -r 777", 0.75),
    ("docker rm -f", 0.72),
    ("find . -delete", 0.7),
];

/// The question the Permission Guard asks about every shell command.
pub const GUARD_QUESTION: &str = "is this command destructive?";

/// Destructive probability of `command`, in `0.0..=1.0`.
pub fn score(command: &str) -> f64 {
    let command = command.to_lowercase();
    PATTERNS
        .iter()
        .filter(|(pattern, _)| command.contains(pattern))
        .map(|(_, probability)| *probability)
        .fold(0.0, f64::max)
}

/// Pattern blacklist. No model, no files, no state.
#[derive(Debug, Default, Clone, Copy)]
pub struct HeuristicProvider;

#[async_trait::async_trait]
impl DecisionProvider for HeuristicProvider {
    async fn noul(&self, state: &str, _question: &str) -> Result<f64> {
        Ok(score(state))
    }

    async fn choice(
        &self,
        _state: &str,
        _question: &str,
        options: &[&str],
    ) -> Result<(String, f64)> {
        let pick = options
            .iter()
            .map(|option| (*option, score(option)))
            .max_by(|left, right| left.1.total_cmp(&right.1));
        match pick {
            Some((option, probability)) => Ok((option.to_owned(), probability)),
            None => Ok((String::new(), 0.0)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn destructive_commands_score_above_the_default_threshold() {
        for command in [
            "rm -rf /",
            "rm -rf ~/projects",
            "sudo mkfs.ext4 /dev/sda1",
            "dd of=/dev/sda if=image.iso",
            "psql -c 'DROP TABLE users;'",
            "git push --force origin main",
            ":(){ :|:& };:",
        ] {
            assert!(
                score(command) > 0.8,
                "`{command}` should need confirmation, got {}",
                score(command)
            );
        }
    }

    #[test]
    fn ordinary_commands_do_not_raise_the_alarm() {
        for command in [
            "ls -la",
            "cargo test --workspace",
            "git log --graph --oneline",
            "npm run build",
            "grep -rn TODO src",
            "mkdir -p build && cp -r assets build/",
            "docker ps",
            "rm build/artifact.o",
        ] {
            assert_eq!(score(command), 0.0, "`{command}` should run unprompted");
        }
    }

    #[test]
    fn matching_is_case_insensitive() {
        assert_eq!(score("DROP TABLE users"), score("drop table users"));
        assert_eq!(score("RM -RF /"), score("rm -rf /"));
    }

    #[tokio::test]
    async fn noul_reports_the_score() {
        let guard = HeuristicProvider;
        let risky = guard.noul("rm -rf /", GUARD_QUESTION).await.expect("score");
        assert!(risky > 0.8);
        let safe = guard.noul("ls -la", GUARD_QUESTION).await.expect("score");
        assert_eq!(safe, 0.0);
    }

    #[tokio::test]
    async fn choice_picks_the_riskiest_option() {
        let guard = HeuristicProvider;
        let (pick, probability) = guard
            .choice("plan", "what next?", &["ls -la", "rm -rf /"])
            .await
            .expect("choice");
        assert_eq!(pick, "rm -rf /");
        assert!(probability > 0.8);
    }

    struct Fixed(f64);
    struct Broken;

    #[async_trait::async_trait]
    impl DecisionProvider for Fixed {
        async fn noul(&self, _state: &str, _question: &str) -> Result<f64> {
            Ok(self.0)
        }
        async fn choice(
            &self,
            _state: &str,
            _question: &str,
            options: &[&str],
        ) -> Result<(String, f64)> {
            Ok((options[0].to_owned(), self.0))
        }
    }

    #[async_trait::async_trait]
    impl DecisionProvider for Broken {
        async fn noul(&self, _state: &str, _question: &str) -> Result<f64> {
            Err(apc_core::Error::Decision("server is down".to_owned()))
        }
        async fn choice(
            &self,
            _state: &str,
            _question: &str,
            _options: &[&str],
        ) -> Result<(String, f64)> {
            Err(apc_core::Error::Decision("server is down".to_owned()))
        }
    }

    #[tokio::test]
    async fn a_working_backend_answers_first() {
        let cascade = Cascade::new(
            std::sync::Arc::new(Fixed(0.97)),
            std::sync::Arc::new(HeuristicProvider),
        );
        assert_eq!(cascade.noul("ls -la", GUARD_QUESTION).await.unwrap(), 0.97);
    }

    #[tokio::test]
    async fn a_broken_backend_falls_back_to_the_heuristics() {
        let cascade = Cascade::new(
            std::sync::Arc::new(Broken),
            std::sync::Arc::new(HeuristicProvider),
        );
        assert!(cascade.noul("rm -rf /", GUARD_QUESTION).await.unwrap() > 0.8);
        assert_eq!(cascade.noul("ls -la", GUARD_QUESTION).await.unwrap(), 0.0);
    }
}
