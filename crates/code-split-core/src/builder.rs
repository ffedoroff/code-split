use crate::graph::{Edge, EdgeKind, Graph, Node, NodeId, NodeKind};
use std::collections::HashSet;

#[derive(Debug, Default)]
pub struct GraphBuilder {
    graph: Graph,
    seen_nodes: HashSet<NodeId>,
}

impl GraphBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_node(&mut self, node: Node) -> bool {
        if self.seen_nodes.insert(node.id.clone()) {
            self.graph.nodes.push(node);
            true
        } else {
            false
        }
    }

    pub fn add_edge(&mut self, edge: Edge) {
        self.graph.edges.push(edge);
    }

    pub fn node_count(&self) -> usize {
        self.graph.nodes.len()
    }

    pub fn edge_count_of_kind(&self, kind: EdgeKind) -> usize {
        self.graph.edges.iter().filter(|e| e.kind == kind).count()
    }

    pub fn nodes(&self) -> &Vec<Node> {
        &self.graph.nodes
    }

    pub fn nodes_mut(&mut self) -> &mut Vec<Node> {
        &mut self.graph.nodes
    }

    /// Look up an existing `Fn` or `Method` node by `(path, name)`.
    /// Returns the node ID if exactly one match is found, `None` otherwise
    /// (ambiguous or not found — caller must create a new node).
    pub fn find_fn_node(&self, path: &str, name: &str) -> Option<NodeId> {
        let mut found = None;
        for node in &self.graph.nodes {
            if matches!(node.kind, NodeKind::Fn | NodeKind::Method)
                && node.path == path
                && node.name == name
            {
                if found.is_some() {
                    return None; // ambiguous — fall through to line-based lookup
                }
                found = Some(node.id.clone());
            }
        }
        found
    }

    /// Line-based fallback: find an existing `Fn` or `Method` node by
    /// `(path, name, line)`. Used when name-only lookup is ambiguous.
    pub fn find_fn_node_by_line(&self, path: &str, name: &str, line: u32) -> Option<NodeId> {
        self.graph
            .nodes
            .iter()
            .find(|n| {
                matches!(n.kind, NodeKind::Fn | NodeKind::Method)
                    && n.path == path
                    && n.name == name
                    && n.line == Some(line)
            })
            .map(|n| n.id.clone())
    }

    pub fn build(self) -> Graph {
        self.graph
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::NodeKind;

    fn node(id: &str) -> Node {
        Node {
            id: id.into(),
            kind: NodeKind::Crate,
            name: id.into(),
            path: String::new(),
            parent: None,
            external: None,
            visibility: None,
            loc: None,
            line: None,
            item_count: None,
            method_count: None,
            complexity: None,
            cycle_kind: None,
        }
    }

    #[test]
    fn add_node_deduplicates_by_id() {
        let mut b = GraphBuilder::new();
        assert!(b.add_node(node("x")));
        assert!(!b.add_node(node("x")));
        let g = b.build();
        assert_eq!(g.nodes.len(), 1);
    }
}
