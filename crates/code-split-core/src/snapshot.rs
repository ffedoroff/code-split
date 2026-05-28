use crate::graph::{Graph, NodeKind};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageTime {
    pub stage: String,
    pub ms: u64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub schema_version: String,
    pub generated_at: DateTime<Utc>,
    pub command: String,
    /// Directory from which `code-split` was invoked.
    pub workspace: String,
    /// The analyzed project directory (absolute path, stored once here).
    pub target: String,
    pub plugin: String,
    /// Config file used for this analysis, if any was found.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_file: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub local_only: bool,
    pub versions: HashMap<String, String>,
    /// Named system roots used to shorten node paths (e.g. `{cargo}`, `{rustup}`).
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub roots: HashMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git: Option<GitInfo>,
    /// Per-stage timing in milliseconds, in execution order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub timings: Vec<StageTime>,
    pub graphs: PluginGraphs,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitInfo {
    pub branch: String,
    pub commit: String,
    pub dirty_files: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginGraphs {
    pub modules: Graph,
    /// Omitted when the plugin produces no file-kind nodes (e.g. Rust).
    #[serde(default, skip_serializing_if = "Graph::is_empty")]
    pub files: Graph,
    pub functions: Graph,
}

impl Snapshot {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        command: String,
        workspace: String,
        target: String,
        plugin: String,
        config_file: Option<String>,
        local_only: bool,
        versions: HashMap<String, String>,
        roots: HashMap<String, String>,
        git: Option<GitInfo>,
        timings: Vec<StageTime>,
        graphs: PluginGraphs,
    ) -> Self {
        Self {
            schema_version: "1".to_string(),
            generated_at: Utc::now(),
            command,
            workspace,
            target,
            plugin,
            config_file,
            local_only,
            versions,
            roots,
            git,
            timings,
            graphs,
        }
    }
}

// ---------------------------------------------------------------------------
// Path relativization
// ---------------------------------------------------------------------------

/// Rewrite all node `path` fields to be relative:
/// - paths under `target` → plain relative path (`src/main.rs`)
/// - paths under a named root → `{name}/rest/of/path`
/// - anything else → left as-is
pub fn relativize_graphs(
    graphs: &mut PluginGraphs,
    target: &Path,
    roots: &HashMap<String, String>,
) {
    for graph in [
        &mut graphs.modules,
        &mut graphs.files,
        &mut graphs.functions,
    ] {
        for node in &mut graph.nodes {
            node.path = relativize_path(&node.path, target, roots);
        }
    }
}

pub(crate) fn relativize_path(
    path: &str,
    target: &Path,
    roots: &HashMap<String, String>,
) -> String {
    if path.is_empty() {
        return path.to_string();
    }
    let p = Path::new(path);
    // target first — local paths become {target}/src/...
    if let Ok(rel) = p.strip_prefix(target) {
        return format!("{{target}}/{}", rel.to_string_lossy());
    }
    // Longest root wins.
    let mut sorted: Vec<_> = roots.iter().collect();
    sorted.sort_by_key(|(_, root)| Reverse(root.len()));
    for (name, root) in &sorted {
        if let Ok(rel) = p.strip_prefix(root.as_str()) {
            return format!("{{{name}}}/{}", rel.to_string_lossy());
        }
    }
    path.to_string()
}

// ---------------------------------------------------------------------------
// ID rewriting  (Variant A: {crate_name}::{path}, version only on conflict)
// ---------------------------------------------------------------------------

/// Rewrite all node `id`, `parent` and edge `from`/`to` fields from the raw
/// cargo-based IDs to short human-readable IDs.
///
/// Scheme:
/// - `crate:{pkg_repr}` → `crate:anyhow` / `crate:anyhow@1.0.102` (conflict)
/// - `mod:{pkg_repr}::{path}` → `mod:anyhow::{path}`
/// - `trait:{pkg_repr}::{path}` → `trait:anyhow::{path}`
/// - `file:{abs_path}` → `file:{rel_path}` (relativized via roots)
pub fn rewrite_ids(graphs: &mut PluginGraphs, target: &Path, roots: &HashMap<String, String>) {
    // Step 1: collect (pkg_repr → (name, version)) from crate nodes.
    let mut pkg_info: HashMap<String, (String, String)> = HashMap::new();
    for node in graphs
        .modules
        .nodes
        .iter()
        .chain(graphs.files.nodes.iter())
        .chain(graphs.functions.nodes.iter())
    {
        if node.kind == NodeKind::Crate
            && let Some(pkg_repr) = node.id.strip_prefix("crate:")
        {
            pkg_info
                .entry(pkg_repr.to_string())
                .or_insert_with(|| parse_pkg_repr(pkg_repr));
        }
    }

    // Step 2: detect name conflicts (same name, different versions).
    let mut name_versions: HashMap<String, HashSet<String>> = HashMap::new();
    for (name, version) in pkg_info.values() {
        name_versions
            .entry(name.clone())
            .or_default()
            .insert(version.clone());
    }

    // Step 3: pkg_repr → short crate identifier.
    let crate_map: HashMap<String, String> = pkg_info
        .iter()
        .map(|(repr, (name, version))| {
            let conflict = name_versions.get(name).is_some_and(|v| v.len() > 1);
            let short = if conflict && !version.is_empty() {
                format!("{name}@{version}")
            } else {
                name.clone()
            };
            (repr.clone(), short)
        })
        .collect();

    // Step 4: build old_id → new_id for every node across all graphs.
    let mut id_map: HashMap<String, String> = HashMap::new();
    for node in graphs
        .modules
        .nodes
        .iter()
        .chain(graphs.files.nodes.iter())
        .chain(graphs.functions.nodes.iter())
    {
        let new_id = rewrite_node_id(&node.id, &crate_map, target, roots);
        if new_id != node.id {
            id_map.insert(node.id.clone(), new_id);
        }
    }

    // Step 5: apply mapping to nodes and edges.
    for graph in [
        &mut graphs.modules,
        &mut graphs.files,
        &mut graphs.functions,
    ] {
        for node in &mut graph.nodes {
            if let Some(new_id) = id_map.get(&node.id) {
                node.id = new_id.clone();
            }
            if let Some(parent) = node.parent.as_mut() {
                if let Some(new_parent) = id_map.get(parent.as_str()) {
                    *parent = new_parent.clone();
                } else {
                    // parent references a node not in any graph (e.g. stdlib file node);
                    // rewrite it directly instead of relying on id_map lookup.
                    let rewritten = rewrite_node_id(parent, &crate_map, target, roots);
                    if rewritten != *parent {
                        *parent = rewritten;
                    }
                }
            }
        }
        for edge in &mut graph.edges {
            if let Some(v) = id_map.get(&edge.from) {
                edge.from = v.clone();
            }
            if let Some(v) = id_map.get(&edge.to) {
                edge.to = v.clone();
            }
        }
    }
}

fn rewrite_node_id(
    id: &str,
    crate_map: &HashMap<String, String>,
    target: &Path,
    roots: &HashMap<String, String>,
) -> String {
    // crate:
    if let Some(pkg_repr) = id.strip_prefix("crate:") {
        let short = crate_map
            .get(pkg_repr)
            .cloned()
            .unwrap_or_else(|| parse_pkg_repr(pkg_repr).0);
        return format!("crate:{short}");
    }
    // mod: / trait: / fn: / method:
    for kind in ["mod", "trait", "fn", "method"] {
        let prefix = format!("{kind}:");
        if let Some(rest) = id.strip_prefix(&prefix)
            && let Some((pkg_repr, path_part)) = split_version_boundary(rest)
        {
            let short = crate_map
                .get(&pkg_repr)
                .cloned()
                .unwrap_or_else(|| parse_pkg_repr(&pkg_repr).0);
            // Strip redundant `{crate_name}::` prefix from path when target == crate.
            let trimmed = path_part
                .strip_prefix(&format!("{short}::"))
                .unwrap_or(&path_part)
                .to_string();
            return format!("{kind}:{short}::{trimmed}");
        }
    }
    // file:
    if let Some(abs_path) = id.strip_prefix("file:") {
        let rel = relativize_path(abs_path, target, roots);
        return format!("file:{rel}");
    }
    id.to_string()
}

/// Split `path+file:///path#0.1.0::mod::sub` into
/// (`path+file:///path#0.1.0`, `mod::sub`).
fn split_version_boundary(s: &str) -> Option<(String, String)> {
    let hash_pos = s.find('#')?;
    let after_hash = &s[hash_pos + 1..];
    let colon_pos = after_hash.find("::")?;
    let pkg_repr = s[..hash_pos + 1 + colon_pos].to_string();
    let path_part = after_hash[colon_pos + 2..].to_string();
    Some((pkg_repr, path_part))
}

/// Extract `(name, version)` from a raw cargo package repr.
///
/// Examples:
/// - `path+file:///path/to/anyhow#1.0.102`   → (`anyhow`, `1.0.102`)
/// - `registry+https://...#anyhow@1.0.102`   → (`anyhow`, `1.0.102`)
/// - `git+https://...?tag=v0.1.0#a3f9c21`    → (`repo-name`, `v0.1.0`)
fn parse_pkg_repr(repr: &str) -> (String, String) {
    if let Some(hash_pos) = repr.rfind('#') {
        let after = &repr[hash_pos + 1..];
        // registry style: name@version after #
        if let Some((name, ver)) = after.split_once('@') {
            return (name.to_string(), ver.to_string());
        }
        // path style: version is after #, name is last component before #
        let version = after.to_string();
        let before = &repr[..hash_pos];
        // strip query string (?tag=...) if present
        let before = before.split('?').next().unwrap_or(before);
        let name = before
            .split('/')
            .next_back()
            .unwrap_or("unknown")
            .to_string();
        return (name, version);
    }
    // Fallback: use the whole thing as name
    (repr.to_string(), String::new())
}
