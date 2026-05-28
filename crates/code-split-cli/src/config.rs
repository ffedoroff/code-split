use anyhow::{Context, Result};
use code_split_core::graph::{CycleKind, Graph};
use code_split_core::snapshot::PluginGraphs;
use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::Deserialize;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;

// ── Config structs ─────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub struct Config {
    /// Default plugin name (e.g. "rust", "python"). Overridden by --plugin.
    pub plugin: Option<String>,
    pub ignore: IgnoreConfig,
    pub rules: RulesConfig,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub struct IgnoreConfig {
    pub paths: Vec<String>,
    /// Strip all inline `mod tests { … }` submodules (IDs ending with `::tests`).
    pub test_modules: bool,
    /// Strip crates that appear only in [dev-dependencies], never in [dependencies].
    pub dev_only_crates: bool,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub struct RulesConfig {
    pub cycles: CycleRules,
    pub thresholds: ThresholdRules,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct CycleRules {
    #[serde(rename = "test-embed")]
    pub test_embed: Severity,
    pub mutual: Severity,
    pub chain: Severity,
}

impl Default for CycleRules {
    fn default() -> Self {
        Self {
            test_embed: Severity::Allow,
            mutual: Severity::Deny,
            chain: Severity::Deny,
        }
    }
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub struct ThresholdRules {
    /// Per-node: flag any single node whose metric exceeds the limit.
    pub node: MetricThresholds,
    /// Graph-average: flag when the graph-wide average exceeds the limit.
    pub avg: MetricThresholds,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct MetricThresholds {
    pub hk: Option<f64>,
    pub cyclomatic: Option<f64>,
    pub cognitive: Option<f64>,
    pub fan_in: Option<f64>,
    pub fan_out: Option<f64>,
    pub loc: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Allow,
    Warn,
    #[default]
    Deny,
}

impl std::str::FromStr for Severity {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        match s {
            "allow" => Ok(Self::Allow),
            "warn" => Ok(Self::Warn),
            "deny" => Ok(Self::Deny),
            other => anyhow::bail!("unknown severity {:?}; expected allow|warn|deny", other),
        }
    }
}

// ── Loading ────────────────────────────────────────────────────────────────────
//
// Priority (highest wins):
//   1. CLI flags   --ignore / --cycle-rule / --threshold
//   2. --config <file>  (explicit path)
//   3. code-split.toml     (cwd, then workspace root)
//   4. Cargo.toml       [workspace.metadata.code-split] or [package.metadata.code-split]
//   5. Built-in defaults

/// Loaded config together with the file it came from (for snapshot recording).
pub struct LoadedConfig {
    pub config: Config,
    /// Canonical path of the file that was used, if any.
    pub source_file: Option<String>,
}

pub fn load(
    workspace: &Path,
    config_path: Option<&Path>,
    ignore_paths: &[String],
    cycle_rules: &[String],
    thresholds: &[String],
) -> Result<LoadedConfig> {
    let (mut config, source_file) = load_file(workspace, config_path)?;
    apply_cli_overrides(&mut config, ignore_paths, cycle_rules, thresholds)?;
    Ok(LoadedConfig {
        config,
        source_file,
    })
}

fn load_file(workspace: &Path, explicit: Option<&Path>) -> Result<(Config, Option<String>)> {
    // 1. Explicit --config
    if let Some(path) = explicit {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let cfg = toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        return Ok((cfg, Some(path.display().to_string())));
    }

    let cwd = std::env::current_dir().unwrap_or_default();

    // 2. code-split.toml in cwd, then workspace root
    for dir in [cwd.as_path(), workspace] {
        let p = dir.join("code-split.toml");
        if p.exists() {
            let text =
                std::fs::read_to_string(&p).with_context(|| format!("reading {}", p.display()))?;
            let cfg = toml::from_str(&text).with_context(|| format!("parsing {}", p.display()))?;
            let canonical = p.canonicalize().unwrap_or(p);
            return Ok((cfg, Some(canonical.display().to_string())));
        }
    }

    // 3. Cargo.toml [workspace.metadata.code-split] / [package.metadata.code-split]
    for dir in [cwd.as_path(), workspace] {
        if let Some((cfg, src)) = load_from_cargo_toml(dir)? {
            return Ok((cfg, Some(src)));
        }
    }

    Ok((Config::default(), None))
}

fn load_from_cargo_toml(dir: &Path) -> Result<Option<(Config, String)>> {
    let cargo = dir.join("Cargo.toml");
    if !cargo.exists() {
        return Ok(None);
    }
    let text =
        std::fs::read_to_string(&cargo).with_context(|| format!("reading {}", cargo.display()))?;
    let val: toml::Value =
        toml::from_str(&text).with_context(|| format!("parsing {}", cargo.display()))?;

    let section = val
        .get("workspace")
        .and_then(|w| w.get("metadata"))
        .and_then(|m| m.get("code-split"))
        .or_else(|| {
            val.get("package")
                .and_then(|p| p.get("metadata"))
                .and_then(|m| m.get("code-split"))
        });

    if let Some(v) = section {
        let cfg: Config = v
            .clone()
            .try_into()
            .with_context(|| format!("parsing [*.metadata.code-split] in {}", cargo.display()))?;
        let canonical = cargo.canonicalize().unwrap_or(cargo);
        return Ok(Some((
            cfg,
            format!("{}#metadata.code-split", canonical.display()),
        )));
    }
    Ok(None)
}

// ── CLI overrides ──────────────────────────────────────────────────────────────

fn apply_cli_overrides(
    cfg: &mut Config,
    ignore_paths: &[String],
    cycle_rules: &[String],
    thresholds: &[String],
) -> Result<()> {
    cfg.ignore.paths.extend_from_slice(ignore_paths);

    for raw in cycle_rules {
        // Format: "kind=severity", e.g. "test-embed=allow"
        let (kind_str, sev_str) = split_kv(raw, "cycle-rule")?;
        let sev: Severity = sev_str.parse()?;
        match kind_str {
            "test-embed" => cfg.rules.cycles.test_embed = sev,
            "mutual" => cfg.rules.cycles.mutual = sev,
            "chain" => cfg.rules.cycles.chain = sev,
            other => anyhow::bail!(
                "unknown cycle kind {:?}; expected test-embed|mutual|chain",
                other
            ),
        }
    }

    for raw in thresholds {
        // Format: "scope.metric=N", e.g. "node.hk=500000" or "avg.cyclomatic=10"
        let (scope_metric, val_str) = split_kv(raw, "threshold")?;
        let val: f64 = val_str
            .parse()
            .with_context(|| format!("threshold value must be a number: {raw}"))?;
        let (scope, metric) = scope_metric
            .split_once('.')
            .with_context(|| format!("threshold must be scope.metric=N, got: {raw}"))?;
        let bucket = match scope {
            "node" => &mut cfg.rules.thresholds.node,
            "avg" => &mut cfg.rules.thresholds.avg,
            other => anyhow::bail!("unknown threshold scope {:?}; expected node|avg", other),
        };
        match metric {
            "hk" => bucket.hk = Some(val),
            "cyclomatic" => bucket.cyclomatic = Some(val),
            "cognitive" => bucket.cognitive = Some(val),
            "fan_in" => bucket.fan_in = Some(val),
            "fan_out" => bucket.fan_out = Some(val),
            "loc" => bucket.loc = Some(val),
            other => anyhow::bail!(
                "unknown metric {:?}; expected hk|cyclomatic|cognitive|fan_in|fan_out|loc",
                other
            ),
        }
    }

    Ok(())
}

fn split_kv<'a>(s: &'a str, flag: &str) -> Result<(&'a str, &'a str)> {
    s.split_once('=')
        .with_context(|| format!("--{flag} must be key=value, got: {s}"))
}

// ── Path filtering ─────────────────────────────────────────────────────────────

pub fn apply_ignore(
    graphs: &mut PluginGraphs,
    ignore: &IgnoreConfig,
    target: &Path,
) -> Result<usize> {
    let gs = if ignore.paths.is_empty() {
        None
    } else {
        Some(build_glob_set(&ignore.paths)?)
    };
    let dev_only = if ignore.dev_only_crates {
        collect_dev_only_crates(target)
    } else {
        HashSet::new()
    };
    if gs.is_none() && !ignore.test_modules && dev_only.is_empty() {
        return Ok(0);
    }
    Ok(filter_graph(
        &mut graphs.modules,
        gs.as_ref(),
        ignore.test_modules,
        &dev_only,
    ) + filter_graph(
        &mut graphs.files,
        gs.as_ref(),
        ignore.test_modules,
        &dev_only,
    ) + filter_graph(
        &mut graphs.functions,
        gs.as_ref(),
        ignore.test_modules,
        &dev_only,
    ))
}

// ── Dev-only crate detection ───────────────────────────────────────────────────

/// Returns names of crates that are only reachable via dev-dependency edges
/// in the full transitive dependency graph (via `cargo metadata`).
fn collect_dev_only_crates(target: &Path) -> HashSet<String> {
    let out = std::process::Command::new("cargo")
        .args(["metadata", "--format-version", "1"])
        .current_dir(target)
        .stderr(std::process::Stdio::null())
        .output()
        .expect("cargo metadata failed — is cargo installed?");
    assert!(
        out.status.success(),
        "cargo metadata exited with {}",
        out.status
    );

    let meta: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("cargo metadata produced invalid JSON");

    // id → package name
    let packages = meta["packages"].as_array().expect("packages array");
    let mut id_to_name: HashMap<&str, &str> = HashMap::new();
    for pkg in packages {
        if let (Some(id), Some(name)) = (pkg["id"].as_str(), pkg["name"].as_str()) {
            id_to_name.insert(id, name);
        }
    }

    // workspace member ids
    let workspace_members: HashSet<&str> = meta["workspace_members"]
        .as_array()
        .expect("workspace_members array")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();

    // adjacency: pkg_id → [(dep_pkg_id, dev_only_edge)]
    // An edge is dev-only when every dep_kind has kind == "dev"
    // (kind == null means a normal runtime dependency).
    let nodes = meta["resolve"]["nodes"]
        .as_array()
        .expect("resolve.nodes array");
    let mut adj: HashMap<&str, Vec<(&str, bool)>> = HashMap::new();
    for node in nodes {
        let Some(id) = node["id"].as_str() else {
            continue;
        };
        let Some(deps) = node["deps"].as_array() else {
            continue;
        };
        let edges = deps
            .iter()
            .filter_map(|dep| {
                let dep_id = dep["pkg"].as_str()?;
                let kinds = dep["dep_kinds"].as_array()?;
                let dev_only = kinds.iter().all(|k| k["kind"].as_str() == Some("dev"));
                Some((dep_id, dev_only))
            })
            .collect();
        adj.insert(id, edges);
    }

    // BFS from workspace members following only non-dev edges.
    let mut regular: HashSet<&str> = workspace_members.iter().copied().collect();
    let mut queue: VecDeque<&str> = regular.iter().copied().collect();
    while let Some(id) = queue.pop_front() {
        for &(dep_id, dev_only) in adj.get(id).map(Vec::as_slice).unwrap_or(&[]) {
            if !dev_only && regular.insert(dep_id) {
                queue.push_back(dep_id);
            }
        }
    }

    // Everything in the graph but not regularly reachable is dev-only.
    adj.keys()
        .filter(|&&id| !regular.contains(id))
        .filter_map(|&id| id_to_name.get(id).map(|n| n.to_string()))
        .collect()
}

fn build_glob_set(patterns: &[String]) -> Result<GlobSet> {
    let mut b = GlobSetBuilder::new();
    for p in patterns {
        b.add(Glob::new(p).with_context(|| format!("invalid glob: {p}"))?);
    }
    Ok(b.build()?)
}

// Paths are stored as "{root}/sub/path" after relativize; strip the "{…}/" prefix.
fn strip_root_prefix(path: &str) -> &str {
    if path.starts_with('{')
        && let Some(idx) = path.find('}')
    {
        return path[idx + 1..].trim_start_matches('/');
    }
    path
}

fn filter_graph(
    graph: &mut Graph,
    gs: Option<&GlobSet>,
    test_modules: bool,
    dev_only: &HashSet<String>,
) -> usize {
    let removed: HashSet<String> = graph
        .nodes
        .iter()
        .filter(|n| {
            if let Some(gs) = gs
                && gs.is_match(strip_root_prefix(&n.path))
            {
                return true;
            }
            if test_modules && n.id.ends_with("::tests") {
                return true;
            }
            if !dev_only.is_empty() {
                // ID format after rewriting: "crate:rstest" or "crate:rstest@1.0.0"
                if let Some(crate_name) = n.id.strip_prefix("crate:") {
                    let base = crate_name.split('@').next().unwrap_or(crate_name);
                    if dev_only.contains(base) {
                        return true;
                    }
                }
            }
            false
        })
        .map(|n| n.id.clone())
        .collect();
    if removed.is_empty() {
        return 0;
    }
    let before = graph.nodes.len();
    graph.nodes.retain(|n| !removed.contains(&n.id));
    graph
        .edges
        .retain(|e| !removed.contains(&e.from) && !removed.contains(&e.to));
    for cg in &mut graph.cycles {
        cg.nodes.retain(|id| !removed.contains(id));
    }
    graph.cycles.retain(|cg| cg.nodes.len() >= 2);
    before - graph.nodes.len()
}

// ── Cycle rules ────────────────────────────────────────────────────────────────

pub fn apply_cycle_rules(graphs: &mut PluginGraphs, rules: &CycleRules) {
    apply_cycle_rules_graph(&mut graphs.modules, rules);
    apply_cycle_rules_graph(&mut graphs.files, rules);
    apply_cycle_rules_graph(&mut graphs.functions, rules);
}

fn apply_cycle_rules_graph(graph: &mut Graph, rules: &CycleRules) {
    let allowed: HashSet<CycleKind> = [
        (&CycleKind::TestEmbed, &rules.test_embed),
        (&CycleKind::Mutual, &rules.mutual),
        (&CycleKind::Chain, &rules.chain),
    ]
    .iter()
    .filter(|(_, s)| **s == Severity::Allow)
    .map(|(k, _)| (*k).clone())
    .collect();

    if allowed.is_empty() {
        return;
    }
    for node in &mut graph.nodes {
        if node
            .cycle_kind
            .as_ref()
            .map(|k| allowed.contains(k))
            .unwrap_or(false)
        {
            node.cycle_kind = None;
        }
    }
    graph.cycles.retain(|cg| !allowed.contains(&cg.kind));
}

// ── Threshold violations ───────────────────────────────────────────────────────

#[derive(Debug)]
pub struct Violation {
    pub severity: Severity,
    pub graph: &'static str,
    pub message: String,
}

impl Violation {
    pub fn is_error(&self) -> bool {
        self.severity == Severity::Deny
    }
}

pub fn check_violations(graphs: &PluginGraphs, rules: &RulesConfig) -> Vec<Violation> {
    let mut vs = Vec::new();
    check_graph_violations("modules", &graphs.modules, rules, &mut vs);
    check_graph_violations("files", &graphs.files, rules, &mut vs);
    check_graph_violations("functions", &graphs.functions, rules, &mut vs);
    vs
}

fn check_graph_violations(
    name: &'static str,
    graph: &Graph,
    rules: &RulesConfig,
    vs: &mut Vec<Violation>,
) {
    let nt = &rules.thresholds.node;

    for node in &graph.nodes {
        let Some(cx) = &node.complexity else { continue };

        if let (Some(limit), Some(c)) = (nt.hk, &cx.coupling)
            && c.hk > limit
        {
            push(
                vs,
                name,
                format!("{}: hk {:.0} > {:.0}", node.id, c.hk, limit),
            );
        }
        if let Some(limit) = nt.cyclomatic
            && cx.cyclomatic > limit
        {
            push(
                vs,
                name,
                format!(
                    "{}: cyclomatic {:.0} > {:.0}",
                    node.id, cx.cyclomatic, limit
                ),
            );
        }
        if let Some(limit) = nt.cognitive
            && cx.cognitive > limit
        {
            push(
                vs,
                name,
                format!("{}: cognitive {:.0} > {:.0}", node.id, cx.cognitive, limit),
            );
        }
        if let (Some(limit), Some(c)) = (nt.fan_in, &cx.coupling)
            && c.fan_in as f64 > limit
        {
            push(
                vs,
                name,
                format!("{}: fan_in {} > {:.0}", node.id, c.fan_in, limit),
            );
        }
        if let (Some(limit), Some(c)) = (nt.fan_out, &cx.coupling)
            && c.fan_out as f64 > limit
        {
            push(
                vs,
                name,
                format!("{}: fan_out {} > {:.0}", node.id, c.fan_out, limit),
            );
        }
        if let (Some(limit), Some(loc)) = (nt.loc, &cx.loc)
            && loc.source > limit
        {
            push(
                vs,
                name,
                format!("{}: loc {:.0} > {:.0}", node.id, loc.source, limit),
            );
        }
    }

    let at = &rules.thresholds.avg;
    let Some(stats) = &graph.stats else { return };

    if let Some(limit) = at.hk {
        let avg = stats.coupling.as_ref().map(|c| c.hk).unwrap_or(0.0);
        if avg > limit {
            push(vs, name, format!("avg hk {:.0} > {:.0}", avg, limit));
        }
    }
    if let Some(limit) = at.cyclomatic
        && stats.cyclomatic > limit
    {
        push(
            vs,
            name,
            format!("avg cyclomatic {:.1} > {:.1}", stats.cyclomatic, limit),
        );
    }
    if let Some(limit) = at.cognitive
        && stats.cognitive > limit
    {
        push(
            vs,
            name,
            format!("avg cognitive {:.1} > {:.1}", stats.cognitive, limit),
        );
    }
    if let Some(limit) = at.fan_in {
        let avg = stats.coupling.as_ref().map(|c| c.fan_in).unwrap_or(0.0);
        if avg > limit {
            push(vs, name, format!("avg fan_in {:.1} > {:.1}", avg, limit));
        }
    }
    if let Some(limit) = at.fan_out {
        let avg = stats.coupling.as_ref().map(|c| c.fan_out).unwrap_or(0.0);
        if avg > limit {
            push(vs, name, format!("avg fan_out {:.1} > {:.1}", avg, limit));
        }
    }
    if let Some(limit) = at.loc {
        let avg = stats.loc.as_ref().map(|l| l.source).unwrap_or(0.0);
        if avg > limit {
            push(vs, name, format!("avg loc {:.0} > {:.0}", avg, limit));
        }
    }
}

fn push(vs: &mut Vec<Violation>, graph: &'static str, message: String) {
    vs.push(Violation {
        severity: Severity::Deny,
        graph,
        message,
    });
}
