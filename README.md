![Discord](https://img.shields.io/discord/1381424110831145070?style=flat-square&logo=rust&link=https%3A%2F%2Fdiscord.gg%2FfHmRmuBDxF)
[![Crates.io](https://img.shields.io/crates/v/agentsmd.svg)](https://crates.io/crates/agentsmd)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)


# agentsmd

Generate per‑project `AGENTS.md` and `CLAUDE.md` files by combining a
project‑local template at `<project-root>/.agents.md` with a shared template at
`~/.agents.md`. Templates evaluate matchers against the target project (e.g.,
`lang(rust)`) to conditionally include blocks, letting you tailor the output to
your project.

---

## How it works

1. **Finds project root** by scanning upward for `.git/` or other VCS markers
2. **Loads templates** from `~/.agents.md` (shared) and `<project-root>/.agents.md` (local)
3. **Evaluates conditional blocks** using matchers like `exists("**/*.rs")` and `env(CI)`
4. **Renders output** by concatenating local template results with shared template results
5. **Writes** `AGENTS.md` (and optionally `CLAUDE.md`) to the project root

---

## Installation

```bash
cargo install agentsmd
```

---

## Usage

```bash
# Render for the current project and write AGENTS.md to the project root
agentsmd

# Render for the current project and write AGENTS.md to the project root
agentsmd --claude

# Render for a specific project path and write AGENTS.md to the project root
agentsmd /path/to/project
```

```
--template <path>     Override template (defaults to ~/.agents.md)
--root <path>         Force project root (skip detection)
--stdout              Print instead of writing AGENTS.md
--diff                Show a unified diff of pending changes, do not write
--quiet               Suppress default diff output when writing changes
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

Templates are just Markdown with **HTML‑comment control tags**, so they still
render cleanly if opened directly. We use comments so templates remain readable
and portable across viewers. The control syntax stays invisible in rendered
Markdown, diff‑friendly in git, and avoids executing arbitrary code - only
simple boolean checks (e.g., `exists`, `env`).

#### Block conditionals

```md
<!-- if exists("**/*.rs") -->
This project contains Rust sources.
<!-- endif -->
```

#### Expressions

* Combine conditions with `&&`, `||`, `!` and parentheses.
* Strings may use **single quotes**, **double quotes**, or **raw strings** to reduce escaping.
  * Examples: `exists('src/**/{main,lib}.rs')`, `exists("Cargo.toml")`,
    `exists(r"**/*.rs")`.

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

## Errors, exit codes, and idempotency

* **Template errors** (e.g., unmatched `endif`, invalid expression, unknown
  matcher): the process **exits with a non‑zero status** and does not write
  output.
* **Missing templates**: if neither a local `<project-root>/.agents.md` nor a shared
  template is present/readable, `agents` **exits with a non‑zero status**.
* **Idempotency**: running `agents` with the same inputs (shared template,
  local `.agents.md`, project tree, and env) yields **byte‑identical**
  `AGENTS.md`. Re‑running without changes results in no diff and no rewrite.
* **Determinism**: evaluation is pure with respect to the project tree and the
  current environment; there are no network calls or time‑dependent behaviors.

---


## Community

Want to contribute? Have ideas or feature requests? Come tell us about it on
[Discord](https://discord.gg/fHmRmuBDxF).


---
