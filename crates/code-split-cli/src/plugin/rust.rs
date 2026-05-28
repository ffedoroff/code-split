use anyhow::Result;
use code_split_core::{EdgeKind, GraphBuilder, NodeKind, PluginGraphs, SemanticIndex, StageTime};
use std::path::Path;

use crate::logger;

pub fn run(
    workspace: &Path,
    local_only: bool,
    want_functions: bool,
) -> Result<(PluginGraphs, Vec<StageTime>)> {
    let mut timings: Vec<StageTime> = Vec::new();
    let mut builder = GraphBuilder::new();

    {
        let t = logger::Timer::start("syn: parsing modules and files");
        if local_only {
            code_split_syn::analyze_local_only(workspace, &mut builder)?;
        } else {
            code_split_syn::analyze(workspace, &mut builder)?;
        }
        let n = builder.node_count();
        let detail = format!("{n} nodes");
        let ms = t.finish_with(&detail);
        timings.push(StageTime {
            stage: "syn".into(),
            ms,
            detail,
        });
    }

    let has_cargo = which::which("cargo").is_ok();
    if want_functions && !local_only && has_cargo {
        let t = logger::Timer::start("sema: building call graph via rust-analyzer");
        let res = code_split_sema::RustAnalyzerSemantic.analyze(workspace, &mut builder);
        let calls = builder.edge_count_of_kind(code_split_core::EdgeKind::Calls);
        let detail = format!("{calls} call edges");
        let ms = match res {
            Ok(_) => t.finish_with(&detail),
            Err(e) => {
                logger::info(&format!("sema skipped: {e:#}"));
                0
            }
        };
        timings.push(StageTime {
            stage: "sema".into(),
            ms,
            detail,
        });
    } else {
        let reason = if !want_functions {
            "functions graph not requested"
        } else if local_only {
            "--local-only"
        } else {
            "cargo not found"
        };
        logger::info(&format!("sema: skipped ({reason})"));
        timings.push(StageTime {
            stage: "sema".into(),
            ms: 0,
            detail: format!("skipped ({reason})"),
        });
    }

    {
        let t = logger::Timer::start("complexity: cyclomatic / cognitive / halstead / MI / LOC");
        let annotated = match code_split_complexity::analyze(workspace, &mut builder) {
            Ok(n) => n,
            Err(e) => {
                logger::info(&format!("complexity skipped: {e:#}"));
                0
            }
        };
        let detail = format!("{annotated} nodes annotated");
        let ms = t.finish_with(&detail);
        timings.push(StageTime {
            stage: "complexity".into(),
            ms,
            detail,
        });
    }

    let t = logger::Timer::start("projecting graphs (modules / files / functions)");
    let full = builder.build();

    let modules = full.project(
        &[NodeKind::Crate, NodeKind::Module, NodeKind::Trait],
        &[EdgeKind::Contains, EdgeKind::Uses, EdgeKind::Reexports],
    );
    // Rust analysis produces no NodeKind::File nodes — files graph is empty.
    // File-level tracking is implemented in the Python/JS/TS plugins.
    let files = full.project(&[NodeKind::File], &[EdgeKind::Contains, EdgeKind::Uses]);
    let functions = full.project(
        &[
            NodeKind::Crate,
            NodeKind::Module,
            NodeKind::Fn,
            NodeKind::Method,
            NodeKind::Trait,
        ],
        &[
            EdgeKind::Contains,
            EdgeKind::Uses,
            EdgeKind::Reexports,
            EdgeKind::Calls,
        ],
    );
    let detail = format!(
        "modules={} files={} functions={}",
        modules.nodes.len(),
        files.nodes.len(),
        functions.nodes.len(),
    );
    let ms = t.finish_with(&detail);
    timings.push(StageTime {
        stage: "projection".into(),
        ms,
        detail,
    });

    Ok((
        PluginGraphs {
            modules,
            files,
            functions,
        },
        timings,
    ))
}

pub fn version_string() -> Option<String> {
    which::which("rustc").ok()?;
    let out = std::process::Command::new("rustc")
        .arg("--version")
        .output()
        .ok()?;
    if out.status.success() {
        Some(
            String::from_utf8_lossy(&out.stdout)
                .split_whitespace()
                .nth(1)
                .unwrap_or("unknown")
                .to_string(),
        )
    } else {
        None
    }
}
