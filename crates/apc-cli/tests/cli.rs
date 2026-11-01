//! End-to-end checks for `apc init` and `apc config` (no extra dev-dependencies).

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn sandbox(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("apc-cli-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create sandbox");
    dir
}

/// Run `apc` in `dir` with a throwaway home, so the user layer never leaks in.
fn apc(dir: &Path, args: &[&str]) -> Output {
    apc_env(dir, args, &[])
}

fn apc_env(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_apc"));
    command
        .args(args)
        .current_dir(dir)
        .env("HOME", dir)
        .env("XDG_CONFIG_HOME", dir.join("xdg"))
        .env_remove("APC_PROVIDER")
        .env_remove("APC_MODEL")
        .env_remove("APC_PERMISSION_THRESHOLD")
        .env_remove("APC_TELEMETRY");
    for (key, value) in env {
        command.env(key, value);
    }
    command.output().expect("run apc")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// The table row whose first column is `key`.
fn row<'a>(text: &'a str, key: &str) -> &'a str {
    text.lines()
        .find(|line| line.split_whitespace().next() == Some(key))
        .unwrap_or_else(|| panic!("no `{key}` row in:\n{text}"))
}

#[test]
fn init_writes_a_config_and_refuses_to_overwrite() {
    let dir = sandbox("init");

    let first = apc(&dir, &["init"]);
    assert!(first.status.success(), "{}", stderr(&first));
    let text = std::fs::read_to_string(dir.join("apc.toml")).expect("apc.toml written");
    assert!(text.contains("[provider]"), "{text}");

    let second = apc(&dir, &["init"]);
    assert!(!second.status.success());
    assert!(
        stderr(&second).contains("already exists"),
        "{}",
        stderr(&second)
    );

    let forced = apc(&dir, &["init", "--force"]);
    assert!(forced.status.success(), "{}", stderr(&forced));
}

#[test]
fn config_reports_values_and_their_layer() {
    let dir = sandbox("layers");
    assert!(apc(&dir, &["init"]).status.success());

    let output = apc(&dir, &["config"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);

    assert!(text.contains("config files"), "{text}");
    assert!(row(&text, "provider.name").ends_with("project"));
    assert!(row(&text, "telemetry").ends_with("project"));
}

#[test]
fn local_file_env_and_flags_win_in_that_order() {
    let dir = sandbox("override");
    assert!(apc(&dir, &["init"]).status.success());
    std::fs::write(
        dir.join("apc.local.toml"),
        "[provider]\nmodel = \"local-model\"\n[decision]\nthreshold = 0.5\n",
    )
    .expect("write local file");

    let local = stdout(&apc(&dir, &["config"]));
    assert!(row(&local, "provider.model").contains("local-model"));
    assert!(row(&local, "decision.threshold").contains("0.5"));

    let env = stdout(&apc_env(&dir, &["config"], &[("APC_PROVIDER", "ollama")]));
    assert!(row(&env, "provider.name").contains("ollama"));
    assert!(row(&env, "provider.name").ends_with("env"));

    let flags = stdout(&apc(&dir, &["config", "--permission-threshold", "0.1"]));
    assert!(row(&flags, "decision.threshold").contains("0.1"));
    assert!(row(&flags, "decision.threshold").ends_with("flag"));
}

#[test]
fn env_telemetry_accepts_zero_and_one() {
    let dir = sandbox("telemetry");
    assert!(apc(&dir, &["init"]).status.success());

    let on = stdout(&apc_env(&dir, &["config"], &[("APC_TELEMETRY", "1")]));
    assert!(row(&on, "telemetry").contains("true"));

    let off = stdout(&apc_env(&dir, &["config"], &[("APC_TELEMETRY", "0")]));
    assert!(row(&off, "telemetry").contains("false"));
}

#[test]
fn version_reports_the_decision_model() {
    let dir = sandbox("version");
    let output = apc(&dir, &["--version"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains("apc 0.1.0"), "{text}");
    assert!(text.contains("decision model"), "{text}");
}
