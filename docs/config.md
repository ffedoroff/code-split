# code-split configuration

## Priority order

Settings are merged from multiple sources. **Higher priority wins** for the same key.

| Priority | Source | Example |
|---|---|---|
| 1 | CLI flags | `--ignore '**/tests/**'` |
| 2 | `--config <file>` | `--config ci/code-split.toml` |
| 3 | `code-split.toml` in cwd | `./code-split.toml` |
| 4 | `code-split.toml` in workspace root | `<workspace>/code-split.toml` |
| 5 | `Cargo.toml` metadata | `[workspace.metadata.code-split]` |
| 6 | Built-in defaults | `test-embed = allow`, all else = deny |

For `ignore.paths` and CLI `--ignore`: lists are **merged** (union), not replaced.  
For cycle rules and thresholds: CLI **overrides** the file value.

---

## Config file: `code-split.toml`

```toml
# Default plugin. Overridden by --plugin.
plugin = "rust"

[ignore]
paths = [
  "**/tests/**",
  "**/generated/**",
  "crates/*/benches/**",
]

[rules.cycles]
test-embed = "allow"   # default
mutual     = "deny"    # default
chain      = "deny"    # default

[rules.thresholds.node]   # any single node exceeds → violation
hk         = 500_000
cyclomatic  = 25
cognitive   = 30
fan_out     = 50

[rules.thresholds.avg]    # graph-wide average exceeds → violation
hk         = 50_000
cyclomatic  = 10
```

---

## Config in `Cargo.toml`

Useful when you don't want an extra file. Supports the same keys under
`[workspace.metadata.code-split]` (monorepo) or `[package.metadata.code-split]`
(single crate).

```toml
[workspace.metadata.code-split.ignore]
paths = ["**/tests/**"]

[workspace.metadata.code-split.rules.cycles]
test-embed = "allow"
mutual     = "deny"

[workspace.metadata.code-split.rules.thresholds.node]
hk = 500_000
```

---

## CLI flags

All config values can be set or overridden from the command line.

### `--plugin <NAME>`
Override the default plugin (`rust`, `python`, or a path to a binary).  
Falls back to `plugin` in config file, then `"rust"`.

```bash
code-split analyze .                   # uses config.plugin or "rust"
code-split analyze . --plugin python   # always uses python
```

### `--config <FILE>`
Load config from an explicit path instead of auto-discovery.

```bash
code-split analyze . --config ci/strict.toml
```

### `--ignore <GLOB>`
Add a path glob to the ignore list. Repeatable.

```bash
code-split analyze . --ignore '**/tests/**' --ignore '**/generated/**'
```

### `--cycle-rule <KIND=SEVERITY>`
Override a cycle rule. `KIND`: `test-embed` | `mutual` | `chain`.  
`SEVERITY`: `allow` | `warn` | `deny`. Repeatable.

```bash
# Suppress test-embed cycles, treat mutual cycles as warnings
code-split analyze . --cycle-rule test-embed=allow --cycle-rule mutual=warn
```

### `--threshold <SCOPE.METRIC=N>`
Set a threshold. `SCOPE`: `node` | `avg`. `METRIC`: `hk` | `cyclomatic` |
`cognitive` | `fan_in` | `fan_out` | `loc`. Repeatable.

```bash
code-split analyze . --threshold node.hk=500000 --threshold avg.cyclomatic=10
```

### `--exit-zero`
Exit 0 even when `deny` violations are found. Useful in CI when you want to
collect the snapshot as an artifact without blocking the pipeline.

```bash
code-split analyze . --exit-zero
```

Without this flag, `code-split analyze` exits 1 whenever at least one `deny`
violation is found — matching the default behaviour of tools like `ruff check`
and `cargo clippy -- -D warnings`.

---

## Severity levels

| Level | Effect |
|---|---|
| `allow` | Strip from snapshot; not shown in reports |
| `warn`  | Shown in report; exit 0 |
| `deny`  | Shown in report; exit 1 (unless `--exit-zero`) |

---

## Typical CI setup

```yaml
# collect-only (never blocks the pipeline)
- run: code-split analyze . --exit-zero

# linter mode (blocks on any deny violation)
- run: code-split analyze .
```

Or with inline overrides to tighten rules in CI without changing `code-split.toml`:

```bash
code-split analyze . --cycle-rule chain=deny --threshold node.hk=200000
```
