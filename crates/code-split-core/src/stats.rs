use crate::graph::{AvgCoupling, Graph, GraphStats, Halstead, Loc, Maintainability, Node};

pub fn annotate_stats(graph: &mut Graph) {
    fn avg<F>(nodes: &[Node], f: F) -> f64
    where
        F: Fn(&Node) -> Option<f64>,
    {
        let vals: Vec<f64> = nodes
            .iter()
            .filter_map(f)
            .filter(|v| v.is_finite() && *v > 0.0)
            .collect();
        if vals.is_empty() {
            return 0.0;
        }
        vals.iter().sum::<f64>() / vals.len() as f64
    }

    let nodes = &graph.nodes;

    let cyclomatic = avg(nodes, |n| n.complexity.as_ref().map(|c| c.cyclomatic));
    let cognitive = avg(nodes, |n| n.complexity.as_ref().map(|c| c.cognitive));

    let fan_in = avg(nodes, |n| {
        n.complexity
            .as_ref()
            .and_then(|c| c.coupling.as_ref())
            .map(|c| c.fan_in as f64)
    });
    let fan_out = avg(nodes, |n| {
        n.complexity
            .as_ref()
            .and_then(|c| c.coupling.as_ref())
            .map(|c| c.fan_out as f64)
    });
    let hk = avg(nodes, |n| {
        n.complexity
            .as_ref()
            .and_then(|c| c.coupling.as_ref())
            .map(|c| c.hk)
    });
    let coupling = (fan_in > 0.0 || fan_out > 0.0 || hk > 0.0).then_some(AvgCoupling {
        fan_in,
        fan_out,
        hk,
    });

    let mi = avg(nodes, |n| {
        n.complexity
            .as_ref()
            .and_then(|c| c.maintainability.as_ref())
            .map(|m| m.mi)
    });
    let mi_sei = avg(nodes, |n| {
        n.complexity
            .as_ref()
            .and_then(|c| c.maintainability.as_ref())
            .map(|m| m.mi_sei)
    });
    let maintainability = (mi > 0.0).then_some(Maintainability { mi, mi_sei });

    let loc_source = avg(nodes, |n| {
        n.complexity
            .as_ref()
            .and_then(|c| c.loc.as_ref())
            .map(|l| l.source)
    });
    let loc_comments = avg(nodes, |n| {
        n.complexity
            .as_ref()
            .and_then(|c| c.loc.as_ref())
            .map(|l| l.comments)
    });
    let loc_blank = avg(nodes, |n| {
        n.complexity
            .as_ref()
            .and_then(|c| c.loc.as_ref())
            .map(|l| l.blank)
    });
    let loc = (loc_source > 0.0).then_some(Loc {
        source: loc_source,
        logical: 0.0,
        comments: loc_comments,
        blank: loc_blank,
    });

    let h_length = avg(nodes, |n| {
        n.complexity
            .as_ref()
            .and_then(|c| c.halstead.as_ref())
            .map(|h| h.length)
    });
    let h_vocabulary = avg(nodes, |n| {
        n.complexity
            .as_ref()
            .and_then(|c| c.halstead.as_ref())
            .map(|h| h.vocabulary)
    });
    let h_volume = avg(nodes, |n| {
        n.complexity
            .as_ref()
            .and_then(|c| c.halstead.as_ref())
            .map(|h| h.volume)
    });
    let h_effort = avg(nodes, |n| {
        n.complexity
            .as_ref()
            .and_then(|c| c.halstead.as_ref())
            .map(|h| h.effort)
    });
    let h_time = avg(nodes, |n| {
        n.complexity
            .as_ref()
            .and_then(|c| c.halstead.as_ref())
            .map(|h| h.time)
    });
    let h_bugs = avg(nodes, |n| {
        n.complexity
            .as_ref()
            .and_then(|c| c.halstead.as_ref())
            .map(|h| h.bugs)
    });
    let halstead = (h_volume > 0.0).then_some(Halstead {
        length: h_length,
        vocabulary: h_vocabulary,
        volume: h_volume,
        effort: h_effort,
        time: h_time,
        bugs: h_bugs,
    });

    if cyclomatic == 0.0
        && cognitive == 0.0
        && coupling.is_none()
        && maintainability.is_none()
        && loc.is_none()
        && halstead.is_none()
    {
        return;
    }

    graph.stats = Some(GraphStats {
        cyclomatic,
        cognitive,
        coupling,
        maintainability,
        loc,
        halstead,
    });
}
