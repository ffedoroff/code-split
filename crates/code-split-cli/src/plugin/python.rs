use anyhow::Result;
use code_split_core::{
    EdgeKind, GraphBuilder, NodeKind, PluginGraphs, StageTime,
    graph::{Edge, Node, Visibility},
};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::logger;

pub fn run(
    workspace: &Path,
    _local_only: bool,
    _want_functions: bool,
) -> Result<(PluginGraphs, Vec<StageTime>)> {
    let mut timings = Vec::new();
    let mut builder = GraphBuilder::new();

    let t = logger::Timer::start("python: scan + parse + build graph");

    let py_files = collect_py_files(workspace);
    let module_index = build_module_index(workspace, &py_files);

    for abs_path in &py_files {
        let Some(mod_path) = file_to_module_path(workspace, abs_path) else {
            continue;
        };
        add_package_ancestors(&mod_path, workspace, &mut builder);
        let _ = parse_and_add(abs_path, &mod_path, workspace, &module_index, &mut builder);
    }

    let n = builder.node_count();
    let detail = format!("{n} nodes from {} files", py_files.len());
    let ms = t.finish_with(&detail);
    timings.push(StageTime {
        stage: "python".into(),
        ms,
        detail,
    });

    {
        let t = logger::Timer::start("complexity: cyclomatic / cognitive / halstead / MI / LOC");
        let annotated = match code_split_complexity::analyze_python(workspace, &mut builder) {
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

    {
        let t = logger::Timer::start("sema: heuristic call graph (tree-sitter)");
        let name_index = build_fn_name_index(&builder);
        let mut call_count = 0usize;
        for abs_path in &py_files {
            let Some(mod_path) = file_to_module_path(workspace, abs_path) else {
                continue;
            };
            match extract_calls_py(abs_path, &mod_path, &name_index) {
                Ok(calls) => {
                    call_count += calls.len();
                    for (from, to) in calls {
                        builder.add_edge(Edge {
                            from,
                            to,
                            kind: EdgeKind::Calls,
                            unresolved: None,
                            external: None,
                            visibility: None,
                        });
                    }
                }
                Err(e) => logger::info(&format!("sema: skipped {}: {e:#}", abs_path.display())),
            }
        }
        let detail = format!("{call_count} call edges");
        let ms = t.finish_with(&detail);
        timings.push(StageTime {
            stage: "sema".into(),
            ms,
            detail,
        });
    }

    let t = logger::Timer::start("projecting graphs (modules / files / functions)");
    let full = builder.build();

    let modules = full.project(&[NodeKind::Module], &[EdgeKind::Contains, EdgeKind::Uses]);
    let files = full.project(
        &[NodeKind::Module, NodeKind::File],
        &[EdgeKind::Contains, EdgeKind::Uses],
    );
    let functions = full.project(
        &[
            NodeKind::Module,
            NodeKind::File,
            NodeKind::Impl,
            NodeKind::Fn,
            NodeKind::Method,
        ],
        &[EdgeKind::Contains, EdgeKind::Uses, EdgeKind::Calls],
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

// ---------------------------------------------------------------------------
// File discovery
// ---------------------------------------------------------------------------

fn collect_py_files(workspace: &Path) -> Vec<PathBuf> {
    WalkDir::new(workspace)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_type().is_file()
                && e.path().extension().is_some_and(|x| x == "py")
                && !is_skip_path(e.path(), workspace)
        })
        .map(|e| e.into_path())
        .collect()
}

fn is_skip_path(path: &Path, workspace: &Path) -> bool {
    path.strip_prefix(workspace)
        .map(|rel| {
            rel.components().any(|c| {
                let s = c.as_os_str().to_string_lossy();
                s.starts_with('.') || s == "venv" || s == "__pycache__" || s == "node_modules"
            })
        })
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Module path helpers
// ---------------------------------------------------------------------------

/// `parser/shops/amazon/pdp.py` → `"parser.shops.amazon.pdp"`
/// `parser/shops/amazon/__init__.py` → `"parser.shops.amazon"`
fn file_to_module_path(workspace: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(workspace).ok()?;
    let mut parts: Vec<String> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();

    let last = parts.last_mut()?;
    if *last == "__init__.py" {
        parts.pop();
    } else if let Some(stem) = last.strip_suffix(".py") {
        *last = stem.to_string();
    } else {
        return None;
    }

    if parts.is_empty() {
        return None;
    }
    Some(parts.join("."))
}

fn mod_id(mod_path: &str) -> String {
    format!("mod:{}", mod_path.replace('.', "::"))
}

fn build_module_index(workspace: &Path, py_files: &[PathBuf]) -> HashMap<String, PathBuf> {
    py_files
        .iter()
        .filter_map(|p| file_to_module_path(workspace, p).map(|m| (m, p.clone())))
        .collect()
}

// ---------------------------------------------------------------------------
// Package ancestor nodes
// ---------------------------------------------------------------------------

fn add_package_ancestors(mod_path: &str, workspace: &Path, builder: &mut GraphBuilder) {
    let parts: Vec<&str> = mod_path.split('.').collect();
    for i in 1..=parts.len() {
        let pkg_parts = &parts[..i];
        let pkg_dir = workspace.join(pkg_parts.join(std::path::MAIN_SEPARATOR_STR));
        if !pkg_dir.join("__init__.py").exists() {
            continue;
        }

        let id = mod_id(&pkg_parts.join("."));
        let parent_id = (i > 1).then(|| mod_id(&parts[..i - 1].join(".")));

        builder.add_node(Node {
            id: id.clone(),
            kind: NodeKind::Module,
            name: parts[i - 1].to_string(),
            path: pkg_dir.to_string_lossy().into_owned(),
            parent: parent_id.clone(),
            external: Some(false),
            visibility: Some(Visibility::Public),
            loc: None,
            line: None,
            item_count: None,
            method_count: None,
            complexity: None,
            cycle_kind: None,
        });

        if let Some(p) = parent_id {
            builder.add_edge(Edge {
                from: p,
                to: id,
                kind: EdgeKind::Contains,
                unresolved: None,
                external: None,
                visibility: None,
            });
        }
    }
}

// ---------------------------------------------------------------------------
// Per-file parsing
// ---------------------------------------------------------------------------

struct ExtractedFn {
    name: String,
    class_name: Option<String>,
    line: u32,
    end_line: u32,
}

struct ExtractedImport {
    base: String,       // "parser.shops.amazon" or ".." or ".utils"
    names: Vec<String>, // imported names; empty for plain `import X`
}

fn parse_and_add(
    abs_path: &Path,
    mod_path: &str,
    workspace: &Path,
    module_index: &HashMap<String, PathBuf>,
    builder: &mut GraphBuilder,
) -> Result<()> {
    let source = std::fs::read(abs_path)?;

    let mut ts_parser = tree_sitter::Parser::new();
    ts_parser
        .set_language(&tree_sitter_python::LANGUAGE.into())
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    let tree = ts_parser
        .parse(&source, None)
        .ok_or_else(|| anyhow::anyhow!("parse failed: {}", abs_path.display()))?;

    let loc = source.iter().filter(|&&b| b == b'\n').count() as u32 + 1;
    let file_id = format!("file:{}", abs_path.to_string_lossy());

    // Parent package = all parts except the last component
    let parts: Vec<&str> = mod_path.split('.').collect();
    let parent_id = (parts.len() > 1).then(|| mod_id(&parts[..parts.len() - 1].join(".")));

    builder.add_node(Node {
        id: file_id.clone(),
        kind: NodeKind::File,
        name: abs_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
        path: abs_path.to_string_lossy().into_owned(),
        parent: parent_id.clone(),
        external: Some(false),
        visibility: Some(py_visibility(parts[parts.len() - 1])),
        loc: Some(loc),
        line: None,
        item_count: None,
        method_count: None,
        complexity: None,
        cycle_kind: None,
    });

    if let Some(pid) = &parent_id {
        builder.add_edge(Edge {
            from: pid.clone(),
            to: file_id.clone(),
            kind: EdgeKind::Contains,
            unresolved: None,
            external: None,
            visibility: None,
        });
    }

    // Walk tree
    let root = tree.root_node();
    let (fns, imports) = extract_tree_info(&root, &source);

    // Import edges: file → file
    for imp in &imports {
        for target_path in resolve_import(&imp.base, &imp.names, mod_path, module_index) {
            let target_mod = file_to_module_path(workspace, &target_path).unwrap_or_default();
            let is_init = target_path.file_name().is_some_and(|n| n == "__init__.py");
            let target_id = if is_init {
                mod_id(&target_mod)
            } else {
                format!("file:{}", target_path.to_string_lossy())
            };
            if target_id != file_id {
                builder.add_edge(Edge {
                    from: file_id.clone(),
                    to: target_id,
                    kind: EdgeKind::Uses,
                    unresolved: None,
                    external: None,
                    visibility: None,
                });
            }
        }
    }

    // Class nodes (collected from methods)
    let mut seen_classes: HashSet<String> = HashSet::new();
    for f in &fns {
        if let Some(cls) = &f.class_name
            && seen_classes.insert(cls.clone())
        {
            let cls_id = format!("impl:{}::{}", mod_path.replace('.', "::"), cls);
            builder.add_node(Node {
                id: cls_id.clone(),
                kind: NodeKind::Impl,
                name: cls.clone(),
                path: abs_path.to_string_lossy().into_owned(),
                parent: Some(file_id.clone()),
                external: Some(false),
                visibility: Some(py_visibility(cls)),
                loc: None,
                line: None,
                item_count: None,
                method_count: None,
                complexity: None,
                cycle_kind: None,
            });
            builder.add_edge(Edge {
                from: file_id.clone(),
                to: cls_id,
                kind: EdgeKind::Contains,
                unresolved: None,
                external: None,
                visibility: None,
            });
        }
    }

    // Function / method nodes
    for f in &fns {
        let (fn_id, fn_kind, fn_parent) = if let Some(cls) = &f.class_name {
            let cls_id = format!("impl:{}::{}", mod_path.replace('.', "::"), cls);
            (
                format!(
                    "method:{}::{}::{}",
                    mod_path.replace('.', "::"),
                    cls,
                    f.name
                ),
                NodeKind::Method,
                cls_id,
            )
        } else {
            (
                format!("fn:{}::{}", mod_path.replace('.', "::"), f.name),
                NodeKind::Fn,
                file_id.clone(),
            )
        };

        builder.add_node(Node {
            id: fn_id.clone(),
            kind: fn_kind,
            name: f.name.clone(),
            path: abs_path.to_string_lossy().into_owned(),
            parent: Some(fn_parent.clone()),
            external: Some(false),
            visibility: Some(py_visibility(&f.name)),
            loc: Some(f.end_line.saturating_sub(f.line) + 1),
            line: Some(f.line),
            item_count: None,
            method_count: None,
            complexity: None,
            cycle_kind: None,
        });

        builder.add_edge(Edge {
            from: fn_parent,
            to: fn_id,
            kind: EdgeKind::Contains,
            unresolved: None,
            external: None,
            visibility: None,
        });
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Tree-sitter extraction
// ---------------------------------------------------------------------------

fn extract_tree_info(
    root: &tree_sitter::Node,
    source: &[u8],
) -> (Vec<ExtractedFn>, Vec<ExtractedImport>) {
    let mut fns = Vec::new();
    let mut imports = Vec::new();
    visit_node(root, source, None, &mut fns, &mut imports);
    (fns, imports)
}

fn visit_node<'t>(
    node: &tree_sitter::Node<'t>,
    source: &[u8],
    class_ctx: Option<&str>,
    fns: &mut Vec<ExtractedFn>,
    imports: &mut Vec<ExtractedImport>,
) {
    let mut cursor = node.walk();
    let children: Vec<tree_sitter::Node<'t>> = node.children(&mut cursor).collect();

    for child in &children {
        match child.kind() {
            "function_definition" | "async_function_definition" => {
                if let Some(name) = child
                    .child_by_field_name("name")
                    .and_then(|n| n.utf8_text(source).ok())
                {
                    fns.push(ExtractedFn {
                        name: name.to_string(),
                        class_name: class_ctx.map(str::to_string),
                        line: child.start_position().row as u32 + 1,
                        end_line: child.end_position().row as u32 + 1,
                    });
                    // Recurse into function body only for nested class discovery;
                    // nested functions are skipped to keep the graph clean.
                }
            }
            "class_definition" => {
                if let Some(name) = child
                    .child_by_field_name("name")
                    .and_then(|n| n.utf8_text(source).ok())
                {
                    let cls = name.to_string();
                    if let Some(body) = child.child_by_field_name("body") {
                        visit_node(&body, source, Some(&cls), fns, imports);
                    }
                }
            }
            "decorated_definition" => {
                // Unwrap decorator and recurse on the actual def/class
                let mut ic = child.walk();
                let inner: Vec<_> = child.children(&mut ic).collect();
                for n in &inner {
                    if matches!(
                        n.kind(),
                        "function_definition" | "async_function_definition" | "class_definition"
                    ) {
                        visit_node_single(n, source, class_ctx, fns, imports);
                    }
                }
            }
            "import_statement" => {
                // import a.b.c  OR  import a, b
                let mut ic = child.walk();
                for c in child.children(&mut ic) {
                    let actual = if c.kind() == "aliased_import" {
                        c.child_by_field_name("name").unwrap_or(c)
                    } else {
                        c
                    };
                    if actual.kind() == "dotted_name"
                        && let Ok(t) = actual.utf8_text(source)
                    {
                        imports.push(ExtractedImport {
                            base: t.to_string(),
                            names: vec![],
                        });
                    }
                }
            }
            "import_from_statement" => {
                let base = child
                    .child_by_field_name("module_name")
                    .and_then(|n| n.utf8_text(source).ok())
                    .unwrap_or("")
                    .to_string();

                let mut names = Vec::new();
                let mut ic = child.walk();
                for c in child.children(&mut ic) {
                    let actual = if c.kind() == "aliased_import" {
                        c.child_by_field_name("name").unwrap_or(c)
                    } else {
                        c
                    };
                    if actual.kind() == "dotted_name"
                        && actual.start_byte()
                            != child
                                .child_by_field_name("module_name")
                                .map_or(0, |n| n.start_byte())
                        && let Ok(t) = actual.utf8_text(source)
                    {
                        names.push(t.to_string());
                    }
                }

                if !base.is_empty() {
                    imports.push(ExtractedImport { base, names });
                }
            }
            _ => {
                // Recurse at module/class level only
                if class_ctx.is_none() || node.kind() == "block" {
                    visit_node(child, source, class_ctx, fns, imports);
                }
            }
        }
    }
}

fn visit_node_single<'t>(
    node: &tree_sitter::Node<'t>,
    source: &[u8],
    class_ctx: Option<&str>,
    fns: &mut Vec<ExtractedFn>,
    imports: &mut Vec<ExtractedImport>,
) {
    match node.kind() {
        "function_definition" | "async_function_definition" => {
            if let Some(name) = node
                .child_by_field_name("name")
                .and_then(|n| n.utf8_text(source).ok())
            {
                fns.push(ExtractedFn {
                    name: name.to_string(),
                    class_name: class_ctx.map(str::to_string),
                    line: node.start_position().row as u32 + 1,
                    end_line: node.end_position().row as u32 + 1,
                });
            }
        }
        "class_definition" => {
            if let Some(name) = node
                .child_by_field_name("name")
                .and_then(|n| n.utf8_text(source).ok())
            {
                let cls = name.to_string();
                if let Some(body) = node.child_by_field_name("body") {
                    visit_node(&body, source, Some(&cls), fns, imports);
                }
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Import resolution
// ---------------------------------------------------------------------------

/// Resolve one import record to a set of target file paths in this project.
fn resolve_import(
    base: &str,
    names: &[String],
    current_mod: &str,
    index: &HashMap<String, PathBuf>,
) -> Vec<PathBuf> {
    let abs_base = absolute_base(base, current_mod);
    let mut results: Vec<PathBuf> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();

    let mut try_add = |mod_path: &str| {
        if let Some(p) = index.get(mod_path)
            && seen.insert(p.clone())
        {
            results.push(p.clone());
        }
    };

    if names.is_empty() {
        // plain `import X.Y.Z`
        try_add(&abs_base);
    } else {
        for name in names {
            let full = if abs_base.is_empty() {
                name.clone()
            } else {
                format!("{abs_base}.{name}")
            };
            try_add(&full);
        }
        // Also add the base itself (might import symbols from it)
        if !abs_base.is_empty() {
            try_add(&abs_base);
        }
    }

    results
}

/// Turn a possibly-relative base like `"."`, `".utils"`, `"..shops"` into
/// an absolute dotted module path using `current_mod` as the anchor.
fn absolute_base(base: &str, current_mod: &str) -> String {
    if !base.starts_with('.') {
        return base.to_string();
    }

    let dots = base.chars().take_while(|&c| c == '.').count();
    let suffix = base[dots..].to_string(); // part after dots (may be empty)

    // current_mod = "parser.shops.amazon.pdp"
    // parts = ["parser", "shops", "amazon", "pdp"]
    // 1 dot  → drop last 1 → ["parser", "shops", "amazon"] → pkg = "parser.shops.amazon"
    // 2 dots → drop last 2 → ["parser", "shops"]            → pkg = "parser.shops"
    let parts: Vec<&str> = current_mod.split('.').collect();
    let keep = parts.len().saturating_sub(dots);
    let pkg = parts[..keep].join(".");

    if suffix.is_empty() {
        pkg
    } else if pkg.is_empty() {
        suffix
    } else {
        format!("{pkg}.{suffix}")
    }
}

// ---------------------------------------------------------------------------
// Heuristic sema: call graph via tree-sitter
// ---------------------------------------------------------------------------

fn build_fn_name_index(builder: &GraphBuilder) -> HashMap<String, Vec<String>> {
    let mut index: HashMap<String, Vec<String>> = HashMap::new();
    for node in builder.nodes() {
        if matches!(node.kind, NodeKind::Fn | NodeKind::Method) {
            index
                .entry(node.name.clone())
                .or_default()
                .push(node.id.clone());
        }
    }
    index
}

fn extract_calls_py(
    abs_path: &Path,
    mod_path: &str,
    name_index: &HashMap<String, Vec<String>>,
) -> Result<Vec<(String, String)>> {
    let source = std::fs::read(abs_path)?;
    let mut ts_parser = tree_sitter::Parser::new();
    ts_parser
        .set_language(&tree_sitter_python::LANGUAGE.into())
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let tree = ts_parser
        .parse(&source, None)
        .ok_or_else(|| anyhow::anyhow!("parse failed"))?;
    let mut calls: HashSet<(String, String)> = HashSet::new();
    visit_calls_py(
        &tree.root_node(),
        &source,
        mod_path,
        None,
        None,
        name_index,
        &mut calls,
    );
    Ok(calls.into_iter().collect())
}

fn visit_calls_py<'t>(
    node: &tree_sitter::Node<'t>,
    source: &[u8],
    mod_path: &str,
    class_ctx: Option<&str>,
    current_fn_id: Option<&str>,
    name_index: &HashMap<String, Vec<String>>,
    calls: &mut HashSet<(String, String)>,
) {
    match node.kind() {
        "function_definition" | "async_function_definition" => {
            if let Some(name) = node
                .child_by_field_name("name")
                .and_then(|n| n.utf8_text(source).ok())
            {
                let fn_id = if let Some(cls) = class_ctx {
                    format!("method:{}::{}::{}", mod_path.replace('.', "::"), cls, name)
                } else {
                    format!("fn:{}::{}", mod_path.replace('.', "::"), name)
                };
                if let Some(body) = node.child_by_field_name("body") {
                    visit_calls_py(
                        &body,
                        source,
                        mod_path,
                        class_ctx,
                        Some(&fn_id),
                        name_index,
                        calls,
                    );
                }
            }
        }
        "class_definition" => {
            if let Some(name) = node
                .child_by_field_name("name")
                .and_then(|n| n.utf8_text(source).ok())
                && let Some(body) = node.child_by_field_name("body")
            {
                visit_calls_py(&body, source, mod_path, Some(name), None, name_index, calls);
            }
        }
        "decorated_definition" => {
            let mut c = node.walk();
            for child in node.children(&mut c).collect::<Vec<_>>() {
                if matches!(
                    child.kind(),
                    "function_definition" | "async_function_definition" | "class_definition"
                ) {
                    visit_calls_py(
                        &child,
                        source,
                        mod_path,
                        class_ctx,
                        current_fn_id,
                        name_index,
                        calls,
                    );
                }
            }
        }
        "call" => {
            if let Some(from_id) = current_fn_id
                && let Some(fn_node) = node.child_by_field_name("function")
            {
                let callee = match fn_node.kind() {
                    "identifier" => fn_node.utf8_text(source).ok().map(str::to_string),
                    "attribute" => fn_node
                        .child_by_field_name("attribute")
                        .and_then(|a| a.utf8_text(source).ok())
                        .map(str::to_string),
                    _ => None,
                };
                if let Some(callee) = callee {
                    for to_id in name_index.get(&callee).into_iter().flatten() {
                        if to_id.as_str() != from_id {
                            calls.insert((from_id.to_string(), to_id.clone()));
                        }
                    }
                }
            }
            // Recurse into call arguments to catch nested calls
            let mut c = node.walk();
            for child in node.children(&mut c).collect::<Vec<_>>() {
                visit_calls_py(
                    &child,
                    source,
                    mod_path,
                    class_ctx,
                    current_fn_id,
                    name_index,
                    calls,
                );
            }
        }
        _ => {
            let mut c = node.walk();
            for child in node.children(&mut c).collect::<Vec<_>>() {
                visit_calls_py(
                    &child,
                    source,
                    mod_path,
                    class_ctx,
                    current_fn_id,
                    name_index,
                    calls,
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Visibility heuristic
// ---------------------------------------------------------------------------

fn py_visibility(name: &str) -> Visibility {
    if name.starts_with("__") && !name.ends_with("__") {
        Visibility::Private
    } else if name.starts_with('_') {
        Visibility::Restricted {
            path: "module".into(),
        }
    } else {
        Visibility::Public
    }
}
