mod config;
mod git;
mod logger;
mod plugin;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use code_split_core::Snapshot;
use std::collections::HashMap;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum GraphKind {
    Modules,
    Files,
    Functions,
}

#[derive(Parser, Debug)]
#[command(
    name = "code-split",
    version,
    about = "Pluggable multi-language structural analysis platform"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Analyze a workspace and produce a snapshot JSON file.
    Analyze {
        /// Path to the workspace (directory containing the project).
        workspace: PathBuf,

        /// Plugin to use for analysis (e.g. `rust`, `python`, or a PATH binary).
        /// Falls back to `plugin` in code-split.toml, then "rust".
        #[arg(long)]
        plugin: Option<String>,

        /// Output snapshot file. Defaults to `.code-split/snap-<timestamp>.json`.
        #[arg(long, short)]
        output: Option<PathBuf>,

        /// Analyze only local code — skip network-dependent steps.
        #[arg(long)]
        local_only: bool,

        /// Which graphs to build. Repeat or comma-separate: modules,files,functions.
        #[arg(long = "graph", value_enum, num_args = 1.., value_delimiter = ',',
              default_values_t = [GraphKind::Modules, GraphKind::Files, GraphKind::Functions])]
        graphs: Vec<GraphKind>,

        /// Path to config file. Auto-discovered if not set (code-split.toml, Cargo.toml metadata).
        #[arg(long, value_name = "FILE")]
        config: Option<PathBuf>,

        /// Ignore paths matching these globs (repeatable). Merged with config file.
        #[arg(long = "ignore", value_name = "GLOB")]
        ignore_paths: Vec<String>,

        /// Override cycle rule: allow|warn|deny (e.g. `--cycle-rule test-embed=allow`).
        #[arg(long = "cycle-rule", value_name = "KIND=SEVERITY")]
        cycle_rules: Vec<String>,

        /// Per-node metric threshold (e.g. `--threshold node.hk=500000`).
        #[arg(long = "threshold", value_name = "SCOPE.METRIC=N")]
        thresholds: Vec<String>,

        /// Exit 0 even when `deny` violations are found (collect-only mode).
        #[arg(long)]
        exit_zero: bool,

        /// Extra arguments forwarded to the plugin after `--`.
        #[arg(last = true)]
        extra: Vec<String>,
    },

    /// Generate an HTML report from a snapshot file.
    Report {
        /// Input snapshot JSON file.
        #[arg(long)]
        input: PathBuf,

        /// Output HTML file. Defaults to stdout.
        #[arg(long, short, default_value = "-")]
        output: String,
    },

    /// Generate an HTML diff report between two snapshots.
    Diff {
        /// Snapshot taken before the change.
        #[arg(long)]
        before: PathBuf,

        /// Snapshot taken after the change.
        #[arg(long)]
        after: PathBuf,

        /// Output HTML file. Defaults to stdout.
        #[arg(long, short, default_value = "-")]
        output: String,
    },

    /// Compare two snapshots and output a diff summary as JSON.
    Compare {
        /// Snapshot taken before the change.
        #[arg(long)]
        before: PathBuf,

        /// Snapshot taken after the change.
        #[arg(long)]
        after: PathBuf,

        /// Output file. Use '-' for stdout.
        #[arg(long, short, default_value = "-")]
        output: String,

        /// Generate a self-contained interactive HTML report instead of JSON.
        #[arg(long)]
        html: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let t = logger::Timer::start(&format!(
        "code-split {}",
        std::env::args().skip(1).collect::<Vec<_>>().join(" ")
    ));
    let res = match cli.command {
        Command::Analyze {
            workspace,
            plugin,
            output,
            local_only,
            graphs,
            config,
            ignore_paths,
            cycle_rules,
            thresholds,
            exit_zero,
            extra,
        } => run_analyze(
            &workspace,
            plugin.as_deref(),
            output.as_deref(),
            local_only,
            &graphs,
            config.as_deref(),
            &ignore_paths,
            &cycle_rules,
            &thresholds,
            exit_zero,
            &extra,
        ),
        Command::Report { input, output } => run_report(&input, &output),
        Command::Diff {
            before,
            after,
            output,
        } => run_diff(&before, &after, &output),
        Command::Compare {
            before,
            after,
            output,
            html,
        } => run_compare(&before, &after, &output, html),
    };
    match &res {
        Ok(_) => {
            t.finish();
        }
        Err(e) => logger::info(&format!("error: {e:#}")),
    }
    res
}

#[allow(clippy::too_many_arguments)]
fn run_analyze(
    target_arg: &Path,
    plugin_arg: Option<&str>,
    output: Option<&Path>,
    local_only: bool,
    requested: &[GraphKind],
    config_path: Option<&Path>,
    ignore_paths: &[String],
    cycle_rules: &[String],
    thresholds: &[String],
    exit_zero: bool,
    extra: &[String],
) -> Result<()> {
    let target = target_arg
        .canonicalize()
        .with_context(|| format!("workspace not found: {}", target_arg.display()))?;
    let cwd = std::env::current_dir()?;

    // Load config early so we can resolve the plugin name from it.
    let loaded = config::load(&target, config_path, ignore_paths, cycle_rules, thresholds)
        .unwrap_or_else(|e| {
            logger::info(&format!("config warning: {e}"));
            config::LoadedConfig {
                config: Default::default(),
                source_file: None,
            }
        });
    let cfg = loaded.config;
    if let Some(f) = &loaded.source_file {
        logger::info(&format!("config: {f}"));
    }

    // Priority: --plugin CLI > config.plugin > "rust"
    let plugin_name: &str = plugin_arg.or(cfg.plugin.as_deref()).unwrap_or("rust");

    let want_modules = requested.contains(&GraphKind::Modules);
    let want_files = requested.contains(&GraphKind::Files);
    let want_functions = requested.contains(&GraphKind::Functions);

    logger::info(&format!("target:    {}", target.display()));
    logger::info(&format!("workspace: {}", cwd.display()));
    logger::info(&format!(
        "plugin: {plugin_name}{}",
        if local_only { " (local-only)" } else { "" }
    ));
    logger::info(&format!(
        "graphs: {}",
        requested
            .iter()
            .map(|g| format!("{g:?}").to_lowercase())
            .collect::<Vec<_>>()
            .join(", ")
    ));

    let command = build_command_string(plugin_name, &target, local_only, requested, extra);

    let (mut plugin_graphs, mut timings) =
        plugin::run(plugin_name, &target, local_only, want_functions, extra)
            .with_context(|| format!("plugin '{}' failed", plugin_name))?;

    if !want_modules {
        plugin_graphs.modules = Default::default();
    }
    if !want_files {
        plugin_graphs.files = Default::default();
    }
    if !want_functions {
        plugin_graphs.functions = Default::default();
    }

    let mut roots = detect_roots();
    roots.insert("target".to_string(), target.display().to_string());
    code_split_core::relativize_graphs(&mut plugin_graphs, &target, &roots);
    code_split_core::rewrite_ids(&mut plugin_graphs, &target, &roots);

    let ignored = config::apply_ignore(&mut plugin_graphs, &cfg.ignore, &target)?;
    if ignored > 0 {
        logger::info(&format!("config: {ignored} nodes filtered by ignore.paths"));
    }

    code_split_core::annotate_all_cycles(&mut plugin_graphs);
    config::apply_cycle_rules(&mut plugin_graphs, &cfg.rules.cycles);
    code_split_core::annotate_hk(&mut plugin_graphs);
    code_split_core::annotate_stats(&mut plugin_graphs.modules);
    code_split_core::annotate_stats(&mut plugin_graphs.files);
    code_split_core::annotate_stats(&mut plugin_graphs.functions);

    let violations = config::check_violations(&plugin_graphs, &cfg.rules);
    for v in &violations {
        logger::info(&format!(
            "violation [{}] {}: {}",
            v.graph,
            if v.is_error() { "error" } else { "warn" },
            v.message
        ));
    }
    let errors = violations.iter().filter(|v| v.is_error()).count();
    if errors > 0 && !exit_zero {
        anyhow::bail!("{errors} deny violation(s) found — see above");
    }

    let git = git::collect(&target);
    if let Some(g) = &git {
        logger::info(&format!(
            "git: {} @ {} ({} dirty)",
            g.branch, g.commit, g.dirty_files
        ));
    }

    let mut versions = HashMap::new();
    versions.insert("code-split".to_string(), env!("CARGO_PKG_VERSION").to_string());
    if plugin_name == "rust" {
        versions.insert(
            "code_split_plugin_rust".to_string(),
            env!("CARGO_PKG_VERSION").to_string(),
        );
        if let Some(rv) = plugin::rust::version_string() {
            versions.insert("rustc".to_string(), rv);
        }
    }

    let out_path = match output {
        Some(p) => p.to_owned(),
        None => default_snapshot_path(&target)?,
    };

    let t_write = logger::Timer::start("writing snapshot");
    let json = serde_json::to_string_pretty(&Snapshot::new(
        command,
        cwd.display().to_string(),
        target.display().to_string(),
        plugin_name.to_string(),
        loaded.source_file,
        local_only,
        versions,
        roots,
        git,
        timings.clone(),
        plugin_graphs,
    ))?;

    if out_path == Path::new("-") {
        io::stdout().lock().write_all(json.as_bytes())?;
        writeln!(io::stdout().lock())?;
        t_write.finish();
    } else {
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating directory {}", parent.display()))?;
        }
        std::fs::write(&out_path, &json)
            .with_context(|| format!("writing snapshot to {}", out_path.display()))?;
        let ms = t_write.finish_with(&format!("{}", out_path.display()));
        timings.push(code_split_core::StageTime {
            stage: "write".into(),
            ms,
            detail: out_path.display().to_string(),
        });
    }

    Ok(())
}

fn detect_roots() -> HashMap<String, String> {
    let mut roots = HashMap::new();
    let home = std::env::var("HOME").unwrap_or_default();

    let cargo = std::env::var("CARGO_HOME").unwrap_or_else(|_| format!("{home}/.cargo"));
    let rustup = std::env::var("RUSTUP_HOME").unwrap_or_else(|_| format!("{home}/.rustup"));

    if !cargo.is_empty() {
        // Auto-detect crates.io registry hash dir (e.g. index.crates.io-<hash>).
        let registry_src = format!("{cargo}/registry/src");
        if let Ok(entries) = std::fs::read_dir(&registry_src) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with("index.crates.io") {
                    roots.insert("registry".to_string(), format!("{registry_src}/{name}"));
                    break;
                }
            }
        }
        roots.insert("cargo".to_string(), cargo);
    }
    if !rustup.is_empty() {
        // Add rust-src root: sysroot/lib/rustlib/src/rust/library
        // This shortens stdlib paths from {rustup}/toolchains/.../library/... to {rust-src}/...
        if let Ok(out) = std::process::Command::new("rustc")
            .args(["--print", "sysroot"])
            .output()
            && out.status.success()
        {
            let sysroot = String::from_utf8_lossy(&out.stdout).trim().to_string();
            let rust_lib = format!("{sysroot}/lib/rustlib/src/rust/library");
            if std::path::Path::new(&rust_lib).exists() {
                roots.insert("rust-src".to_string(), rust_lib);
            }
        }
        roots.insert("rustup".to_string(), rustup);
    }
    roots
}

fn default_snapshot_path(workspace: &Path) -> Result<PathBuf> {
    let ts = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    let slug = workspace
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("snapshot");
    let dir = std::env::current_dir()?.join(".code-split");
    Ok(dir.join(format!("{slug}-{ts}.json")))
}

fn build_command_string(
    plugin: &str,
    workspace: &Path,
    local_only: bool,
    graphs: &[GraphKind],
    extra: &[String],
) -> String {
    let mut parts = vec![
        "code-split".to_string(),
        "analyze".to_string(),
        workspace.display().to_string(),
        "--plugin".to_string(),
        plugin.to_string(),
    ];
    if local_only {
        parts.push("--local-only".to_string());
    }
    let all = [GraphKind::Modules, GraphKind::Files, GraphKind::Functions];
    if graphs != all {
        let kinds = graphs
            .iter()
            .map(|g| format!("{g:?}").to_lowercase())
            .collect::<Vec<_>>()
            .join(",");
        parts.push(format!("--graph {kinds}"));
    }
    if !extra.is_empty() {
        parts.push("--".to_string());
        parts.extend_from_slice(extra);
    }
    parts.join(" ")
}

fn run_report(input: &Path, output: &str) -> Result<()> {
    let bytes =
        std::fs::read(input).with_context(|| format!("reading snapshot {}", input.display()))?;
    let snapshot: Snapshot = serde_json::from_slice(&bytes)
        .with_context(|| format!("parsing snapshot {}", input.display()))?;

    let html = render_html_report(&snapshot);
    write_output(output, html.as_bytes())
}

fn run_diff(before: &Path, after: &Path, output: &str) -> Result<()> {
    let snap_before = load_snapshot(before)?;
    let snap_after = load_snapshot(after)?;

    let html = render_html_diff(&snap_before, &snap_after);
    write_output(output, html.as_bytes())
}

fn load_snapshot(path: &Path) -> Result<Snapshot> {
    let bytes =
        std::fs::read(path).with_context(|| format!("reading snapshot {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("parsing snapshot {}", path.display()))
}

fn write_output(output: &str, data: &[u8]) -> Result<()> {
    if output == "-" {
        io::stdout().lock().write_all(data)?;
    } else {
        let mut f = BufWriter::new(
            std::fs::File::create(output)
                .with_context(|| format!("creating output file {output}"))?,
        );
        f.write_all(data)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Minimal built-in HTML renderers (P1: functional, not polished)
// ---------------------------------------------------------------------------

fn render_html_report(snap: &Snapshot) -> String {
    let mod_nodes = snap.graphs.modules.nodes.len();
    let file_nodes = snap.graphs.files.nodes.len();
    let fn_nodes = snap.graphs.functions.nodes.len();
    let mod_edges = snap.graphs.modules.edges.len();
    let file_edges = snap.graphs.files.edges.len();
    let fn_edges = snap.graphs.functions.edges.len();

    let git_html = match &snap.git {
        Some(g) => format!(
            "<p>Git: <code>{}</code> @ <code>{}</code> ({} dirty file(s))</p>",
            escape_html(&g.branch),
            escape_html(&g.commit),
            g.dirty_files,
        ),
        None => String::new(),
    };

    let versions_html: String = snap
        .versions
        .iter()
        .map(|(k, v)| {
            format!(
                "<li><code>{}</code>: {}</li>",
                escape_html(k),
                escape_html(v)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let fn_rows: String = snap
        .graphs
        .functions
        .nodes
        .iter()
        .filter(|n| {
            matches!(
                n.kind,
                code_split_core::NodeKind::Fn | code_split_core::NodeKind::Method
            )
        })
        .map(|n| {
            let callers = snap
                .graphs
                .functions
                .edges
                .iter()
                .filter(|e| e.to == n.id && e.kind == code_split_core::EdgeKind::Calls)
                .count();
            let callees = snap
                .graphs
                .functions
                .edges
                .iter()
                .filter(|e| e.from == n.id && e.kind == code_split_core::EdgeKind::Calls)
                .count();
            format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                escape_html(&n.path),
                escape_html(&n.name),
                callers,
                callees,
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>code-split report — {workspace}</title>
<style>
body {{ font-family: system-ui, sans-serif; margin: 2rem; }}
table {{ border-collapse: collapse; width: 100%; }}
th, td {{ border: 1px solid #ccc; padding: 0.4rem 0.8rem; text-align: left; }}
th {{ background: #f0f0f0; cursor: pointer; }}
tr:hover {{ background: #fafafa; }}
.summary {{ display: flex; gap: 2rem; margin-bottom: 1rem; }}
.stat {{ background: #f5f5f5; padding: 0.8rem 1.2rem; border-radius: 6px; }}
.stat .num {{ font-size: 2rem; font-weight: bold; }}
</style>
</head>
<body>
<h1>code-split report</h1>
<p><strong>Workspace:</strong> {workspace}</p>
<p><strong>Plugin:</strong> {plugin}{local_only}</p>
<p><strong>Generated:</strong> {generated_at}</p>
{git_html}
<h2>Versions</h2>
<ul>{versions_html}</ul>
<h2>Summary</h2>
<div class="summary">
  <div class="stat"><div class="num">{mod_nodes}</div>modules nodes</div>
  <div class="stat"><div class="num">{mod_edges}</div>modules edges</div>
  <div class="stat"><div class="num">{file_nodes}</div>file nodes</div>
  <div class="stat"><div class="num">{file_edges}</div>file edges</div>
  <div class="stat"><div class="num">{fn_nodes}</div>function nodes</div>
  <div class="stat"><div class="num">{fn_edges}</div>function edges</div>
</div>
<h2>Functions / Methods</h2>
<table id="fn-table">
<thead><tr><th onclick="sort(0)">Path</th><th onclick="sort(1)">Name</th><th onclick="sort(2)">Callers ▼</th><th onclick="sort(3)">Callees</th></tr></thead>
<tbody>
{fn_rows}
</tbody>
</table>
<script>
function sort(col) {{
  const tb = document.querySelector('#fn-table tbody');
  const rows = Array.from(tb.rows);
  const asc = tb.dataset.sortCol == col && tb.dataset.sortDir === 'asc';
  rows.sort((a, b) => {{
    const av = a.cells[col].textContent.trim();
    const bv = b.cells[col].textContent.trim();
    const an = parseFloat(av), bn = parseFloat(bv);
    const cmp = isNaN(an) ? av.localeCompare(bv) : an - bn;
    return asc ? -cmp : cmp;
  }});
  rows.forEach(r => tb.appendChild(r));
  tb.dataset.sortCol = col;
  tb.dataset.sortDir = asc ? 'desc' : 'asc';
}}
</script>
</body>
</html>"#,
        workspace = escape_html(&snap.workspace),
        plugin = escape_html(&snap.plugin),
        local_only = if snap.local_only { " (local-only)" } else { "" },
        generated_at = snap.generated_at,
        git_html = git_html,
        versions_html = versions_html,
        mod_nodes = mod_nodes,
        mod_edges = mod_edges,
        file_nodes = file_nodes,
        file_edges = file_edges,
        fn_nodes = fn_nodes,
        fn_edges = fn_edges,
        fn_rows = fn_rows,
    )
}

fn render_html_diff(before: &Snapshot, after: &Snapshot) -> String {
    let added_mods = count_new_nodes(&before.graphs.modules, &after.graphs.modules);
    let removed_mods = count_new_nodes(&after.graphs.modules, &before.graphs.modules);
    let added_fns = count_new_nodes(&before.graphs.functions, &after.graphs.functions);
    let removed_fns = count_new_nodes(&after.graphs.functions, &before.graphs.functions);
    let added_calls = count_new_edges(
        &before.graphs.functions,
        &after.graphs.functions,
        code_split_core::EdgeKind::Calls,
    );
    let removed_calls = count_new_edges(
        &after.graphs.functions,
        &before.graphs.functions,
        code_split_core::EdgeKind::Calls,
    );

    let new_fn_rows: String = after
        .graphs
        .functions
        .nodes
        .iter()
        .filter(|n| {
            matches!(
                n.kind,
                code_split_core::NodeKind::Fn | code_split_core::NodeKind::Method
            )
        })
        .filter(|n| !before.graphs.functions.nodes.iter().any(|b| b.id == n.id))
        .map(|n| {
            format!(
                "<tr class='added'><td>+</td><td>{}</td><td>{}</td></tr>",
                escape_html(&n.path),
                escape_html(&n.name)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let removed_fn_rows: String = before
        .graphs
        .functions
        .nodes
        .iter()
        .filter(|n| {
            matches!(
                n.kind,
                code_split_core::NodeKind::Fn | code_split_core::NodeKind::Method
            )
        })
        .filter(|n| !after.graphs.functions.nodes.iter().any(|a| a.id == n.id))
        .map(|n| {
            format!(
                "<tr class='removed'><td>−</td><td>{}</td><td>{}</td></tr>",
                escape_html(&n.path),
                escape_html(&n.name)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>code-split diff</title>
<style>
body {{ font-family: system-ui, sans-serif; margin: 2rem; }}
table {{ border-collapse: collapse; width: 100%; }}
th, td {{ border: 1px solid #ccc; padding: 0.4rem 0.8rem; }}
.added {{ background: #e6ffed; }}
.removed {{ background: #ffeef0; }}
.summary {{ display: flex; gap: 2rem; margin-bottom: 1rem; flex-wrap: wrap; }}
.stat {{ padding: 0.8rem 1.2rem; border-radius: 6px; }}
.stat.pos {{ background: #e6ffed; }}
.stat.neg {{ background: #ffeef0; }}
.stat .num {{ font-size: 2rem; font-weight: bold; }}
</style>
</head>
<body>
<h1>code-split diff</h1>
<table>
<tr><th></th><th>Before</th><th>After</th></tr>
<tr><td>Workspace</td><td>{ws_b}</td><td>{ws_a}</td></tr>
<tr><td>Commit</td><td>{commit_b}</td><td>{commit_a}</td></tr>
<tr><td>Branch</td><td>{branch_b}</td><td>{branch_a}</td></tr>
</table>
<h2>Summary</h2>
<div class="summary">
  <div class="stat pos"><div class="num">+{added_mods}</div>modules added</div>
  <div class="stat neg"><div class="num">−{removed_mods}</div>modules removed</div>
  <div class="stat pos"><div class="num">+{added_fns}</div>functions added</div>
  <div class="stat neg"><div class="num">−{removed_fns}</div>functions removed</div>
  <div class="stat pos"><div class="num">+{added_calls}</div>calls added</div>
  <div class="stat neg"><div class="num">−{removed_calls}</div>calls removed</div>
</div>
<h2>Function changes</h2>
<table>
<thead><tr><th></th><th>Path</th><th>Name</th></tr></thead>
<tbody>
{new_fn_rows}
{removed_fn_rows}
</tbody>
</table>
</body>
</html>"#,
        ws_b = escape_html(&before.workspace),
        ws_a = escape_html(&after.workspace),
        commit_b = before
            .git
            .as_ref()
            .map(|g| g.commit.as_str())
            .unwrap_or("—"),
        commit_a = after.git.as_ref().map(|g| g.commit.as_str()).unwrap_or("—"),
        branch_b = before
            .git
            .as_ref()
            .map(|g| g.branch.as_str())
            .unwrap_or("—"),
        branch_a = after.git.as_ref().map(|g| g.branch.as_str()).unwrap_or("—"),
        added_mods = added_mods,
        removed_mods = removed_mods,
        added_fns = added_fns,
        removed_fns = removed_fns,
        added_calls = added_calls,
        removed_calls = removed_calls,
        new_fn_rows = new_fn_rows,
        removed_fn_rows = removed_fn_rows,
    )
}

fn count_new_nodes(base: &code_split_core::Graph, other: &code_split_core::Graph) -> usize {
    other
        .nodes
        .iter()
        .filter(|n| !base.nodes.iter().any(|b| b.id == n.id))
        .count()
}

fn count_new_edges(
    base: &code_split_core::Graph,
    other: &code_split_core::Graph,
    kind: code_split_core::EdgeKind,
) -> usize {
    other
        .edges
        .iter()
        .filter(|e| {
            e.kind == kind
                && !base
                    .edges
                    .iter()
                    .any(|b| b.from == e.from && b.to == e.to && b.kind == e.kind)
        })
        .count()
}

// ── Assets embedded at compile time ──────────────────────────────────────────
const ASSET_CSS: &str = include_str!("assets/index.css");
const ASSET_GV: &str = include_str!("assets/graphviz.umd.js");
const ASSET_DIFF: &str = include_str!("assets/diff.js");
const ASSET_LAYOUT: &str = include_str!("assets/layout.js");
const ASSET_UTILS: &str = include_str!("assets/utils.js");
const ASSET_MODAL: &str = include_str!("assets/modal.js");
const ASSET_PANZOOM: &str = include_str!("assets/panzoom.js");
const ASSET_DIAGRAM: &str = include_str!("assets/diagram.js");
const ASSET_UI: &str = include_str!("assets/ui.js");
const ASSET_SUMMARY: &str = include_str!("assets/summary.js");
const ASSET_EXPORT_POPUP: &str = include_str!("assets/export-popup.js");
const ASSET_NODE_TABLE: &str = include_str!("assets/node-table.js");
const ASSET_NAV: &str = include_str!("assets/nav.js");
const ASSET_APP: &str = include_str!("assets/app.js");
const ASSET_HTML: &str = include_str!("assets/index.html");

fn run_compare(before: &Path, after: &Path, output: &str, html: bool) -> Result<()> {
    let snap_before = load_snapshot(before)?;
    let snap_after = load_snapshot(after)?;

    let data = if html {
        render_compare_html(&snap_before, &snap_after).into_bytes()
    } else {
        let summary = code_split_core::compare_snapshots(&snap_before, &snap_after);
        let mut json = serde_json::to_string_pretty(&summary)?;
        json.push('\n');
        json.into_bytes()
    };

    write_output(output, &data)
}

fn render_compare_html(before: &Snapshot, after: &Snapshot) -> String {
    let before_json = serde_json::to_string(before).expect("serialize before");
    let after_json = serde_json::to_string(after).expect("serialize after");

    let data_script = format!(
        "<script>const BEFORE = {};\nconst AFTER = {};\n</script>",
        before_json, after_json
    );

    ASSET_HTML
        .replace(
            r#"<link rel="stylesheet" href="./index.css">"#,
            &format!("<style>{}</style>", ASSET_CSS),
        )
        .replace(
            r#"<script src="./graphviz.umd.js"></script>"#,
            &format!("<script>{}</script>", ASSET_GV),
        )
        .replace(r#"<script src="./data.js"></script>"#, &data_script)
        .replace(
            r#"<script src="./diff.js"></script>"#,
            &format!("<script>{}</script>", ASSET_DIFF),
        )
        .replace(
            r#"<script src="./layout.js"></script>"#,
            &format!("<script>{}</script>", ASSET_LAYOUT),
        )
        .replace(
            r#"<script src="./utils.js"></script>"#,
            &format!("<script>{}</script>", ASSET_UTILS),
        )
        .replace(
            r#"<script src="./modal.js"></script>"#,
            &format!("<script>{}</script>", ASSET_MODAL),
        )
        .replace(
            r#"<script src="./panzoom.js"></script>"#,
            &format!("<script>{}</script>", ASSET_PANZOOM),
        )
        .replace(
            r#"<script src="./diagram.js"></script>"#,
            &format!("<script>{}</script>", ASSET_DIAGRAM),
        )
        .replace(
            r#"<script src="./ui.js"></script>"#,
            &format!("<script>{}</script>", ASSET_UI),
        )
        .replace(
            r#"<script src="./summary.js"></script>"#,
            &format!("<script>{}</script>", ASSET_SUMMARY),
        )
        .replace(
            r#"<script src="./export-popup.js"></script>"#,
            &format!("<script>{}</script>", ASSET_EXPORT_POPUP),
        )
        .replace(
            r#"<script src="./node-table.js"></script>"#,
            &format!("<script>{}</script>", ASSET_NODE_TABLE),
        )
        .replace(
            r#"<script src="./nav.js"></script>"#,
            &format!("<script>{}</script>", ASSET_NAV),
        )
        .replace(
            r#"<script src="./app.js"></script>"#,
            &format!("<script>{}</script>", ASSET_APP),
        )
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
