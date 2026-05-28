use anyhow::{Context, Result};
use code_split_core::{Edge, EdgeKind, GraphBuilder, Node, NodeId, NodeKind, SemanticIndex};
use ra_ap_hir::{
    self as hir, AsAssocItem, AssocItem, AssocItemContainer, Crate, HasSource, HirDisplay,
    ModuleDef, Semantics, attach_db,
};
use ra_ap_ide::{AnalysisHost, RootDatabase};
use ra_ap_ide_db::{base_db::SourceDatabase, line_index};
use ra_ap_load_cargo::{LoadCargoConfig, ProcMacroServerChoice, load_workspace_at};
use ra_ap_project_model::{CargoConfig, RustLibSource};
use ra_ap_syntax::{AstNode, ast};
use ra_ap_vfs::Vfs;
use std::collections::{HashMap, HashSet};
use std::convert::Infallible;
use std::path::Path;

#[derive(Debug, Default)]
pub struct NullSemanticIndex;

impl SemanticIndex for NullSemanticIndex {
    type Error = Infallible;

    fn analyze(&self, _workspace: &Path, _builder: &mut GraphBuilder) -> Result<(), Self::Error> {
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct RustAnalyzerSemantic;

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct SemaError(#[from] pub anyhow::Error);

impl SemanticIndex for RustAnalyzerSemantic {
    type Error = SemaError;

    fn analyze(&self, workspace: &Path, builder: &mut GraphBuilder) -> Result<(), Self::Error> {
        analyze_inner(workspace, builder).map_err(SemaError)
    }
}

fn analyze_inner(workspace: &Path, builder: &mut GraphBuilder) -> Result<()> {
    let cargo_config = CargoConfig {
        sysroot: Some(RustLibSource::Discover),
        all_targets: true,
        ..Default::default()
    };
    let load_config = LoadCargoConfig {
        load_out_dirs_from_check: true,
        with_proc_macro_server: ProcMacroServerChoice::Sysroot,
        prefill_caches: false,
        num_worker_threads: 1,
        proc_macro_processes: 1,
    };

    let (db, vfs, _pm) = load_workspace_at(workspace, &cargo_config, &load_config, &|_| {})
        .context("load_workspace_at failed")?;
    let host = AnalysisHost::with_database(db);
    let db: &RootDatabase = host.raw_database();

    attach_db(db, || analyze_with_db(db, &vfs, builder))
}

fn analyze_with_db(db: &RootDatabase, vfs: &Vfs, builder: &mut GraphBuilder) -> Result<()> {
    let sema = Semantics::new(db);

    let mut fn_node_id: HashMap<hir::Function, NodeId> = HashMap::new();
    let mut emitted_fns: HashSet<NodeId> = HashSet::new();
    let mut emitted_edges: HashSet<(NodeId, NodeId)> = HashSet::new();

    for krate in Crate::all(db) {
        let root_file = krate.root_file(db);
        let sr_id = db.file_source_root(root_file).source_root_id(db);
        if db.source_root(sr_id).source_root(db).is_library {
            continue;
        }

        for module in krate.modules(db) {
            let mut callers: Vec<hir::Function> = Vec::new();
            for decl in module.declarations(db) {
                match decl {
                    ModuleDef::Function(f) => callers.push(f),
                    ModuleDef::Trait(t) => {
                        for item in t.items(db) {
                            if let AssocItem::Function(f) = item {
                                callers.push(f);
                            }
                        }
                    }
                    _ => {}
                }
            }
            for impl_def in module.impl_defs(db) {
                for item in impl_def.items(db) {
                    if let AssocItem::Function(f) = item {
                        callers.push(f);
                    }
                }
            }

            for caller in callers {
                let Some(caller_id) =
                    ensure_fn_node(caller, db, vfs, &mut fn_node_id, &mut emitted_fns, builder)
                else {
                    continue;
                };

                let Some(src) = sema.source(caller) else {
                    continue;
                };
                let Some(body) = src.value.body() else {
                    continue;
                };

                for node in body.syntax().descendants() {
                    if let Some(mc) = ast::MethodCallExpr::cast(node.clone()) {
                        if let Some(callee) = sema.resolve_method_call(&mc) {
                            record_call(
                                caller_id.clone(),
                                callee,
                                db,
                                vfs,
                                &mut fn_node_id,
                                &mut emitted_fns,
                                &mut emitted_edges,
                                builder,
                            );
                        }
                        continue;
                    }
                    if let Some(ce) = ast::CallExpr::cast(node) {
                        let expr: ast::Expr = ce.into();
                        if let Some(callable) = sema.resolve_expr_as_callable(&expr)
                            && let hir::CallableKind::Function(f) = callable.kind()
                        {
                            record_call(
                                caller_id.clone(),
                                f,
                                db,
                                vfs,
                                &mut fn_node_id,
                                &mut emitted_fns,
                                &mut emitted_edges,
                                builder,
                            );
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn record_call(
    caller_id: NodeId,
    callee: hir::Function,
    db: &RootDatabase,
    vfs: &Vfs,
    fn_node_id: &mut HashMap<hir::Function, NodeId>,
    emitted_fns: &mut HashSet<NodeId>,
    emitted_edges: &mut HashSet<(NodeId, NodeId)>,
    builder: &mut GraphBuilder,
) {
    let Some(callee_id) = ensure_fn_node(callee, db, vfs, fn_node_id, emitted_fns, builder) else {
        return;
    };
    if !emitted_edges.insert((caller_id.clone(), callee_id.clone())) {
        return;
    }
    builder.add_edge(Edge {
        from: caller_id,
        to: callee_id,
        kind: EdgeKind::Calls,
        unresolved: None,
        external: None,
        visibility: None,
    });
}

fn ensure_fn_node(
    f: hir::Function,
    db: &RootDatabase,
    vfs: &Vfs,
    fn_node_id: &mut HashMap<hir::Function, NodeId>,
    emitted_fns: &mut HashSet<NodeId>,
    builder: &mut GraphBuilder,
) -> Option<NodeId> {
    if let Some(id) = fn_node_id.get(&f) {
        return Some(id.clone());
    }

    let src = f.source(db)?;

    if src.file_id.is_macro() {
        return None;
    }

    let file_id = src.file_id.original_file(db).file_id(db);
    let path = vfs.file_path(file_id);
    let path_str = path.as_path()?.to_string();

    let name = f.name(db).as_str().to_owned();
    let is_method = f.as_assoc_item(db).is_some();
    let kind = if is_method {
        NodeKind::Method
    } else {
        NodeKind::Fn
    };

    // Crate name
    let krate = f.module(db).krate(db);
    let crate_name = krate
        .display_name(db)
        .map(|n| n.crate_name().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let display_target = krate.to_display_target(db);

    // Module path within the crate (e.g. "handlers::error")
    let mod_path = build_module_path(f.module(db), db);

    // For methods: impl type name or trait name
    let container = if is_method {
        f.as_assoc_item(db).map(|assoc| match assoc.container(db) {
            AssocItemContainer::Impl(impl_def) => {
                let ty = impl_def.self_ty(db).display(db, display_target).to_string();
                // Strip generics and leading path: "path::Foo<Bar>" → "Foo"
                let ty = ty.split('<').next().unwrap_or(&ty);
                ty.split("::").last().unwrap_or(ty).trim().to_string()
            }
            AssocItemContainer::Trait(t) => t.name(db).as_str().to_owned(),
        })
    } else {
        None
    };

    // Stable ID: no line numbers, no byte offsets
    let id = match (&mod_path[..], container.as_deref()) {
        ("", Some(c)) => format!("method:{crate_name}::{c}::{name}"),
        (m, Some(c)) => format!("method:{crate_name}::{m}::{c}::{name}"),
        ("", None) => format!("fn:{crate_name}::{name}"),
        (m, None) => format!("fn:{crate_name}::{m}::{name}"),
    };

    // Reuse existing syn-created node when available (avoids duplicate nodes
    // with different ID schemes for the same local workspace function).
    let sema_line = {
        let range = src.value.syntax().text_range();
        let li = line_index(db, file_id);
        li.try_line_col(range.start()).map(|lc| lc.line + 1)
    };
    let existing_id = builder
        .find_fn_node(&path_str, &name)
        .or_else(|| sema_line.and_then(|l| builder.find_fn_node_by_line(&path_str, &name, l)));
    if let Some(existing_id) = existing_id {
        fn_node_id.insert(f, existing_id.clone());
        emitted_fns.insert(existing_id.clone());
        return Some(existing_id);
    }

    if emitted_fns.insert(id.clone()) {
        let (line, loc) = {
            let range = src.value.syntax().text_range();
            let li = line_index(db, file_id);
            let start = li.try_line_col(range.start());
            let end = li.try_line_col(range.end());
            let loc = start.zip(end).map(|(s, e)| e.line - s.line + 1);
            (sema_line, loc)
        };
        builder.add_node(Node {
            id: id.clone(),
            kind,
            name,
            path: path_str.clone(),
            parent: Some(format!("file:{path_str}")),
            external: None,
            visibility: None,
            loc,
            line,
            item_count: None,
            method_count: None,
            complexity: None,
            cycle_kind: None,
        });
    }

    fn_node_id.insert(f, id.clone());
    Some(id)
}

/// Walk up the module hierarchy to build a `::` separated path,
/// excluding the crate root (which has no name).
fn build_module_path(module: hir::Module, db: &RootDatabase) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut m = module;
    loop {
        match m.name(db) {
            Some(name) => parts.push(name.as_str().to_owned()),
            None => break,
        }
        match m.parent(db) {
            Some(parent) => m = parent,
            None => break,
        }
    }
    parts.reverse();
    parts.join("::")
}
