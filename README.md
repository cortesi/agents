# agents

A tiny CLI that renders a per‑project `AGENTS.md` by combining a project‑local
template at `<project-root>/.agents.md` (optional) with a shared template at
`~/.agents.md` (or an override). Both files are fully interpreted templates.
It evaluates simple matchers against the target project (e.g., `exists("**/*.rs")`)
to conditionally include or skip blocks of Markdown.

---

## How it works

1. **Locate project root** (prefers git root; falls back to heuristics / provided path).
2. **Load templates**: optional local `<project-root>/.agents.md` and shared `~/.agents.md` (unless overridden).
3. **Evaluate matchers** against the target project (filesystem, metadata, environment).
4. **Render and combine**: render the local template (if present), then the shared template; concatenate results.
5. **Write** the result to `<project-root>/AGENTS.md`.

---

## Installation

Install using Cargo:

```bash
cargo install agents
```

---

## Usage

### Basic

```bash
# Render for the current project (detected root) and write AGENTS.md there
agents

# Render for a specific project path
agents /path/to/project
```

### Flags

```
--template <path>     Override template (defaults to ~/.agents.md)
--root <path>         Force project root (skip detection)
--stdout              Print instead of writing AGENTS.md
--diff                Show a unified diff of pending changes, do not write
--claude              Also write CLAUDE.md alongside AGENTS.md
--out <path>          Override output file path (relative to project root if not absolute)
-V, --version         Print version
-h, --help            Help
```

---

## Project root detection

The tool walks upward from the provided path (or current working directory) and picks the first match:

1. A directory containing a version control directory: `.git/`, `.hg/`, or `.svn/` (preferred)
2. Otherwise, the nearest directory containing a `Cargo.lock`

If neither is found, `agents` exits with a non‑zero status. The resolved root is where `AGENTS.md` is written.

---

## Templates

### Project‑local template (`.agents.md`)

- Optional file at `<project-root>/.agents.md`.
- Fully interpreted using the same template language as the shared template.
- Rendered first; its output is concatenated before the shared template output.

### Template language (minimal & Markdown‑safe)

Templates are just Markdown with **HTML‑comment control tags**, so they still render cleanly if opened directly. We use comments so templates remain readable and portable across viewers. The control syntax stays invisible in rendered Markdown, diff‑friendly in git, and avoids executing arbitrary code—only simple boolean checks (e.g., `exists`, `env`).

#### Block conditionals

```md
<!-- if exists("**/*.rs") -->
This project contains Rust sources.
<!-- endif -->
```

#### Expressions

* Combine conditions with `&&`, `||`, `!` and parentheses.
* Strings may use **single quotes**, **double quotes**, or **raw strings** to reduce escaping.

  * Examples: `exists('src/**/{main,lib}.rs')`, `exists("Cargo.toml")`, `exists(r"**/*.rs")`.

Example:

```md
<!-- if exists("package.json") && !exists("pnpm-lock.yaml") -->
Using npm or yarn (no pnpm lockfile found).
<!-- endif -->
```

### Matchers

* `exists(pattern)`: true if any non‑ignored file under the project root matches
  the pattern. Matching uses the Rust `ignore` crate (gitignore semantics).
  * Syntax: gitignore‑style via `ignore`/`globset`: `*`, `?`, `**` (recursive),
    and `{a,b}` alternation.
  * Scope: patterns are evaluated relative to the project root (treat as
    root‑relative).
  * Ignores: respects `.gitignore`, `.ignore`, and `.git/info/exclude` — paths
    ignored there are skipped and will not match.
  * Examples:
    * `exists("**/*.rs")`
    * `exists("Cargo.toml")`
    * `exists('src/**/{main,lib}.rs')`
* `env(NAME)`: true if environment variable `NAME` is set and non‑empty in the current process environment.
* `env(NAME=value)`: true if environment variable `NAME` exists **and** exactly equals `value` (string comparison).
  * Values with spaces or special characters may be quoted: `env("MY FLAG"="on")`.
  * Name matching follows the host OS conventions (typically case‑sensitive on Unix).
  * Examples:
    * `env(CI)`
    * `env(NODE_ENV=production)`
    * `env(RUST_LOG=debug)`
* `lang(name)`: true if any non‑ignored file under the project root has a file
  extension associated with the given programming language name. Language
  lookup is powered by the [languages](https://github.com/cortesi/languages)
  crate and is case‑insensitive.
  * Examples: `lang(rust)`, `lang("TypeScript")`, `lang(r"C++")`
  * Unknown languages are treated as template errors.

### Template grammar

| Element | Syntax | Notes |
| --- | --- | --- |
| Conditional block | `<!-- if EXPR --> … <!-- endif -->` | HTML‑comment control tags; blocks may nest; `endif` cannot have trailing content. |
| Operators | `!`, `&&`, `||`, `()` | Precedence: `!` > `&&` > `||`; whitespace is ignored between tokens. |
| Matcher: `exists` | `exists(PATTERN)` | Gitignore/globset pattern, relative to project root; matches files only; respects `.gitignore`, `.ignore`, and git excludes. |
| Matcher: `env` (exists) | `env(NAME)` | True when env var is set and non‑empty; `NAME` may be quoted or bare. |
| Matcher: `env` (equals) | `env(NAME=VALUE)` | True when env var exists and equals `VALUE` (string compare); `NAME`/`VALUE` may be quoted or raw. |
| Matcher: `lang` | `lang(NAME)` | True when any file matches extensions for language `NAME` (case‑insensitive); unknown names are errors. |
| Strings | `'...'`, `"..."`, `r"..."`, or bare token | Quoted strings support `\n`, `\r`, `\t`, `\\`, `\'`, `\"`; raw strings take contents verbatim; bare tokens end at whitespace or `)`. |
| Other comments | `<!-- … -->` | Non‑control comments are preserved verbatim in output. |
| Parse errors | — | Unclosed `if`, stray `endif`, trailing characters in expressions, invalid glob patterns, and unknown languages cause a non‑zero exit. |

### Examples

**`~/.agents.md`** (shared template)

```md
<!-- if exists("**/*.rs") -->
run cargo clippy before finalising work and fix all warnings
<!-- endif -->
```

```md
<!-- if lang(rust) -->
This project contains Rust code.
<!-- endif -->
```

**`<project>/.agents.md`** (per‑project template)

```md
<!-- if exists("README.md") -->
# Project Agents
<!-- endif -->
```

**Rendered `AGENTS.md`** (local then shared)

```md
# Project Agents

run cargo clippy before finalising work and fix all warnings
```

---

## Configuration

Priority order (first present wins):

1. `--template <path>`
2. `AGENTS_TEMPLATE` env var
3. `~/.agents.md` (default)

Additional inputs:

* `<project-root>/.agents.md`: Optional per‑project template rendered before the shared one.

---

## Errors, exit codes, and idempotency

* **Template errors** (e.g., unmatched `endif`, invalid expression, unknown
  matcher): the process **exits with a non‑zero status** and does not write
  output.
* **Idempotency**: running `agents` with the same inputs (shared template,
  local `.agents.md`, project tree, and env) yields **byte‑identical**
  `AGENTS.md`. Re‑running without changes results in no diff and no rewrite.
* **Determinism**: evaluation is pure with respect to the project tree and the
  current environment; there are no network calls or time‑dependent behaviors.

