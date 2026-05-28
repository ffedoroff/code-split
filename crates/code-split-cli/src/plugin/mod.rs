pub mod javascript;
pub mod python;
pub mod rust;

use anyhow::{Context, Result, bail};
use code_split_core::{PluginGraphs, StageTime};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Resolve and run a plugin for the given workspace.
/// `want_functions` controls whether the sema (call-graph) stage runs.
/// Returns `(graphs, timings)`.
pub fn run(
    name: &str,
    workspace: &Path,
    local_only: bool,
    want_functions: bool,
    extra_args: &[String],
) -> Result<(PluginGraphs, Vec<StageTime>)> {
    if name == "rust" {
        return rust::run(workspace, local_only, want_functions);
    }
    if name == "python" {
        return python::run(workspace, local_only, want_functions);
    }
    if name == "javascript" || name == "typescript" || name == "js" || name == "ts" {
        return javascript::run(workspace, local_only, want_functions);
    }
    Ok((
        run_external(name, workspace, local_only, extra_args)?,
        vec![],
    ))
}

/// Discover the external plugin binary on PATH: `code-split-plugin-<name>`.
fn find_external(name: &str) -> Option<PathBuf> {
    let bin = format!("code-split-plugin-{name}");
    which::which(&bin).ok()
}

fn run_external(
    name: &str,
    workspace: &Path,
    local_only: bool,
    extra_args: &[String],
) -> Result<PluginGraphs> {
    let bin = find_external(name).with_context(|| {
        format!("plugin '{name}' not found: expected built-in or `code-split-plugin-{name}` on PATH")
    })?;

    let tmp = tempfile::NamedTempFile::new()?;
    let tmp_path = tmp.path().to_owned();
    // Keep the file alive until we read it.
    drop(tmp);

    let mut cmd = Command::new(&bin);
    cmd.arg(workspace).arg("--output").arg(&tmp_path);
    if local_only {
        cmd.arg("--local-only");
    }
    if !extra_args.is_empty() {
        cmd.arg("--");
        cmd.args(extra_args);
    }

    let status = cmd
        .status()
        .with_context(|| format!("failed to launch plugin {}", bin.display()))?;
    if !status.success() {
        bail!("plugin '{}' exited with {}", name, status);
    }

    let bytes = std::fs::read(&tmp_path)
        .with_context(|| format!("reading plugin output from {}", tmp_path.display()))?;

    #[derive(serde::Deserialize)]
    struct PluginOutput {
        graphs: PluginGraphs,
    }
    let out: PluginOutput = serde_json::from_slice(&bytes).context("parsing plugin output JSON")?;
    Ok(out.graphs)
}
