# Node JSON Schema

Reference for the node objects emitted in code-split snapshot files
(`.code-split/<project>-<timestamp>.json`), under `graphs.modules.nodes`,
`graphs.files.nodes`, and `graphs.functions.nodes`.

## Full example

```json
{
  "id": "file:{target}/src/test/setup.ts",
  "kind": "file",
  "name": "setup.ts",
  "path": "{target}/src/test/setup.ts",
  "parent": "mod:src::test",
  "visibility": "public",
  "line": null,
  "item_count": null,
  "method_count": null,
  "cycle_kind": null,
  "complexity": {
    "cyclomatic": 1,
    "cognitive": 0,
    "coupling": {
      "fan_in": 2,
      "fan_out": 3,
      "hk": 144
    },
    "maintainability": {
      "mi": 95.867,
      "mi_sei": 63.319
    },
    "loc": {
      "total": 16,
      "source": 15,
      "physical": 14,
      "logical": 7,
      "comments": 0,
      "blank": 1
    },
    "halstead": {
      "length": 63,
      "vocabulary": 27,
      "volume": 299.557,
      "effort": 1100.875,
      "time": 61.159,
      "bugs": 0.0355
    }
  }
}
```

All optional fields are omitted when null or not applicable to the node kind.
Numeric fields inside `complexity` use 3-significant-digit serialization.

---

## Top-level fields

### `id` — string, required

Stable unique key for this node. The scheme depends on the node kind:

| kind | scheme | example |
|------|--------|---------|
| `crate` | `crate:{name}` | `crate:tokio` |
| `module` | `mod:{crate}::{dotted.path}` | `mod:myapp::db::schema` |
| `file` | `file:{path}` | `file:{target}/src/api/auth.ts` |
| `fn` | `fn:{crate}::{mod}::{name}` | `fn:myapp::db::schema::find_user` |
| `method` | `method:{crate}::{mod}::{type}::{name}` | `method:myapp::db::User::save` |
| `trait` | `trait:{crate}::{mod}::{name}` | `trait:myapp::storage::Repository` |

IDs contain no line numbers or byte offsets and remain stable across
code moves within the same module.

### `kind` — string, required

The structural category of this node. One of:

| value | description | plugins |
|-------|-------------|---------|
| `crate` | A Cargo crate (root of a Rust package) | rust |
| `module` | A logical namespace / directory — in Rust a `.rs` file IS its module | rust, python, js |
| `file` | A source file — used when files and modules are distinct entities | python, js |
| `fn` | A standalone function or free function | rust, python, js |
| `method` | A function that belongs to a type or class | rust, python |
| `trait` | A Rust trait definition | rust |
| `impl` | A Python class body (groups methods) | python |

### `name` — string, required

Short human-readable name without path or module prefix.
Examples: `"setup.ts"`, `"find_user"`, `"UserService"`.

### `path` — string, required

Physical location of the source file that defines this node.
Uses named-root prefixes so paths are portable across machines:

| prefix | resolves to |
|--------|-------------|
| `{target}` | the analyzed project root |
| `{workspace}` | the code-split workspace root |
| `{registry}` | Cargo registry cache |
| `{rustup}` | rustup toolchain root |

Examples: `{target}/src/api/auth.ts`, `{registry}/tokio-1.38.0/src/lib.rs`.

### `parent` — string, optional

`id` of the containing node. `null` for root nodes (top-level crates,
top-level modules without a parent directory).

### `visibility` — string or object, optional

Declared visibility of the node.

Simple cases are represented as a plain string:

| value | meaning |
|-------|---------|
| `"public"` | visible to everyone (`pub`) |
| `"private"` | visible only within the current module (default in Rust) |
| `"crate"` | visible within the current crate (`pub(crate)`) |
| `"super"` | visible to the parent module (`pub(super)`) |

When visibility is path-restricted, an object is used instead:

```json
"visibility": { "restricted": "crate::services::platform_client" }
```

`null` for nodes that have no inherent visibility (e.g. `crate` nodes).

### `line` — integer, optional

1-based line number of the node's declaration within its file.
Present for `fn`, `method`, and inline `module` nodes.
`null` for file-backed modules, files, crates, and traits.

### `item_count` — integer, optional

Number of direct child items declared in this node.
Present only for file-backed `module` nodes in the Rust plugin.
`null` for all other kinds and plugins.

### `method_count` — integer, optional

Number of methods declared in this trait.
Present only for `trait` nodes in the Rust plugin.
`null` for all other kinds and plugins.

### `cycle_kind` — string, optional

Set when this node participates in a dependency cycle. `null` otherwise.

| value | meaning |
|-------|---------|
| `"test_embed"` | cycle caused by a `#[cfg(test)]` back-edge (Rust only) |
| `"mutual"` | two nodes that directly depend on each other (SCC size = 2) |
| `"chain"` | cycle involving three or more nodes (SCC size ≥ 3) |

---

## `complexity` — object, optional

All code and structural metrics for this node. Omitted entirely when no
metrics are available (external crates, empty files, pure-namespace modules).

### `complexity.cyclomatic` — number

**Cyclomatic complexity** (McCabe, 1976). Counts the number of linearly
independent paths through the code: `branches + 1`. Each `if`, `else if`,
`for`, `while`, `match` arm, `&&`, `||` adds 1.

- Minimum value: **1** (a straight-line function with no branches)
- Good range: 1–5; review at >10; refactor at >20
- Computed from: AST branch nodes

### `complexity.cognitive` — number

**Cognitive complexity** (SonarSource, 2018). Measures how difficult the
code is to *read and understand*, not just to test. Unlike cyclomatic,
it penalises nesting: an `if` inside a loop inside another `if` costs
more than three flat `if` statements.

- Minimum value: **0**
- More sensitive to deeply nested code than cyclomatic
- Computed from: AST structure with nesting weights

### `complexity.coupling` — object, optional

Structural coupling metrics derived from the dependency graph (edges),
not from source code. Present for nodes that participate in dependency
analysis (`module`, `file`, `crate`). Omitted for `fn`/`method` in
plugins that do not track function-level imports.

#### `coupling.fan_in` — number

Number of other nodes that **depend on** this node (incoming `Uses` edges).
A high fan_in means many callers — changing this node is risky.

#### `coupling.fan_out` — number

Number of nodes that **this node depends on** (outgoing `Uses` edges).
A high fan_out means broad responsibilities — the node knows too much.

#### `coupling.hk` — number

**Henry-Kafura complexity** (1984):

```
hk = loc × (fan_in × fan_out)²
```

Combines size with coupling. A small isolated module has `hk = 0`.
A large hub module can reach values in the millions. Use as a relative
ranking within a project rather than an absolute threshold.

### `complexity.maintainability` — object, optional

Composite indices that estimate how easy the code is to maintain.
Both are derived from `halstead.volume`, `cyclomatic`, and LOC.

#### `maintainability.mi` — number

**Maintainability Index** (Oman & Hagemeister, 1992):

```
MI = 171 − 5.2 × ln(halstead.volume) − 0.23 × cyclomatic − 16.2 × ln(loc.source)
```

Higher is better. Rough thresholds: >85 — easy to maintain;
65–85 — moderate effort; <65 — difficult. Can go negative for very
complex files.

#### `maintainability.mi_sei` — number

**MI (SEI variant)** (Carnegie Mellon SEI, 1997). Adds a bonus term
for comment density:

```
MI_SEI = MI + 50 × sin(√(2.4 × comment_ratio))
```

When `loc.comments = 0` the bonus is zero and `mi_sei` equals `mi`.
A well-documented file can score ~25 points higher than its raw `mi`.

### `complexity.loc` — object, optional

Line-of-code breakdown. Multiple LOC definitions coexist because each
answers a different question.

#### `loc.total` — number

Total lines in the file or function body, including everything.
Same as `ploc` in legacy notation.

#### `loc.source` — number

**Source lines** — lines that contain at least one non-whitespace,
non-comment character. The most common LOC metric.
(`sloc` in legacy notation.)

#### `loc.physical` — number

**Physical lines** — same as `total` for most tools; in some
implementations excludes the last blank line. (`ploc` in legacy notation.)

#### `loc.logical` — number

**Logical lines** — counts statements and expressions rather than
physical lines. A one-liner with three statements counts as 3.
(`lloc` in legacy notation.)

#### `loc.comments` — number

Lines that consist entirely of comments (inline comments on code lines
are not counted). (`cloc` in legacy notation.)

#### `loc.blank` — number

Empty or whitespace-only lines.

### `complexity.halstead` — object, optional

**Halstead metrics** (Halstead, 1977) treat a program as a sequence of
operators (keywords, punctuation, operators) and operands (identifiers,
literals). Two raw counts drive all derived metrics:

- **n1** — number of *unique* operators  
- **n2** — number of *unique* operands  
- **N1** — total operator occurrences  
- **N2** — total operand occurrences

#### `halstead.length` — number

Program length: `N = N1 + N2`. Total count of all operator and operand
tokens in the code unit.

#### `halstead.vocabulary` — number

Program vocabulary: `n = n1 + n2`. Number of distinct operators and
operands. Grows logarithmically with program size.

#### `halstead.volume` — number

Program volume: `V = N × log₂(n)`. Represents the information content
of the program in bits. The primary Halstead size metric.

Typical ranges: trivial function ~10, average function ~500,
complex file ~50 000+.

#### `halstead.effort` — number

Mental effort required to write the program:
`E = (n1/2n2) × N × log₂(n)`.

Correlates strongly with development time. Used as input for `mi`.

#### `halstead.time` — number

Estimated programming time in **seconds**: `T = E / 18`.
The divisor 18 is an empirical constant (Stroud number).
Treat as a rough order-of-magnitude estimate only.

#### `halstead.bugs` — number

Estimated number of latent bugs delivered: `B = V / 3000`.
The divisor 3000 is empirical. More useful as a relative
ranking than an absolute count.
