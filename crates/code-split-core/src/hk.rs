use crate::{Complexity, Coupling, EdgeKind, Graph, NodeId, NodeKind, PluginGraphs};
use std::collections::{HashMap, HashSet};

pub fn annotate_hk(graphs: &mut PluginGraphs) {
    annotate_graph_hk(&mut graphs.modules);
    annotate_graph_hk(&mut graphs.files);
    annotate_graph_hk(&mut graphs.functions);
}

fn annotate_graph_hk(graph: &mut Graph) {
    // If the graph has no Calls edges (sema was skipped), fn/method nodes get
    // no coupling annotation — showing 0 would be misleading vs. genuinely
    // isolated nodes discovered when sema did run.
    let has_calls = graph.edges.iter().any(|e| e.kind == EdgeKind::Calls);

    let mut fan_in: HashMap<NodeId, HashSet<NodeId>> = HashMap::new();
    let mut fan_out: HashMap<NodeId, HashSet<NodeId>> = HashMap::new();

    for edge in &graph.edges {
        if edge.kind == EdgeKind::Contains {
            continue;
        }
        fan_out
            .entry(edge.from.clone())
            .or_default()
            .insert(edge.to.clone());
        fan_in
            .entry(edge.to.clone())
            .or_default()
            .insert(edge.from.clone());
    }

    for node in &mut graph.nodes {
        if !has_calls && matches!(node.kind, NodeKind::Fn | NodeKind::Method) {
            continue;
        }
        let fi = fan_in.get(&node.id).map(|s| s.len()).unwrap_or(0);
        let fo = fan_out.get(&node.id).map(|s| s.len()).unwrap_or(0);
        let loc = node.loc.unwrap_or(0) as f64;
        let hk_term = ((fi * fo) as f64).powi(2);
        let hk = if loc > 0.0 { loc * hk_term } else { hk_term };

        let cx = node.complexity.get_or_insert_with(Complexity::default);
        cx.coupling = Some(Coupling {
            fan_in: fi as u32,
            fan_out: fo as u32,
            hk,
        });
    }
}
