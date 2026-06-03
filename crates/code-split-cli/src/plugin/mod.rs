use anyhow::{Result, bail};
use code_split_graph::{PluginGraphs, StageTime};
use std::path::Path;

/// Run a built-in language plugin for the given workspace. Returns
/// `(graphs, timings)`.
///
/// Each language is its own crate (`code-split-plugin-{rust,python,javascript}`);
/// they are compiled into the binary and dispatched here by name. There is no
/// external/dynamic plugin loading.
pub fn run(name: &str, workspace: &Path) -> Result<(PluginGraphs, Vec<StageTime>)> {
    match name {
        "rust" => code_split_plugin_rust::run(workspace),
        "python" => code_split_plugin_python::run(workspace),
        "javascript" | "typescript" | "js" | "ts" => code_split_plugin_javascript::run(workspace),
        other => bail!("unknown plugin {other:?}; built-in plugins are: rust, python, javascript"),
    }
}
