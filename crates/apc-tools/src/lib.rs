#![forbid(unsafe_code)]
//! Built-in tools: file access confined to the working directory, and a guarded shell.

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use apc_core::error::Error;
use apc_core::Result;
use apc_decision::GUARD_QUESTION;
use apc_traits::{Approver, DecisionProvider, Tool};
use serde_json::{json, Value};

/// Resolve `path` under `root`, refusing anything that would leave it.
///
/// Purely lexical on purpose: the file may not exist yet, so there is nothing to canonicalise.
fn resolve(root: &Path, path: &str) -> Result<PathBuf> {
    let candidate = Path::new(path);
    if candidate.is_absolute() {
        return Err(outside(path));
    }
    let mut resolved = root.to_path_buf();
    let mut depth = 0i32;
    for component in candidate.components() {
        match component {
            Component::ParentDir => {
                depth -= 1;
                if depth < 0 {
                    return Err(outside(path));
                }
                resolved.pop();
            }
            Component::CurDir => {}
            other => {
                depth += 1;
                resolved.push(other);
            }
        }
    }
    Ok(resolved)
}

fn outside(path: &str) -> Error {
    Error::Tool(format!("`{path}` is outside the working directory"))
}

fn string(args: &Value, key: &str) -> Result<String> {
    args[key]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| Error::Tool(format!("missing `{key}`")))
}

/// Read a text file.
pub struct ReadFile {
    root: PathBuf,
}

impl ReadFile {
    /// Read relative to `root`.
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
}

#[async_trait::async_trait]
impl Tool for ReadFile {
    fn name(&self) -> &str {
        "read_file"
    }

    fn description(&self) -> &str {
        "Read a UTF-8 text file from the working directory."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": { "path": { "type": "string", "description": "Path relative to the working directory." } },
            "required": ["path"],
        })
    }

    async fn execute(&self, args: Value) -> Result<String> {
        let path = resolve(&self.root, &string(&args, "path")?)?;
        std::fs::read_to_string(&path)
            .map_err(|error| Error::Tool(format!("{}: {error}", path.display())))
    }
}

/// Create or overwrite a text file.
pub struct WriteFile {
    root: PathBuf,
}

impl WriteFile {
    /// Write relative to `root`.
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
}

#[async_trait::async_trait]
impl Tool for WriteFile {
    fn name(&self) -> &str {
        "write_file"
    }

    fn description(&self) -> &str {
        "Write a text file in the working directory, creating it or replacing its contents."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Path relative to the working directory." },
                "content": { "type": "string", "description": "Full new contents." },
            },
            "required": ["path", "content"],
        })
    }

    async fn execute(&self, args: Value) -> Result<String> {
        let path = resolve(&self.root, &string(&args, "path")?)?;
        let content = string(&args, "content")?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| Error::Tool(format!("{}: {error}", parent.display())))?;
        }
        std::fs::write(&path, &content)
            .map_err(|error| Error::Tool(format!("{}: {error}", path.display())))?;
        Ok(format!(
            "wrote {} bytes to {}",
            content.len(),
            path.display()
        ))
    }
}

/// List a directory, one entry per line, directories suffixed with `/`.
pub struct ListDir {
    root: PathBuf,
}

impl ListDir {
    /// List relative to `root`.
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
}

#[async_trait::async_trait]
impl Tool for ListDir {
    fn name(&self) -> &str {
        "list_dir"
    }

    fn description(&self) -> &str {
        "List a directory of the working directory (`path` defaults to `.`)."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": { "path": { "type": "string", "description": "Path relative to the working directory." } },
        })
    }

    async fn execute(&self, args: Value) -> Result<String> {
        let path = resolve(&self.root, args["path"].as_str().unwrap_or("."))?;
        let entries = std::fs::read_dir(&path)
            .map_err(|error| Error::Tool(format!("{}: {error}", path.display())))?;
        let mut names = Vec::new();
        for entry in entries {
            let entry =
                entry.map_err(|error| Error::Tool(format!("{}: {error}", path.display())))?;
            let name = entry.file_name().to_string_lossy().into_owned();
            names.push(if entry.path().is_dir() {
                format!("{name}/")
            } else {
                name
            });
        }
        names.sort();
        Ok(names.join("\n"))
    }
}

/// Run a shell command, but only once the decision engine is happy or the human agrees.
///
/// This is the only path to a shell in the whole workspace (SPEC.md §2.3).
pub struct RunShell {
    root: PathBuf,
    decision: Arc<dyn DecisionProvider>,
    approver: Arc<dyn Approver>,
    threshold: f64,
}

impl RunShell {
    /// Guard with `decision`; anything scoring above `threshold` needs `approver`.
    pub fn new(
        root: PathBuf,
        decision: Arc<dyn DecisionProvider>,
        approver: Arc<dyn Approver>,
        threshold: f64,
    ) -> Self {
        Self {
            root,
            decision,
            approver,
            threshold,
        }
    }
}

#[async_trait::async_trait]
impl Tool for RunShell {
    fn name(&self) -> &str {
        "run_shell"
    }

    fn description(&self) -> &str {
        "Run a shell command in the working directory and return its combined output."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": { "command": { "type": "string", "description": "Shell command, run with `sh -c`." } },
            "required": ["command"],
        })
    }

    async fn execute(&self, args: Value) -> Result<String> {
        let command = string(&args, "command")?;
        let risk = self.decision.noul(&command, GUARD_QUESTION).await?;
        if risk > self.threshold && !self.approver.confirm(&command, risk)? {
            return Ok(format!(
                "the user declined to run this command, so it was not executed: {command}"
            ));
        }
        let output = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(&command)
            .current_dir(&self.root)
            .output()
            .await
            .map_err(|error| Error::Tool(format!("{command}: {error}")))?;
        let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&output.stderr));
        if text.trim().is_empty() {
            text = format!("(no output, exit {})", output.status);
        }
        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_relative_path_stays_inside() {
        let root = Path::new("/work");
        assert_eq!(resolve(root, "a/b").unwrap(), Path::new("/work/a/b"));
        assert_eq!(resolve(root, "./a/../b").unwrap(), Path::new("/work/b"));
    }

    struct Yes(f64);

    #[async_trait::async_trait]
    impl DecisionProvider for Yes {
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

    struct Answer(bool);

    impl Approver for Answer {
        fn confirm(&self, _command: &str, _risk: f64) -> Result<bool> {
            Ok(self.0)
        }
    }

    fn shell(dir: &Path, decision: f64, approver: bool) -> RunShell {
        RunShell::new(
            dir.to_path_buf(),
            Arc::new(Yes(decision)),
            Arc::new(Answer(approver)),
            f64::midpoint(0.0, 1.0),
        )
    }

    #[tokio::test]
    async fn a_refused_confirmation_does_not_run_the_command() {
        let dir = std::env::temp_dir().join(format!("apc-guard-{}", std::process::id()));
        let marker = dir.join("ran");
        let command = format!("touch {}", marker.display());
        let answer = shell(&dir, 0.99, false)
            .execute(json!({ "command": command }))
            .await
            .expect("reports the refusal");
        assert!(answer.contains("declined"), "{answer}");
        assert!(!marker.exists(), "the command ran anyway");
    }

    #[tokio::test]
    async fn an_accepted_confirmation_runs_the_command() {
        assert_eq!(
            shell(&std::env::temp_dir(), 0.99, true)
                .execute(json!({ "command": "echo hi" }))
                .await
                .expect("runs"),
            "hi\n"
        );
    }

    #[tokio::test]
    async fn leaving_the_working_directory_is_refused() {
        let root = std::env::temp_dir();
        assert!(resolve(&root, "../secret").is_err());
        assert!(ReadFile::new(root)
            .execute(json!({ "path": "../../etc/passwd" }))
            .await
            .is_err());
    }
}
