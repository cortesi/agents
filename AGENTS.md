# Project Overview

Agents is a tiny CLI that renders a project-local `AGENTS.md` from a shared
template (`~/.agents.md`). It detects the project root, evaluates simple
filesystem and environment matchers (e.g., `exists("**/*.rs")`, `env(CI)`), and
produces deterministic, idempotent output. Run `agents` in your project or pass
a path.



<rust>
<general_guidelines>
- The current Rust edition is 2024
- Never include code examples in doc comments
- Never use std library paths directly in code - always add a `use` declaration
  at the top of the file
</general_guidelines>

<linting>
Lint before committing and fix all warnings. Use the following command:

```bash
cargo clippy -q --fix --all-targets --all-features --allow-dirty --tests --examples 2>&1
```

Clippy prints warnings on stderr. To see all output in this interface,
we merge stderr into stdout. Fix all remaining warnings manually.
</linting>

<formatting>
Ensure the code is formatted according to Rust standards. Use the following command:

```bash
cargo fmt --all 
```
</formatting>

<testing>
When changes are complete, use the following script to run all tests:

```bash
cargo test --all
```
</testing>

<dependencies>
Add dependencies with the `cargo add` command - try to avoid directly
editing the `Cargo.toml` file to add dependencies.

```bash
cargo add mycrate
```
</dependencies>

<ruskel>
**ruskel** returns a Rust skeleton that shows the API of any item with
implementation bodies stripped. Useful for models that need to look up names,
signatures, derives, APIs, and doc‑comments while writing or reviewing Rust
code. An item can be a crate, module, struct, trait, function, or any other
Rust entity that can be referred to with a Rust path.

Use this tool when:
- You need to look up a function/trait/struct signature.
- You want an overview of a public or private API.
- The user asks for examples or docs from a crate.

<ruskel_tips>
- Request deep module paths (e.g. `tokio::sync::mpsc`) to keep the reply below
  your token budget.
- Pass the `--private` flag to include non‑public items. Useful if you're
  looking up details of items in the current codebase for development.
</ruskel_tips>

```bash
# Current project
ruskel

# A trait in the standard library
ruskel std::io::Read

# If we're in a workspace and we have a crate mycrate
ruskel mycrate

# A method on a struct in the current crate
ruskel mycrate::Struct::method

# A dependency of the current project, else we fetch from crates.io
ruskel serde

# A sub-path within a crate
ruskel serde::de::Deserialize 

# Path to a crate
ruskel /my/path

# A module within that crate
ruskel /my/path::foo

# A crate from crates.io with a specific version
ruskel serde@1.0.0
```
</ruskel>

</rust>




