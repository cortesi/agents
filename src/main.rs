use std::{
    env, fs,
    path::{Path, PathBuf},
    process,
};

use clap::Parser;
use owo_colors::OwoColorize;
use similar::TextDiff;

mod error;
mod expr;
mod parse;
mod project;
mod template;

#[derive(Debug, Parser)]
#[command(
    name = "agents",
    about = "Render AGENTS.md by combining project and shared templates with simple matchers",
    version
)]
struct Args {
    /// Target project path (defaults to CWD)
    #[arg(value_name = "path")]
    path: Option<PathBuf>,

    /// Override template path (defaults to ~/.agents.md)
    #[arg(long, value_name = "path")]
    template: Option<PathBuf>,

    /// Force project root (skip detection)
    #[arg(long, value_name = "path")]
    root: Option<PathBuf>,

    /// Print to stdout instead of writing AGENTS.md
    #[arg(long)]
    stdout: bool,

    /// Show unified diff of pending changes; do not write
    #[arg(long)]
    diff: bool,

    /// Also write CLAUDE.md alongside AGENTS.md
    #[arg(long)]
    claude: bool,

    /// Override output file path (relative paths are under project root)
    #[arg(long, value_name = "path")]
    out: Option<PathBuf>,
}

fn main() {
    let args = Args::parse();

    let root = match compute_root(&args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            process::exit(1);
        }
    };
    // Resolve shared template path: --template > AGENTS_TEMPLATE > ~/.agents.md
    let template_path = match resolve_template_path(&args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            process::exit(1);
        }
    };

    // Render combined templates; support --stdout and --diff for now.
    let rendered = match render_combined(&root, &template_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{e}");
            process::exit(1);
        }
    };

    if args.diff {
        let target = compute_output_path(&args, &root);
        let current = fs::read_to_string(&target).unwrap_or_default();
        if current == rendered {
            println!("{}", "No changes".bright_black());
            return;
        }
        print_unified_diff(&current, &rendered, &target);
        return;
    }

    if args.stdout {
        print!("{rendered}");
    } else {
        // Write output (and optionally CLAUDE.md)
        let agents_path = compute_output_path(&args, &root);
        if let Err(e) = write_if_changed(&agents_path, &rendered) {
            eprintln!("write error ({}): {e}", agents_path.display());
            process::exit(1);
        }
        if args.claude {
            let dir = agents_path.parent().unwrap_or(&root);
            let claude_path = dir.join("CLAUDE.md");
            if let Err(e) = write_if_changed(&claude_path, &rendered) {
                eprintln!("write error ({}): {e}", claude_path.display());
                process::exit(1);
            }
        }
    }
}

fn compute_root(args: &Args) -> Result<PathBuf, error::Error> {
    if let Some(root) = &args.root {
        return Ok(root.clone());
    }
    let start = match &args.path {
        Some(p) => p.clone(),
        None => env::current_dir().map_err(|e| error::Error::Root(e.to_string()))?,
    };
    project::project_root(start)
}

fn resolve_template_path(args: &Args) -> Result<PathBuf, error::Error> {
    if let Some(p) = &args.template {
        return Ok(p.clone());
    }
    if let Ok(envp) = env::var("AGENTS_TEMPLATE") {
        return Ok(PathBuf::from(envp));
    }
    // Default to ~/.agents.md
    let home = env::var("HOME").map_err(|e| error::Error::Root(e.to_string()))?;
    Ok(PathBuf::from(home).join(".agents.md"))
}

fn render_combined(root: &Path, shared_template_path: &Path) -> Result<String, error::Error> {
    // Optional project-local template at <root>/.agents.md
    let local_path = root.join(".agents.md");

    // Render local first (if present), then shared. If both paths are the same, render once.
    let mut out = String::new();

    let same_path = paths_equal(&local_path, shared_template_path);

    if local_path.exists() {
        let txt = fs::read_to_string(&local_path).map_err(|e| {
            error::Error::Root(format!(
                "template read error ({}): {e}",
                local_path.display()
            ))
        })?;
        let tpl = template::Template::parse(&txt)?;
        out.push_str(&tpl.render(root, None)?);
    }

    if !same_path {
        let txt = fs::read_to_string(shared_template_path).map_err(|e| {
            error::Error::Root(format!(
                "template read error ({}): {e}",
                shared_template_path.display()
            ))
        })?;
        let tpl = template::Template::parse(&txt)?;
        out.push_str(&tpl.render(root, None)?);
    }

    Ok(out)
}

fn paths_equal(a: &Path, b: &Path) -> bool {
    // Compare via absolute components if possible; fall back to direct equality
    let a_abs = a.canonicalize().unwrap_or_else(|_| a.to_path_buf());
    let b_abs = b.canonicalize().unwrap_or_else(|_| b.to_path_buf());
    a_abs == b_abs
}

fn compute_output_path(args: &Args, root: &Path) -> PathBuf {
    match &args.out {
        Some(p) if p.is_absolute() => p.clone(),
        Some(p) => root.join(p),
        None => root.join("AGENTS.md"),
    }
}

fn write_if_changed(path: &Path, contents: &str) -> Result<(), std::io::Error> {
    match fs::read_to_string(path) {
        Ok(existing) if existing == contents => return Ok(()),
        _ => {}
    }
    fs::write(path, contents)
}

fn print_unified_diff(current: &str, rendered: &str, target: &Path) {
    let diff = TextDiff::from_lines(current, rendered)
        .unified_diff()
        .context_radius(3)
        .header(
            &format!(
                "a/{}",
                target.file_name().unwrap_or_default().to_string_lossy()
            ),
            &format!(
                "b/{}",
                target.file_name().unwrap_or_default().to_string_lossy()
            ),
        )
        .to_string();

    for line in diff.lines() {
        if line.starts_with("+++") || line.starts_with("---") {
            println!("{}", line.bold());
        } else if line.starts_with("@@") {
            println!("{}", line.blue());
        } else if line.starts_with('+') {
            println!("{}", line.green());
        } else if line.starts_with('-') {
            println!("{}", line.red());
        } else {
            println!("{line}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::render_combined;
    use std::fs;
    use std::io::Write;
    use tempfile::TempDir;

    fn write(path: &std::path::Path, contents: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut f = fs::File::create(path).unwrap();
        write!(f, "{contents}").unwrap();
    }

    #[test]
    fn combines_local_then_shared() {
        let td = TempDir::new().unwrap();
        let root = td.path().to_path_buf();
        fs::create_dir_all(root.join(".git")).unwrap();
        let local = root.join(".agents.md");
        let shared = root.join("shared.md");
        write(&local, "L\n");
        write(&shared, "S\n");
        let out = render_combined(&root, &shared).unwrap();
        assert_eq!(out, "L\nS\n");
    }

    #[test]
    fn local_is_full_template() {
        let td = TempDir::new().unwrap();
        let root = td.path().to_path_buf();
        fs::create_dir_all(root.join(".git")).unwrap();
        // Local template checks for an existing file
        let local = root.join(".agents.md");
        write(
            &local,
            "Before\n<!-- if exists(\"Cargo.toml\") -->Hit\n<!-- endif -->\nAfter\n",
        );
        // Create Cargo.toml to satisfy exists
        write(&root.join("Cargo.toml"), "[package]\nname=\"x\"\n");
        // Shared template empty
        let shared = root.join("shared.md");
        write(&shared, "");
        let out = render_combined(&root, &shared).unwrap();
        assert!(out.contains("Before\n"));
        assert!(out.contains("Hit\n"));
        assert!(out.contains("After\n"));
    }

    #[test]
    fn same_path_renders_once() {
        let td = TempDir::new().unwrap();
        let root = td.path().to_path_buf();
        fs::create_dir_all(root.join(".git")).unwrap();
        let local = root.join(".agents.md");
        write(&local, "OnlyOnce\n");
        // Use the same path for shared
        let out = render_combined(&root, &local).unwrap();
        assert_eq!(out, "OnlyOnce\n");
    }
}
