use crate::{Graph, Snapshot};
use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize)]
pub struct DiffCounts {
    pub added: usize,
    pub removed: usize,
    pub affected: usize,
    pub unchanged: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct LevelDiff {
    pub nodes: DiffCounts,
    pub edges: DiffCounts,
    /// Nodes participating in cycles (SCCs with ≥ 2 members).
    pub cycle_nodes_before: usize,
    pub cycle_nodes_after: usize,
    /// Number of SCCs.
    pub sccs_before: usize,
    pub sccs_after: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct SnapMeta {
    pub target: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CompareSummary {
    pub schema_version: String,
    pub before: SnapMeta,
    pub after: SnapMeta,
    pub identical: bool,
    pub modules: LevelDiff,
    pub files: LevelDiff,
    pub functions: LevelDiff,
}

pub fn compare_snapshots(before: &Snapshot, after: &Snapshot) -> CompareSummary {
    let modules = diff_graph(&before.graphs.modules, &after.graphs.modules);
    let files = diff_graph(&before.graphs.files, &after.graphs.files);
    let functions = diff_graph(&before.graphs.functions, &after.graphs.functions);

    let identical = [&modules, &files, &functions].iter().all(|d| {
        d.nodes.added == 0
            && d.nodes.removed == 0
            && d.nodes.affected == 0
            && d.edges.added == 0
            && d.edges.removed == 0
            && d.edges.affected == 0
    });

    CompareSummary {
        schema_version: "1".to_string(),
        before: snap_meta(before),
        after: snap_meta(after),
        identical,
        modules,
        files,
        functions,
    }
}

fn snap_meta(snap: &Snapshot) -> SnapMeta {
    let commit_short = snap
        .git
        .as_ref()
        .map(|g| g.commit[..8.min(g.commit.len())].to_string());
    SnapMeta {
        target: snap
            .target
            .split('/')
            .next_back()
            .unwrap_or(&snap.target)
            .to_string(),
        branch: snap.git.as_ref().map(|g| g.branch.clone()),
        commit: commit_short,
    }
}

// Mirrors computeDiff() from assets/diff.js.
fn diff_graph(before: &Graph, after: &Graph) -> LevelDiff {
    // Non-external node id sets.
    let bg: HashMap<String, ()> = before
        .nodes
        .iter()
        .filter(|n| !n.external.unwrap_or(false))
        .map(|n| (n.id.clone(), ()))
        .collect();
    let ag: HashMap<String, ()> = after
        .nodes
        .iter()
        .filter(|n| !n.external.unwrap_or(false))
        .map(|n| (n.id.clone(), ()))
        .collect();

    // Node status: 0 = unchanged, 1 = added, 2 = removed, 3 = affected.
    let mut node_status: HashMap<String, u8> = HashMap::new();
    for id in ag.keys() {
        node_status.insert(id.clone(), if bg.contains_key(id) { 0 } else { 1 });
    }
    for id in bg.keys() {
        if !ag.contains_key(id) {
            node_status.insert(id.clone(), 2);
        }
    }

    // Edge key: "from\0to\0Kind".
    let ekey = |e: &crate::Edge| format!("{}\x00{}\x00{:?}", e.from, e.to, e.kind);

    // Local edges: both endpoints present in node_status.
    let local_edges = |edges: &[crate::Edge]| -> HashMap<String, (String, String)> {
        edges
            .iter()
            .filter(|e| node_status.contains_key(&e.from) && node_status.contains_key(&e.to))
            .map(|e| (ekey(e), (e.from.clone(), e.to.clone())))
            .collect()
    };

    let bg_edges = local_edges(&before.edges);
    let ag_edges = local_edges(&after.edges);

    // Collect all edges with status.
    let mut edge_list: Vec<(String, String, u8)> = Vec::new();
    for (key, (from, to)) in &ag_edges {
        edge_list.push((
            from.clone(),
            to.clone(),
            if bg_edges.contains_key(key) { 0 } else { 1 },
        ));
    }
    for (key, (from, to)) in &bg_edges {
        if !ag_edges.contains_key(key) {
            edge_list.push((from.clone(), to.clone(), 2));
        }
    }

    // Propagate "affected" to unchanged nodes adjacent to changed edges.
    for (from, to, status) in &edge_list {
        if *status != 0 {
            if node_status.get(from.as_str()) == Some(&0) {
                node_status.insert(from.clone(), 3);
            }
            if node_status.get(to.as_str()) == Some(&0) {
                node_status.insert(to.clone(), 3);
            }
        }
    }

    // Count nodes.
    let mut nodes = DiffCounts {
        added: 0,
        removed: 0,
        affected: 0,
        unchanged: 0,
    };
    for &s in node_status.values() {
        match s {
            1 => nodes.added += 1,
            2 => nodes.removed += 1,
            3 => nodes.affected += 1,
            _ => nodes.unchanged += 1,
        }
    }

    // Count edges (unchanged edge connecting non-unchanged nodes → affected).
    let mut edges = DiffCounts {
        added: 0,
        removed: 0,
        affected: 0,
        unchanged: 0,
    };
    for (from, to, status) in &edge_list {
        let s = if *status == 0
            && (node_status.get(from.as_str()) != Some(&0)
                || node_status.get(to.as_str()) != Some(&0))
        {
            3u8
        } else {
            *status
        };
        match s {
            1 => edges.added += 1,
            2 => edges.removed += 1,
            3 => edges.affected += 1,
            _ => edges.unchanged += 1,
        }
    }

    LevelDiff {
        nodes,
        edges,
        cycle_nodes_before: before.cycles.iter().map(|c| c.nodes.len()).sum(),
        cycle_nodes_after: after.cycles.iter().map(|c| c.nodes.len()).sum(),
        sccs_before: before.cycles.len(),
        sccs_after: after.cycles.len(),
    }
}
