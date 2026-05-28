# PRD — Code Split

<!-- toc -->

- [1. Overview](#1-overview)
  - [1.1 Purpose](#11-purpose)
  - [1.2 Background / Problem Statement](#12-background--problem-statement)
  - [1.3 Goals (Business Outcomes)](#13-goals-business-outcomes)
  - [1.4 Glossary](#14-glossary)
- [2. Actors](#2-actors)
  - [2.1 Human Actors](#21-human-actors)
  - [2.2 System Actors](#22-system-actors)
- [3. Operational Concept & Workflow](#3-operational-concept--workflow)
- [4. Scope](#4-scope)
  - [4.1 Priority Tiers](#41-priority-tiers)
  - [4.2 Out of Scope (All Versions)](#42-out-of-scope-all-versions)
- [5. Functional Requirements](#5-functional-requirements)
  - [5.1 Plugin System — Step 1](#51-plugin-system--step-1)
  - [5.2 Visualization Reports — Step 2](#52-visualization-reports--step-2)
  - [5.3 Diff Analysis — Step 4](#53-diff-analysis--step-4)
- [6. Non-Functional Requirements](#6-non-functional-requirements)
  - [6.1 NFR Inclusions](#61-nfr-inclusions)
  - [6.2 NFR Exclusions](#62-nfr-exclusions)
- [7. Public Interfaces](#7-public-interfaces)
  - [7.1 Code Split Unified CLI](#71-code-split-unified-cli)
  - [7.2 Plugin Binary Contract](#72-plugin-binary-contract)
  - [7.3 Graph JSON Schema](#73-graph-json-schema)
- [8. Use Cases](#8-use-cases)
  - [UC-001 Analyze Rust Workspace Offline](#uc-001-analyze-rust-workspace-offline)
  - [UC-002 Before/After Refactoring Diff](#uc-002-beforeafter-refactoring-diff)
  - [UC-003 CI Structural Diff on Pull Request](#uc-003-ci-structural-diff-on-pull-request)
- [9. Acceptance Criteria](#9-acceptance-criteria)
- [10. Dependencies](#10-dependencies)
- [11. Assumptions](#11-assumptions)
- [12. Risks](#12-risks)

<!-- /toc -->

## 1. Overview

### 1.1 Purpose

Code Split is a polyglot structural-analysis platform that (1) extracts
dependency graphs from local codebases at module, file, and function
granularity via a pluggable analyzer system, (2) visualizes the
resulting graphs as interactive offline HTML reports with coupling
metrics, and (3) tracks and reports architectural drift between two
captured snapshots.

### 1.2 Background / Problem Statement

Developers working on large or aging codebases face two recurring
problems: they cannot see the full picture of structural coupling in a
machine-readable form, and they cannot measure whether a refactoring
actually improved that coupling. Existing tools are fragmented,
language-specific, non-exportable, or single-level.

**Target Users**:

- Developers working on local projects or monorepos (Rust at P1;
  Python, Go, JavaScript and others at P3)
- Tech leads and architects planning or validating refactors
- CI pipelines enforcing structural policies across pull requests

**Key Problems Solved**:

- No unified multi-level dependency graph across languages in a
  portable artifact format
- No before/after coupling comparison that quantifies whether a
  refactoring improved the architecture
- Refactoring decisions rely on intuition rather than measurable data

### 1.3 Goals (Business Outcomes)

**Success Criteria**:

- Extract module/file/function graphs for a 50k-LOC Rust workspace
  in under 30 seconds
- Generate an HTML visualization report from JSON artifacts in under
  5 seconds
- Generate a diff report between two snapshots in under 5 seconds
- Works fully offline — no network access, no LLM calls required

**Capabilities**:

- Pluggable analyzer system: each language/framework provides a CLI
  plugin that emits standard JSON artifacts
- Multi-level graph visualization with coupling metrics and node sorting
- Snapshot diff for before/after refactoring quantification

### 1.4 Glossary

| Term | Definition |
|------|------------|
| Plugin | A CLI program that accepts a workspace path and writes three JSON graph files (module, file, function level) to an output directory |
| Snapshot | A directory containing three JSON graph files produced by a single plugin run |
| Graph | A directed graph whose nodes are code entities and whose edges are structural relationships (`Contains`, `Uses`, `Calls`) |
| Level | One of `module`, `file`, `fn` — the granularity of a graph |
| Node weight | The coupling metric for a node: sum of its incoming and outgoing edge counts |
| Diff | A structured comparison of two snapshots: nodes and edges added, removed, or with changed weight |
| Coupling direction | The overall verdict of a diff: `improved`, `degraded`, or `neutral` |

## 2. Actors

### 2.1 Human Actors

#### Developer

**ID**: `cpt-code-split-actor-developer`

**Role**: Runs the plugin on a local workspace, views the HTML report,
modifies the codebase, then compares before/after snapshots.

**Needs**: Fast offline-capable tools with no mandatory LLM or network
dependency; single-command invocation per step.

#### Tech Lead

**ID**: `cpt-code-split-actor-tech-lead`

**Role**: Reviews HTML reports to evaluate module boundaries and
coupling hotspots; reviews diff reports to validate that refactors
improved the architecture.

**Needs**: Sortable coupling view; clear before/after delta with
magnitude; self-contained HTML that can be shared without tooling.

### 2.2 System Actors

#### CI Pipeline

**ID**: `cpt-code-split-actor-ci`

**Role**: Runs the plugin at pull-request time, stores snapshot
artifacts, runs the diff engine against the base-branch snapshot, and
attaches the diff report to the pull request.

**Needs**: Non-interactive execution; deterministic artifact output;
structured exit codes.

#### PR Reviewer

**ID**: `cpt-code-split-actor-pr-reviewer`

**Role**: Views the diff HTML report attached to a pull request to
evaluate architectural impact without a local toolchain.

**Needs**: Self-contained HTML; clear color-coded coupling changes;
summary verdict readable in under one minute.

## 3. Operational Concept & Workflow

The platform is organized as four sequential steps. Steps 1, 2, and 4
are implemented by Code Split; Step 3 is the user's own modification
activity and is deliberately outside Code Split's scope.

```
Step 1 ─ Extract   →   Step 2 ─ Visualize   →   Step 3 ─ Modify   →   Step 4 ─ Diff
(code-split analyze)       (code-split report)          (User / AI)           (code-split diff)
outputs JSON            outputs HTML              (we wait)             outputs HTML + MD
```

**Step 1 — Graph Extraction (Plugin)**: A language-specific plugin CLI
is invoked on the workspace. It writes three JSON artifact files:
`modules.json`, `files.json`, `functions.json`. No network access or
LLM is required. Artifacts may be stored as CI artifacts for Step 4.

**Step 2 — Visualization (Report Generator)**: The `code-split report`
subcommand reads the snapshot JSON and produces a self-contained offline
HTML report with interactive graph visualization and sorting by node
weight. No network access or LLM is required.

**Step 3 — Modification (User Activity)**: The user reads the report,
decides what to refactor (manually or with AI assistance), and modifies
the codebase. Code Split does not participate in this step.

**Step 4 — Diff Analysis (Diff Engine)**: After modification, the user
re-runs Step 1 to produce a new snapshot. The `code-split diff` subcommand
compares the two snapshots and produces a diff HTML report and a
Markdown text report. No network access or LLM is required.

## 4. Scope

### 4.1 Priority Tiers

#### P1 — Required for Initial Release

| Step | Scope |
|------|-------|
| Step 1 | Rust plugin only; module-, file-, and function-level JSON graphs; no AI prompts; no CI integration |
| Step 2 | Offline HTML report with graph visualization and node sorting by weight |
| Step 4 | Offline HTML diff report and Markdown text report comparing two snapshots |

#### P2 — Follow-On

| Step | Scope |
|------|-------|
| Step 1 | AI prompt generator (heaviest nodes → LLM prompt); CI artifact integration |
| Step 2 | CI artifact hosting |
| Step 4 | CI integration; diff artifacts for PR review automation |
| Distribution | Multi-ecosystem binary distribution: single pre-compiled `code-split` binary per platform published via thin wrappers to PyPI (`pip install code-split`), npm (`npm install -g @code-split/cli`), and GitHub Releases |

#### P3 — Future

| Step | Scope |
|------|-------|
| Step 1 | Additional language plugins: Python, JavaScript, Go, C#, PHP; framework-specific plugins (Django, WordPress, etc.) with domain-specific node kinds |
| Step 2 | AI prompt generation for principles review using the `principles/` corpus (per-language: `principles/rust/`, `principles/python/`, `principles/typescript/`) |

### 4.2 Out of Scope (All Versions)

- External dependency analysis (registry/git/npm/pypi packages are
  opaque leaf nodes; their internals are never expanded)
- Dynamic dispatch resolution beyond what the language-specific
  resolver reports
- Automated code modification or refactoring suggestions
- IDE/LSP integration and interactive visualization
- Cross-language call graph (FFI/RPC boundaries are leaves)
- Database or service deployment; no server component

## 5. Functional Requirements

### 5.1 Plugin System — Step 1

#### Unified Entry-Point Command

- [x] `p1` - **ID**: `cpt-code-split-fr-unified-cli`

All user-facing operations MUST be accessible through a single binary
`code-split`. The three top-level subcommands map to the workflow steps:

```
code-split analyze  <workspace> --plugin <name|path> [--output <file>] [options] [-- <plugin-args>]
code-split report   --input <snapshot.json> --output <report.html>
code-split diff     --before <snap-a.json> --after <snap-b.json> [--output <diff.html>]
code-split compare  --before <snap-a.json> --after <snap-b.json> [--output <summary.json>] [--html]
```

`--output` is optional on `analyze`. When omitted, `code-split` saves the
snapshot as `.code-split/{project-name}-<YYYYMMDD-HHmmss>.json` in the
**current working directory** (not inside the analyzed project). The
timestamp and project slug are in the filename; no additional registry is
created.

Each snapshot is a **single self-contained `.json` file** combining
metadata (command, versions, git state) and all three graphs. See
`cpt-code-split-fr-snapshot-meta` for the full schema.

`report` and `diff` consume snapshot files produced by `analyze` and
are plugin-agnostic. Splitting into separate binaries is forbidden at
P1; the separation of concerns lives inside the binary.

**Rationale**: One file per snapshot is easier to copy, archive, attach
to CI artifacts, and pass to `diff`. A timestamped filename means users
never have to think about naming for routine snapshots; explicit
`--output` is available for named states (e.g., `snap-before-refactor.json`).

**Actors**: `cpt-code-split-actor-developer`, `cpt-code-split-actor-ci`

#### Snapshot File Format

- [x] `p1` - **ID**: `cpt-code-split-fr-snapshot-meta`

Each `code-split analyze` run produces a single `.json` file. The file
combines metadata and all three graphs in one document:

```json
{
  "schema_version": "1",
  "generated_at": "2026-05-22T11:22:33Z",
  "command": "code-split analyze /path/to/axum-api --plugin rust",
  "workspace": "/Users/alice/projects/code-split",
  "target":    "/Users/alice/projects/axum-api",
  "plugin": "rust",
  "config_file": "/Users/alice/projects/axum-api/code-split.toml",
  "local_only": false,
  "versions": {
    "code-split": "0.3.1",
    "plugin_rust": "0.3.1",
    "rustc": "1.78.0"
  },
  "roots": {
    "cargo":    "/Users/alice/.cargo",
    "registry": "/Users/alice/.cargo/registry/src/index.crates.io-abc123",
    "rustup":   "/Users/alice/.rustup",
    "rust-src": "/Users/alice/.rustup/toolchains/stable-aarch64-apple-darwin/lib/rustlib/src/rust/library"
  },
  "git": {
    "branch": "refactor/split-handlers",
    "commit": "a3f9c21",
    "dirty_files": 4
  },
  "timings": [
    { "stage": "syn",        "ms": 600,  "detail": "547 nodes" },
    { "stage": "sema",       "ms": 10500,"detail": "389 call edges" },
    { "stage": "complexity", "ms": 700,  "detail": "147 nodes annotated" },
    { "stage": "projection", "ms": 0,    "detail": "modules=508 files=547 functions=810" },
    { "stage": "write",      "ms": 20,   "detail": "/path/to/snap.json" }
  ],
  "graphs": {
    "modules":   { "nodes": [...], "edges": [...], "stats": { ... } },
    "files":     { "nodes": [...], "edges": [...], "stats": { ... } },
    "functions": { "nodes": [...], "edges": [...], "stats": { ... } }
  }
}
```

Top-level fields:

- `schema_version` — version of the snapshot file format
- `generated_at` — ISO-8601 timestamp
- `command` — full command line as typed
- `workspace` — absolute path to the directory where `code-split` was invoked
- `target` — absolute path to the analyzed project
- `plugin` — resolved plugin name or path
- `config_file` — absolute path of the config file used (`code-split.toml` or `Cargo.toml#metadata.code-split`); omitted when no config file was found
- `local_only` — boolean
- `versions` — `code-split` semver at minimum; the Rust plugin adds
  `plugin_rust` and `rustc`; external plugins add `plugin_<name>`
  from their `--version` output
- `roots` — named system prefixes used to relativize node paths
  (e.g. `{cargo}`, `{registry}`, `{rustup}`, `{rust-src}`); resolve formula:
  `roots[name] + "/" + rest` gives the absolute path. The Rust plugin
  auto-detects `rust-src` from `rustc --print sysroot` to shorten stdlib
  paths (e.g. `{rust-src}/alloc/src/vec/mod.rs`)
- `git` — `branch`, `commit` (short SHA), `dirty_files` (count from
  `git status --porcelain`); omitted entirely if not a git repository
- `timings` — per-stage wall-clock timings in milliseconds, in execution
  order; each entry has `stage` (name), `ms` (elapsed), `detail` (human
  summary); omitted when empty (e.g. external plugins)
- `graphs` — object with three keys: `modules`, `files`, `functions`;
  each value is a graph object with `nodes` and `edges` arrays

`code-split report` and `code-split diff` read the snapshot file and embed
the top-level metadata in the generated HTML as a "Snapshot info" panel.

**Rationale**: One file per snapshot is simpler to copy, archive, and
pass between tools than a directory of four files. The timestamp in the
filename makes snapshots self-organizing without a registry.

**Actors**: `cpt-code-split-actor-developer`, `cpt-code-split-actor-tech-lead`,
`cpt-code-split-actor-ci`

#### Plugin Contract

- [x] `p1` - **ID**: `cpt-code-split-fr-plugin-contract`

A plugin is a CLI program that `code-split analyze` discovers and
sub-processes. The plugin binary receives a fixed invocation contract:

```
<plugin-binary> <workspace-path> --output <file> [-- <plugin-args>]
```

It MUST write a single JSON file to `<file>` containing only the
`graphs` object:

```json
{
  "graphs": {
    "modules":   { "nodes": [...], "edges": [...] },
    "files":     { "nodes": [...], "edges": [...] },
    "functions": { "nodes": [...], "edges": [...] }
  }
}
```

`files` MAY be omitted when the plugin produces no `NodeKind::File`
nodes (e.g. Rust plugin). `code-split` wraps this output with the top-level metadata fields
(`command`, `versions`, `git`, etc.) and writes the final snapshot
file. The plugin MUST exit zero on success and non-zero with a
structured JSON error on stderr on failure. No network access is
permitted during plugin execution. Arguments after `--` are forwarded
verbatim from `code-split analyze` to the plugin binary.

**Rationale**: A standard binary contract decouples plugin
implementations from consumer tools. Any executable that speaks this
contract is a valid plugin, regardless of language.

**Actors**: `cpt-code-split-actor-developer`, `cpt-code-split-actor-ci`

#### Plugin Discovery and Registration

- [x] `p1` - **ID**: `cpt-code-split-fr-plugin-discovery`

`code-split analyze --plugin <value>` MUST resolve the plugin binary
through the following ordered lookup, stopping at the first match:

1. **Path** — `<value>` starts with `./`, `../`, or `/`: execute
   `<value>` directly as a file-system path.
2. **Config** — `<value>` matches a name in `code-split.toml`
   `[plugins.<value>]`: use the `command` key from that section.
3. **PATH** — `code-split-plugin-<value>` is found on `$PATH`: execute it.
4. **Built-in** — `<value>` matches a built-in plugin compiled into
   the `code-split` binary (P1: `rust`; P3: `python`, `go`, `js`, `ts`).

If no match is found, `code-split analyze` MUST exit non-zero with a
human-readable error listing the four lookup steps and what was tried.

Third-party plugins are registered in `code-split.toml` at the workspace
root:

```toml
[plugins.django]
command = "code-split-plugin-django"

[plugins.custom]
command = "./scripts/my-analyzer.sh"
```

**Rationale**: The four-step lookup supports all use cases — built-in
convenience, project-scoped custom scripts, globally installed
community plugins, and path-based ad-hoc plugins — without requiring
a plugin registry or install step.

**Actors**: `cpt-code-split-actor-developer`, `cpt-code-split-actor-ci`

#### Rust Plugin

- [x] `p1` - **ID**: `cpt-code-split-fr-rust-plugin`

The platform MUST ship a Rust plugin (`code-split-rust`) that implements
`cpt-code-split-fr-plugin-contract` for Cargo workspaces. The plugin MUST:

- Derive the module graph from `cargo metadata` and `mod` declarations
  via syntactic analysis (`syn` crate); in Rust a `.rs` file IS its
  module, so no separate `File` nodes are emitted — `loc` and
  `item_count` live on the `Module` node
- Emit an empty `files` graph (no `NodeKind::File` nodes are produced
  for Rust — a `.rs` file IS its module; file-level graphs are the
  responsibility of Python/JS/TS plugins); when empty, `files` is
  omitted from the serialized snapshot
- Derive the function graph using rust-analyzer (`ra_ap_*` crates) for
  resolved `Calls` edges; unresolved call sites MUST be omitted or
  marked `unresolved = true`; no syntactically guessed `Calls` edges
- Classify each crate as local vs. external; external crates appear as
  opaque leaf nodes with `external = true` and are never expanded
- Compute code complexity metrics (cyclomatic, cognitive, Halstead,
  maintainability index, LOC variants, NOM, nexits, nargs) for each
  file-backed `Module`, `Fn`, and `Method` node via `code-split-complexity`;
  metrics are stored in the `complexity` field of the node and
  serialized into the snapshot
- Detect dependency cycles (Kosaraju SCC) across all three graph
  levels; annotate each node in a cycle with `cycle_kind`
  (`TestEmbed` | `Mutual` | `Chain`) and store `CycleGroup` entries
  in `Graph.cycles`
- Compute Henry-Kafura complexity (`HK = LOC × (fan_in × fan_out)²`)
  for every node; store in `complexity.coupling` (`fan_in`, `fan_out`,
  `hk`); excludes `Contains` edges from the counts

**Rationale**: Rust is the primary use-case for the initial release.
The existing `code-split-syn` and `code-split-sema` analysis components
become the implementation of this plugin.

**Actors**: `cpt-code-split-actor-developer`

#### Module-Level Graph

- [x] `p1` - **ID**: `cpt-code-split-fr-module-graph`

The Rust plugin MUST emit a directed graph of modules for each local
crate. Nodes are module units (including folder-backed `mod.rs` /
`lib.rs` hierarchy). Edges are `Contains` (parent → child module) and
`Uses` (module → imported module, derived from `use` statements).

**Rationale**: Module structure is the level where most refactoring
decisions are made; it maps directly to package-layout discussions.

**Actors**: `cpt-code-split-actor-developer`, `cpt-code-split-actor-tech-lead`

#### File-Level Graph

- [x] `p1` - **ID**: `cpt-code-split-fr-file-graph`

The Rust plugin produces no `NodeKind::File` nodes — in Rust a `.rs`
file IS its module, so `loc` and `item_count` live on the `Module`
node. The `files` projection therefore yields an empty graph, which is
omitted from the snapshot JSON (`skip_serializing_if = "Graph::is_empty"`).

File-level graphs (`NodeKind::File` nodes, `Contains`/`Uses` edges per
file) are the responsibility of Python, JavaScript, and TypeScript
plugins where source files and logical modules are distinct entities.

**Rationale**: An empty files graph for Rust adds no information and
wastes snapshot space. Python/JS/TS plugins emit real file nodes with
meaningful dependency edges, so the `files` key is preserved in the
schema for those plugins.

**Actors**: `cpt-code-split-actor-developer`, `cpt-code-split-actor-tech-lead`

#### Function-Level Graph

- [x] `p1` - **ID**: `cpt-code-split-fr-fn-graph`

The Rust plugin MUST emit a directed graph of functions and methods.
Nodes are `Fn` and `Method` items. Edges are `Calls` relationships
resolved by rust-analyzer (`ra_ap_*`). The plugin MUST NOT emit `Calls`
edges based on syntactic pattern matching; every `Calls` edge MUST be
semantically resolved.

**Rationale**: An honest call graph is the differentiator over
syntactic tools. False-positive edges produce misleading coupling
metrics.

**Actors**: `cpt-code-split-actor-developer`, `cpt-code-split-actor-tech-lead`

#### Local-Only Mode

- [x] `p1` - **ID**: `cpt-code-split-fr-local-only`

The Rust plugin MUST support a `--local-only` flag. When set, it MUST
pass `--no-deps` to `cargo metadata` (external crates are not
enumerated) and skip the rust-analyzer stage entirely (producing only
module and file graphs; the `functions.json` artifact is written as an
empty graph). This mode works even when external dependencies are
unreachable or uncached.

**Rationale**: Workspaces with private or unreachable git dependencies
cannot be analyzed in full mode because `cargo metadata` fails. Local-
only still produces the full module/file graph, which is sufficient for
most structural coupling analysis.

**Actors**: `cpt-code-split-actor-developer`, `cpt-code-split-actor-ci`

#### Embedded Static Asset Tracking (P2)

- [ ] `p2` - **ID**: `cpt-code-split-fr-rust-embedded-assets`

The Rust plugin SHOULD track files embedded into the binary via macros
(`include_bytes!`, `include_str!`, `include!`, `sqlx::query_file!`, etc.)
as `File` nodes in the graph, with a dedicated `Embeds` edge kind from
the referencing module to the embedded file.

Currently these dependencies are completely invisible: a module that
embeds a TLS certificate or a SQL migration file shows no outgoing edges
to those assets, making the structural graph incomplete.

**Implementation**: In `walk_items`, detect `Item::Macro` nodes whose
path matches known embedding macros, parse the string literal argument
as a relative path, resolve it against the enclosing file's directory,
and emit a `File` node + `Embeds` edge.

**Rationale**: Embedded assets are real compile-time dependencies. SQL
files, certificates, HTML templates, and proto-generated sources that
are `include!`-ed affect correctness and security but are invisible to
structural analysis today.

**Actors**: `cpt-code-split-actor-developer`, `cpt-code-split-actor-tech-lead`

#### Language Plugins (P3)

- [x] `p3` (Python shipped) - **ID**: `cpt-code-split-fr-lang-plugins`

The platform SHOULD support additional language plugins for Python, Go,
JavaScript, C#, and PHP, each implementing `cpt-code-split-fr-plugin-contract`
and emitting conformant JSON artifacts. Framework-specific plugins
(Django, WordPress, etc.) MAY extend the node and edge kind vocabulary
with domain-specific types; extensions MUST be backward-compatible with
the base schema.

**Python plugin** (`--plugin python`) is shipped as a built-in in
`code-split-cli`. It uses `tree-sitter-python` to extract module/package
structure, classes, functions, methods, and import-based `Uses` edges.
Emits `Module`, `File`, `Impl`, `Fn`, `Method` nodes. Complexity metrics
(cyclomatic, cognitive, Halstead, MI, LOC, NOM, nexits, nargs) are
annotated on `Fn`, `Method`, and `File` nodes via `code-split-complexity`
using `rust-code-analysis`'s `PythonParser`. Function matching uses
line-based fallback (by `start_line`) because `rust-code-analysis` reports
Python functions as `<anonymous>` in `FuncSpace.name`. A heuristic call
graph is built via a second tree-sitter AST pass: after all files are
parsed, callee names extracted from `call` nodes (plain `identifier` or
`attribute` access) are matched against the global `Fn`/`Method` name
index; matching pairs emit `Calls` edges.

**JavaScript / TypeScript plugin** (`--plugin js`, `--plugin ts`,
`--plugin javascript`, `--plugin typescript`) is shipped as a built-in in
`code-split-cli`. It uses `tree-sitter-javascript` and
`tree-sitter-typescript` to extract module/package structure, classes,
functions, methods, and import-based `Uses` edges. Supports both ES
modules (`import`) and CommonJS (`require()`). Emits `Module`, `File`,
`Impl`, `Fn`, `Method` nodes. Complexity metrics are annotated for regular
`function` declarations; arrow functions and class method shorthand are not
matched by `rust-code-analysis` (best-effort annotation). A heuristic
call graph is built via a second tree-sitter AST pass: callee names from
`call_expression` nodes (plain `identifier` or `member_expression`
property) are matched against the global `Fn`/`Method` name index;
matching pairs emit `Calls` edges. Named arrow functions assigned to
`const`/`let` declarators are resolved to their declared name and treated
as first-class function nodes for both traversal and callee lookup.

Go, C#, PHP plugins remain future work (P3 deferred).

**Rationale**: The JSON contract and consumer tools are language-agnostic;
adding a new language plugin does not require changes to the report or
diff layer.

**Actors**: `cpt-code-split-actor-developer`

#### Configuration System

- [x] `p1` - **ID**: `cpt-code-split-fr-config`

`code-split analyze` MUST load a layered configuration from multiple sources.
Priority order (highest wins for scalars; `ignore.paths` is merged):

| Priority | Source |
|---|---|
| 1 | CLI flags (`--ignore`, `--cycle-rule`, `--threshold`, `--plugin`) |
| 2 | `--config <file>` |
| 3 | `code-split.toml` in cwd, then in target directory |
| 4 | `Cargo.toml` `[workspace.metadata.code-split]` / `[package.metadata.code-split]` |
| 5 | Built-in defaults |

**Config file keys** (`code-split.toml` or `Cargo.toml` metadata section):

```toml
plugin = "rust"          # default plugin; overridden by --plugin

[ignore]
paths        = ["**/generated/**"]  # glob patterns matched against node path
test_modules = true      # strip all mod::tests submodules (IDs ending in ::tests)
dev_only_crates = true   # strip crates reachable only via [dev-dependencies]
                         # (uses `cargo metadata` for transitive accuracy)

[rules.cycles]
test-embed = "allow"     # default: allow  (Rust #[cfg(test)] back-edge)
mutual     = "deny"      # default: deny
chain      = "deny"      # default: deny

[rules.thresholds.node]  # flag any single node exceeding the limit
hk         = 500_000
cyclomatic  = 25

[rules.thresholds.avg]   # flag when graph-wide average exceeds the limit
hk         = 50_000
cyclomatic  = 10
```

**CLI flags**:

- `--plugin <NAME>` — override default plugin
- `--config <FILE>` — load config from explicit path
- `--ignore <GLOB>` — add a path glob (repeatable, merged with file)
- `--cycle-rule <KIND=SEVERITY>` — override a cycle rule (e.g. `mutual=warn`)
- `--threshold <SCOPE.METRIC=N>` — set a threshold (e.g. `node.hk=500000`)
- `--exit-zero` — exit 0 even when `deny` violations are found (collect-only mode)

**Severity levels**: `allow` (strip from snapshot) · `warn` (report, exit 0) · `deny` (report, exit 1 unless `--exit-zero`).

The path of the config file actually used is recorded in the snapshot as `config_file`.

**Rationale**: Teams need to suppress expected patterns (e.g. `test-embed`
cycles, dev-only crate noise) and enforce structural budgets in CI without
modifying source code.

**Actors**: `cpt-code-split-actor-developer`, `cpt-code-split-actor-ci`

### 5.2 Visualization Reports — Step 2

#### HTML Report Generation

- [x] `p1` - **ID**: `cpt-code-split-fr-html-report`

The `code-split report` subcommand MUST read a snapshot `.json` file and
generate a single self-contained offline HTML file. The HTML MUST
include:

- Interactive graph visualization for each level (module, file,
  function)
- A coupling metrics table showing node weight (fan-in + fan-out) for
  each node at each level
- All JavaScript and CSS inlined (no CDN or external resources)

**Rationale**: A self-contained HTML file requires no server, no
internet, and no installed dependencies to view — maximizing
accessibility for developers and reviewers on any machine.

**Actors**: `cpt-code-split-actor-developer`, `cpt-code-split-actor-tech-lead`

#### Node Sorting by Weight

- [x] `p1` - **ID**: `cpt-code-split-fr-node-sorting`

The HTML report MUST allow the user to sort nodes by coupling weight
(fan-in + fan-out edge count). The report MUST display the top-N
heaviest nodes prominently at each level. Sorting MUST be performed
client-side within the HTML (no server required).

**Rationale**: The heaviest nodes are the most likely candidates for
refactoring. Surfacing them first reduces the time to actionable insight.

**Actors**: `cpt-code-split-actor-developer`, `cpt-code-split-actor-tech-lead`

#### AI Prompt Generator (P2)

- [x] `p2` - **ID**: `cpt-code-split-fr-ai-prompts`

The HTML report SHOULD include a UI control that generates a prompt for
an LLM, pre-populated with the top-N heaviest nodes and their coupling
context, asking for refactoring recommendations. The prompt format MUST
be copyable as plain text for direct paste into any LLM interface.

**Rationale**: Connecting structural data to an LLM's reasoning closes
the loop between measurement and advice without coupling the offline
tool to a specific LLM provider.

**Actors**: `cpt-code-split-actor-developer`, `cpt-code-split-actor-tech-lead`

#### Principles-Based Prompt Generation (P3)

- [x] `p3` - **ID**: `cpt-code-split-fr-principles-prompts`

The HTML report SHOULD support a principles-audit prompt mode that maps
the top coupling findings to the canonical principle corpus under
`principles/<language>/` (currently `rust/`, `python/`, `typescript/`)
and instructs the LLM to audit the codebase against each principle.

**Actors**: `cpt-code-split-actor-developer`, `cpt-code-split-actor-tech-lead`

### 5.3 Diff Analysis — Step 4

#### Graph Diff Engine

- [x] `p1` - **ID**: `cpt-code-split-fr-graph-diff`

The `code-split diff` subcommand MUST accept two snapshot `.json` files
and compute a structured diff per level: nodes and edges added, removed,
or with changed weight. The diff MUST include an overall coupling
direction verdict: `improved` (total weight fell), `degraded` (total
weight rose), or `neutral` (no significant change). The interactive
diff HTML uses Graphviz WASM (bundled in the binary) for client-side
DOT→SVG layout with cluster grouping; Modules/Files/Functions view
switcher and Before/After/Diff/Cycles presets are independent controls
in diff mode; when only one snapshot is loaded (review mode) the viewer
switches to an All/Cycles preset pair, hides diff-specific filter chips,
shows a single-column summary table, and relabels the header accordingly.
Cycle detection (Tarjan SCC) runs in-browser and annotates nodes/edges
for red-stroke highlighting (solid red, no dasharray, same style for
before/after). All nodes render with a uniform blue fill; all edges
render as uniform blue solid lines — no per-kind or per-status color
differentiation. No legend overlay. The node table column order is:
checkbox, Name, Kind, Cycle (modules only), Status, LOC, HK, Fan-in,
Fan-out, H.vol, H.bugs, H.effort, H.time, H.len, H.vocab, Cyclomatic,
Cognitive, MI, MI SEI, Logical, Comments, Blank. A checkbox column
(leftmost) enables persistent multi-node selection: clicking a checkbox
highlights the row (yellow) and the corresponding SVG node (yellow fill
+ amber stroke); shift-click selects a range; the header checkbox
selects or deselects all visible rows (indeterminate when partial).
The modal popup opened by clicking a row or an SVG node is fullscreen
(locks body scroll); it includes a synced selection checkbox, fields in
order id (⎘ copy) → path (⎘ copy, filename bold) → kind → visibility
→ items/methods → cycle info → status → metric sections in a single
column. Hover highlight (blue drop-shadow) takes CSS priority over
selection highlight.

**Rationale**: The diff is the quantitative answer to "did my
refactoring reduce coupling?" Without it, the user must compare two
static visualizations manually.

**Actors**: `cpt-code-split-actor-developer`, `cpt-code-split-actor-tech-lead`,
`cpt-code-split-actor-pr-reviewer`

#### Diff HTML Report

- [x] `p1` - **ID**: `cpt-code-split-fr-diff-html-report`

The diff tool MUST generate a single self-contained offline HTML report
displaying:

- Added / removed / changed nodes and edges per level, color-coded by
  direction (green = added, amber = removed, grey = affected)
- Cycle detection per level: nodes/edges in dependency cycles annotated
  with `before-only` / `after-only` / `both` / `none` status; red-stroke
  highlighting toggled via Cycles chip row
- Legend explaining edge kind (contains/uses/reexports/calls), diff
  colors, and cycle stroke styles
- Diff summary table: node/edge counts and cycle counts (SCCs, nodes in
  cycles) for each level, before vs after with Δ
- All JavaScript and CSS bundled locally (no CDN or external resources)

**Rationale**: Self-contained HTML is viewable without tooling and
suitable for attaching to PRs or sharing with stakeholders.

**Actors**: `cpt-code-split-actor-developer`, `cpt-code-split-actor-tech-lead`,
`cpt-code-split-actor-pr-reviewer`

#### Structured Compare Command

- [x] `p1` - **ID**: `cpt-code-split-fr-compare`

The `code-split compare` subcommand MUST accept two snapshot `.json` files
and output a machine-readable JSON diff summary. Default output is
stdout; `--output <file>` writes to a file.

JSON schema:

```json
{
  "schema_version": "1",
  "before": { "target": "…", "branch": "…", "commit": "…" },
  "after":  { "target": "…", "branch": "…", "commit": "…" },
  "identical": false,
  "modules":   { "nodes": { "added": 0, "removed": 0, "affected": 0, "unchanged": 26 },
                 "edges": { … }, "cycle_nodes_before": 10, "cycle_nodes_after": 10,
                 "sccs_before": 4, "sccs_after": 4 },
  "files":     { … },
  "functions": { … }
}
```

With `--html`, outputs a single self-contained interactive HTML report
instead of JSON. The report embeds all JS/CSS assets (including Graphviz
WASM) and both snapshot JSON objects inline — no network required, fully
offline-capable from `file://`. This is the CI-shareable artifact.

**Rationale**: Provides a stable machine-readable interface for CI
pipelines and scripts; the `--html` flag produces a shareable diff viewer
without requiring a separate `code-split diff` step.

**Actors**: `cpt-code-split-actor-developer`, `cpt-code-split-actor-ci`,
`cpt-code-split-actor-pr-reviewer`

#### Text Change Report

- [x] `p1` - **ID**: `cpt-code-split-fr-diff-text-report`

The `code-split compare` subcommand emits a structured JSON summary (see
`cpt-code-split-fr-compare`) embeddable in CI logs and PR comments. The
JSON contains node/edge counts and delta per level plus cycle SCC counts.

**Actors**: `cpt-code-split-actor-ci`, `cpt-code-split-actor-pr-reviewer`

#### CI Diff Integration (P2)

- [ ] `p2` - **ID**: `cpt-code-split-fr-ci-diff`

`code-split compare` SHOULD act as a CI linter: exit non-zero when the diff
exceeds configurable thresholds (e.g. new cycles added, HK degraded
beyond a limit). The base-branch snapshot is fetched from a stored CI
artifact; the diff JSON is attached to the pull request automatically.

**Actors**: `cpt-code-split-actor-ci`, `cpt-code-split-actor-pr-reviewer`

## 6. Non-Functional Requirements

### 6.1 NFR Inclusions

#### Offline Operation

- [x] `p1` - **ID**: `cpt-code-split-nfr-offline`

All P1 components (Rust plugin, `code-split report`, `code-split diff`,
`code-split compare`) MUST operate without network access. External resources (CDNs, APIs, LLM
endpoints) are forbidden at P1. All JavaScript and CSS dependencies in
generated HTML MUST be bundled into the `code-split` binary as embedded
assets; no CDN or external resource references in generated HTML.

**Threshold**: Zero outbound network calls during any P1 operation.

**Rationale**: Workspaces may be on air-gapped machines, private CI
runners, or laptops without connectivity. Offline-first is a hard
requirement shared by all three steps.

#### Performance

- [x] `p1` - **ID**: `cpt-code-split-nfr-performance`

The Rust plugin MUST complete graph extraction for a 50k-LOC workspace
in ≤ 30 seconds wall-clock on a modern developer laptop (8-core, 16 GB
RAM, SSD), measured cold-cache. The `code-split report` and `code-split diff`
subcommands MUST each complete in ≤ 5 seconds for graphs with up to
10,000 nodes.

**Threshold**: ≤ 30 s for the plugin at 50k LOC; ≤ 5 s for each
subcommand at 10k nodes.

**Rationale**: Interactive use requires sub-minute turnaround.

#### Artifact Portability

- [x] `p1` - **ID**: `cpt-code-split-nfr-portability`

JSON snapshot artifacts MUST conform to Graph JSON Schema v1 and MUST
be readable by the report generator and diff engine without migration
for all v1.x releases. Generated HTML reports MUST open correctly in
Chrome, Firefox, and Safari without installation.

**Threshold**: Zero schema-migration failures within a major version.

**Rationale**: Artifacts stored as CI artifacts must remain readable
across plugin and tool version bumps within a major version.

### 6.2 NFR Exclusions

- **Accessibility**: Out of scope for v1.0.
- **Internationalization**: English-only in v1.0.
- **Regulatory Compliance**: Not applicable — the tool reads local
  source files only and produces no personal or regulated data.

## 7. Public Interfaces

### 7.1 Code Split Unified CLI

- [x] `p1` - **ID**: `cpt-code-split-interface-cli`

**Type**: Single CLI binary (`code-split`)

**Stability**: unstable (pre-1.0)

**Subcommands**:

```
# Step 1 — extract graphs using a plugin
code-split analyze  <workspace> --plugin <name|path> [--output <snap.json>] [--local-only] [-- <plugin-args>]

# Step 2 — generate HTML visualization report
code-split report   --input <snap.json> --output <report.html>

# Step 4 — compare two snapshots
code-split compare  --before <snap-a.json> --after <snap-b.json>          # JSON summary → stdout
code-split compare  --before <snap-a.json> --after <snap-b.json> --html --output diff.html  # interactive HTML
```

**Exit codes**: 0 = success; non-zero = failure with structured JSON
error on stderr.

**Breaking Change Policy**: Adding flags or subcommands is minor;
renaming or removing flags, changing JSON artifact schema, or changing
exit-code semantics requires a major-version bump.

### 7.2 Plugin Binary Contract

- [x] `p1` - **ID**: `cpt-code-split-interface-plugin-binary`

**Type**: Subprocess contract (any executable)

**Stability**: unstable (pre-1.0)

Every plugin binary — whether built-in, PATH-discovered, config-
registered, or path-specified — receives this invocation from
`code-split analyze`:

```
<binary> <workspace-path> --output <tmpfile> [-- <forwarded-plugin-args>]
```

**Output**: a single JSON file at `<tmpfile>` containing only the
`graphs` object (`modules`, `files`, `functions`). `code-split` merges
it with top-level metadata and writes the final snapshot `.json`.

**stderr on failure**: single-line JSON `{ "error": "...", "code": N }`.

This contract is the stable surface for third-party plugin authors.
Built-in plugins implement the same contract internally.

**Breaking Change Policy**: The binary contract is independently
versioned. Breaking changes are communicated via a `--contract-version`
flag added to the invocation.

### 7.3 Graph JSON Schema

- [x] `p1` - **ID**: `cpt-code-split-interface-graph-schema`

**Type**: Data format (JSON)

**Stability**: unstable (pre-1.0)

**Top-level shape** (full snapshot file):

```json
{
  "schema_version": "1",
  "generated_at":   "<ISO-8601>",
  "command":        "<full command line>",
  "workspace":      "<absolute-path>",
  "plugin":         "<plugin-id>",
  "local_only":     false,
  "versions":       { "code-split": "0.3.1", "plugin_rust": "0.3.1", "rustc": "1.78.0" },
  "git":            { "branch": "main", "commit": "a3f9c21", "dirty_files": 0 },
  "graphs": {
    "modules":   { "nodes": [...], "edges": [...], "stats": { ... } },
    "files":     { "nodes": [...], "edges": [...], "stats": { ... } },
    "functions": { "nodes": [...], "edges": [...], "stats": { ... } }
  }
}
```

`files` is omitted entirely when the plugin produces no `NodeKind::File`
nodes (e.g. the Rust plugin). `stats` is omitted when a graph is empty.

**Graph stats shape** (`stats` field on each non-empty graph):

```json
{
  "cyclomatic": 4.2,
  "cognitive":  1.8,
  "coupling":     { "fan_in": 2.1, "fan_out": 3.4, "hk": 12.5 },
  "maintainability": { "mi": 72.1, "mi_sei": 68.4 },
  "loc":          { "source": 38.2, "logical": 0.0, "comments": 1.1, "blank": 5.3 },
  "halstead":     { "length": 210.4, "vocabulary": 48.1, "volume": 1240.2,
                    "effort": 85000.0, "time": 4722.2, "bugs": 0.413 }
}
```

Fields mirror the node `complexity` structure with per-graph averages (nodes
with zero values excluded from the average). Zero-valued scalar fields and
absent sub-objects are omitted. Percentiles are not stored in JSON — the
HTML viewer computes `p1`/`p10`/`p50`/`p90`/`p99` client-side from raw
node data. All numeric values use 3-significant-digit truncation.

**Node shape** (same across all three graphs):

```json
{
  "id":          "<stable-string-key>",
  "kind":        "crate | module | file | fn | method",
  "name":        "<short-name>",
  "path":        "{target}/src/foo.rs",
  "parent":      "<parent-node-id>",
  "loc":         42,
  "line":        15,
  "external":    false,
  "visibility":  "public",
  "item_count":  7,
  "method_count": 3,
  "complexity": {
    "cyclomatic": 3, "cognitive": 2, "exits": 2, "args": 3,
    "coupling": { "fan_in": 4, "fan_out": 2, "hk": 1344 },
    "maintainability": { "mi": 78.4, "mi_sei": 52.1 },
    "loc": { "source": 42, "logical": 12, "comments": 4, "blank": 6 },
    "halstead": {
      "length": 87, "vocabulary": 23,
      "volume": 312.5, "effort": 4820, "time": 267.8, "bugs": 0.104
    }
  }
}
```

All optional fields (`parent`, `loc`, `line`, `external`, `visibility`,
`item_count`, `method_count`, `complexity`) are omitted when null or empty.
`visibility` is a plain string (`"public"`, `"private"`, `"crate"`, `"super"`)
or an object `{"restricted": "<path>"}` for path-restricted visibility.
`path` values use named-root prefixes resolved via `roots` (e.g.
`{target}/src/main.rs`, `{registry}/anyhow-1.0.102/src/lib.rs`). For
`fn`/`method` nodes `loc` is the function body length in lines and `line`
is the 1-based declaration line; for `file` nodes `loc` is total file line
count. In the Rust plugin, file-backed `Module` nodes (those with `line = null`)
also carry `complexity` with file-level metrics (whole-file cyclomatic,
Halstead, MI, LOC). The `complexity` object is omitted entirely when all
sub-fields are zero/absent; within it, zero-valued scalar fields and absent
sub-objects are omitted. The `coupling` sub-object is omitted when both
`fan_in` and `fan_out` are 0. All numeric fields use 3-significant-digit
truncation; whole numbers are serialized without a decimal point.

**Edge shape**:

```json
{ "from": "<node-id>", "to": "<node-id>", "kind": "contains | uses | calls", "unresolved": false }
```
```

**Breaking Change Policy**: Additive fields are minor; renames or
removals require a major-version bump and migration notes.

## 8. Use Cases

### UC-001 Analyze Rust Workspace Offline

**ID**: `cpt-code-split-usecase-analyze-offline`

**Actors**: `cpt-code-split-actor-developer`

**Preconditions**: The target directory is a valid Cargo workspace;
the `code-split-rust` binary is installed.

**Main Flow**:

1. Developer runs `code-split analyze . --plugin rust`
2. Plugin produces `.code-split/snap-20260522-112233.json`
3. Developer runs `code-split report --input .code-split/snap-20260522-112233.json --output report.html`
4. Developer opens `report.html` in a browser, sorts modules by coupling
   weight
5. Developer identifies the heaviest modules and decides what to refactor

**Postconditions**: A standalone HTML report exists at `report.html`;
no network access was required at any step.

**Alternative Flows**:

- **Plugin fails (cargo metadata error)**: Plugin exits non-zero with
  a structured JSON error on stderr; no JSON files are written.

### UC-002 Before/After Refactoring Diff

**ID**: `cpt-code-split-usecase-diff-refactor`

**Actors**: `cpt-code-split-actor-developer`, `cpt-code-split-actor-tech-lead`

**Preconditions**: A "before" snapshot exists from a prior run; the
developer has made structural changes to the codebase.

**Main Flow**:

1. Developer runs `code-split analyze . --plugin rust --output .code-split/snap-after.json`
2. Developer runs
   `code-split diff --before .code-split/snap-before.json --after .code-split/snap-after.json --html diff.html --md diff.md`
3. Developer opens `diff.html` to see coupling changes color-coded by
   direction
4. Developer reads `diff.md` for the text summary including overall
   coupling verdict

**Postconditions**: Diff HTML and Markdown reports exist; the verdict
quantifies whether the refactoring improved the architecture.

**Alternative Flows**:

- **Schema version mismatch**: Diff tool exits non-zero with an error
  identifying the incompatible artifact; no report is produced.

### UC-003 CI Structural Diff on Pull Request

**ID**: `cpt-code-split-usecase-ci-diff`

**Actors**: `cpt-code-split-actor-ci`, `cpt-code-split-actor-pr-reviewer`

**Note**: This use case is targeted at P2.

**Preconditions**: The base-branch snapshot is stored as a CI artifact;
the PR branch has been pushed.

**Main Flow**:

1. CI downloads the base-branch snapshot to `./snap-base`
2. CI runs `code-split-rust . --output-dir ./snap-pr`
3. CI runs
   `code-split diff --before ./snap-base --after ./snap-pr --html diff.html --md diff.md`
4. CI attaches `diff.html` to the PR and posts `diff.md` as a PR comment
5. PR Reviewer reads the coupling-change summary and diff report without
   local setup

**Postconditions**: Structural coupling changes are visible at PR time
as a self-contained HTML report.

## 9. Acceptance Criteria

- [x] Rust plugin produces three valid JSON files for a reference
  workspace in ≤ 30 s on a modern laptop
- [x] HTML report opens in Chrome/Firefox/Safari with interactive graph
  visualization and client-side node sorting by coupling weight
- [x] Diff tool produces color-coded HTML report from two snapshots;
  the coupling direction verdict is present
- [x] All P1 tools operate with zero outbound network calls
- [x] Generated HTML reports contain no external resource references
- [x] JSON artifacts conform to Graph JSON Schema v1 and pass schema
  validation
- [ ] Diff tool exits non-zero with a structured error on schema version
  mismatch

## 10. Dependencies

| Dependency | Description | Priority |
|------------|-------------|----------|
| `ra_ap_*` crates | rust-analyzer libraries for function-level call resolution in the Rust plugin | p1 |
| `cargo_metadata` crate | Cargo workspace enumeration | p1 |
| `syn` crate | Rust source parsing for module/file structure | p1 |
| `petgraph` crate | In-memory graph representation in the Rust plugin | p1 |
| `rust-code-analysis` crate | Tree-sitter-based multi-language metrics library (cyclomatic, cognitive, Halstead, MI, LOC, NOM, nexits, nargs); used via fork `ffedoroff/rust-code-analysis` | p1 |
| Python 3.9+ | Runtime for the built-in Python language plugin | p3 |

## 11. Assumptions

- Target Rust workspaces are buildable with the host Rust toolchain
- `ra_ap_*` crates pinned to a known revision provide stable APIs for
  the v1.0 timeframe
- Browsers rendering the HTML reports support modern JavaScript (ES2020+)
- The base-branch snapshot used for diffs was produced by the same
  major version of the Rust plugin (schema compatibility guaranteed
  within a major version)

## 12. Risks

| Risk | Impact | Mitigation |
|------|--------|------------|
| `ra_ap_*` API churn breaks the Rust plugin | High — blocks releases | Pin versions; isolate behind a stable internal trait in `code-split-core` |
| Function graph too large to visualize in-browser | Medium — unusable HTML report | Implement pagination and level filtering in the report; warn user when node count exceeds threshold |
| Snapshot schema divergence between plugin versions | Medium — silent diff failures | Enforce schema version check at diff time; abort with structured error on mismatch |
| Performance regressions on large workspaces | Medium — usability loss | Benchmark suite in CI on a curated 5k and 50k LOC corpus |
| P3 plugin contract extensions break base schema consumers | Low — only affects P3 adopters | Extensions use optional fields only; base consumers skip unknown fields |
