use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;
use code_split_core::{Complexity, GraphBuilder, Halstead, Loc, Maintainability, NodeKind};
use rust_code_analysis::{
    FuncSpace, JavascriptParser, ParserTrait, PythonParser, RustParser, SpaceKind, TsxParser,
    TypescriptParser, metrics,
};
use walkdir::WalkDir;

/// Walk all source files under `root` whose extension is in `extensions`,
/// compute complexity metrics via rust-code-analysis, and annotate
/// `Fn`, `Method`, and `File` nodes in the graph.
/// Returns the number of nodes annotated.
pub fn analyze(root: &Path, builder: &mut GraphBuilder) -> Result<usize> {
    analyze_extensions(root, builder, &["rs"])
}

/// Same as `analyze` but for Python source files.
pub fn analyze_python(root: &Path, builder: &mut GraphBuilder) -> Result<usize> {
    analyze_extensions(root, builder, &["py"])
}

/// Same as `analyze` but for JavaScript / TypeScript source files.
pub fn analyze_js(root: &Path, builder: &mut GraphBuilder) -> Result<usize> {
    analyze_extensions(root, builder, &["js", "jsx", "ts", "tsx"])
}

fn analyze_extensions(
    root: &Path,
    builder: &mut GraphBuilder,
    extensions: &[&str],
) -> Result<usize> {
    let mut file_index: HashMap<String, usize> = HashMap::new();
    let mut fn_index: HashMap<(String, String), usize> = HashMap::new();
    // Fallback: match by (file_path, start_line) for languages where names are "<anonymous>"
    let mut fn_line_index: HashMap<(String, usize), usize> = HashMap::new();

    for (i, node) in builder.nodes().iter().enumerate() {
        match node.kind {
            // Module nodes that represent a file (line == None) share the same
            // canonical path as the file itself; inline modules (line.is_some()) share
            // the enclosing file's path and must not receive file-level metrics.
            NodeKind::File => {
                file_index.insert(node.path.clone(), i);
            }
            NodeKind::Module if node.line.is_none() => {
                file_index.entry(node.path.clone()).or_insert(i);
            }
            NodeKind::Fn | NodeKind::Method => {
                fn_index.insert((node.path.clone(), node.name.clone()), i);
                if let Some(line) = node.line {
                    fn_line_index.insert((node.path.clone(), line as usize), i);
                }
            }
            _ => {}
        }
    }

    let mut annotated = 0usize;

    for entry in WalkDir::new(root)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_type().is_file()
                && e.path()
                    .extension()
                    .and_then(|x| x.to_str())
                    .is_some_and(|x| extensions.contains(&x))
        })
    {
        let path = entry.path();
        let Ok(src) = std::fs::read(path) else {
            continue;
        };
        let canonical = path.to_string_lossy().into_owned();

        let Some(space) = parse_metrics(path, src) else {
            continue;
        };

        if let Some(&idx) = file_index.get(&canonical) {
            builder.nodes_mut()[idx].complexity = Some(complexity_from(&space));
            annotated += 1;
        }
        annotated += collect_fns(&space, &canonical, builder, &fn_index, &fn_line_index);
    }

    Ok(annotated)
}

fn parse_metrics(path: &Path, src: Vec<u8>) -> Option<FuncSpace> {
    match path.extension().and_then(|e| e.to_str()) {
        Some("rs") => metrics(&RustParser::new(src, path, None), path),
        Some("py") => metrics(&PythonParser::new(src, path, None), path),
        Some("js") | Some("jsx") => metrics(&JavascriptParser::new(src, path, None), path),
        Some("ts") => metrics(&TypescriptParser::new(src, path, None), path),
        Some("tsx") => metrics(&TsxParser::new(src, path, None), path),
        _ => None,
    }
}

fn collect_fns(
    space: &FuncSpace,
    file: &str,
    builder: &mut GraphBuilder,
    fn_index: &HashMap<(String, String), usize>,
    fn_line_index: &HashMap<(String, usize), usize>,
) -> usize {
    let mut count = 0;
    if matches!(space.kind, SpaceKind::Function) {
        let name = space.name.as_deref().unwrap_or("?");
        let bare = name.split("::").last().unwrap_or(name);

        let idx = fn_line_index
            .get(&(file.to_owned(), space.start_line))
            .copied()
            .or_else(|| fn_index.get(&(file.to_owned(), bare.to_owned())).copied());

        if let Some(idx) = idx {
            builder.nodes_mut()[idx].complexity = Some(complexity_from(space));
            count += 1;
        }
    }
    for child in &space.spaces {
        count += collect_fns(child, file, builder, fn_index, fn_line_index);
    }
    count
}

fn complexity_from(s: &FuncSpace) -> Complexity {
    let m = &s.metrics;
    let sloc = m.loc.sloc();
    let vol = m.halstead.volume();

    Complexity {
        cyclomatic: m.cyclomatic.cyclomatic(),
        cognitive: m.cognitive.cognitive(),
        exits: m.nexits.exit(),
        // fn_args > 0 → args = fn_args; otherwise use closure_args
        args: if m.nargs.fn_args() > 0.0 {
            m.nargs.fn_args()
        } else {
            m.nargs.closure_args()
        },
        functions: m.nom.functions(),
        closures: m.nom.closures(),
        coupling: None, // filled later in annotate_hk
        maintainability: Some(Maintainability {
            mi: m.mi.mi_original(),
            mi_sei: m.mi.mi_sei(),
        }),
        loc: if sloc > 0.0 {
            Some(Loc {
                source: sloc,
                logical: m.loc.lloc(),
                comments: m.loc.cloc(),
                blank: m.loc.blank(),
            })
        } else {
            None
        },
        halstead: if vol > 0.0 {
            Some(Halstead {
                length: m.halstead.length(),
                vocabulary: (m.halstead.u_operators() + m.halstead.u_operands()),
                volume: vol,
                effort: m.halstead.effort(),
                time: m.halstead.time(),
                bugs: m.halstead.bugs(),
            })
        } else {
            None
        },
    }
}
