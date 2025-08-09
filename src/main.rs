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
#[cfg(test)]
mod test_support;

#[derive(Debug, Parser)]
#[command(
    name = "agentsmd",
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

    /// Suppress default diff output when writing changes
    #[arg(long)]
    quiet: bool,

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
    // Resolve optional shared template path: --template > AGENTS_TEMPLATE > ~/.agents.md
    let template_path_opt = resolve_shared_template_path(&args);

    // Render combined templates; support --stdout and --diff for now.
    let rendered = match render_combined(&root, template_path_opt.as_deref()) {
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
        // Unless --quiet, show diff if there are changes, else "No changes" if nothing to do
        if !args.quiet {
            let current = fs::read_to_string(&agents_path).unwrap_or_default();
            let agents_changed = current != rendered;
            if agents_changed {
                print_unified_diff(&current, &rendered, &agents_path);
            } else {
                // If also writing CLAUDE, and it differs, don't print "No changes"
                if args.claude {
                    let claude_path = agents_path.parent().unwrap_or(&root).join("CLAUDE.md");
                    let claude_current = fs::read_to_string(&claude_path).unwrap_or_default();
                    if claude_current == rendered {
                        println!("{}", "No changes".bright_black());
                    }
                } else {
                    println!("{}", "No changes".bright_black());
                }
            }
        }
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
        return Ok(expand_tilde(root));
    }
    let start = match &args.path {
        Some(p) => expand_tilde(p),
        None => env::current_dir().map_err(|e| error::Error::Root(e.to_string()))?,
    };
    project::project_root(start)
}

fn resolve_shared_template_path(args: &Args) -> Option<PathBuf> {
    if let Some(p) = &args.template {
        return Some(expand_tilde(p));
    }
    if let Ok(envp) = env::var("AGENTS_TEMPLATE") {
        let p = PathBuf::from(envp);
        return Some(expand_tilde(&p));
    }
    if let Ok(home) = env::var("HOME") {
        return Some(PathBuf::from(home).join(".agents.md"));
    }
    None
}

fn render_combined(
    root: &Path,
    shared_template_path: Option<&Path>,
) -> Result<String, error::Error> {
    // Optional project-local template at <root>/.agents.md
    let local_path = root.join(".agents.md");

    // Render local first (if present), then shared. If both paths are the same, render once.
    let mut out = String::new();

    let local_exists = local_path.exists();
    let shared_exists = shared_template_path.map(|p| p.exists()).unwrap_or(false);

    if !local_exists && !shared_exists {
        return Err(error::Error::Root(
            "no template found: neither <project>/.agents.md nor shared template".into(),
        ));
    }

    let same_path = match shared_template_path {
        Some(p) => paths_equal(&local_path, p),
        None => false,
    };

    if local_exists {
        let txt = fs::read_to_string(&local_path).map_err(|e| {
            error::Error::Root(format!(
                "template read error ({}): {e}",
                local_path.display()
            ))
        })?;
        let tpl = template::Template::parse(&txt)?;
        out.push_str(&tpl.render(root)?);
    }

    if let Some(sp) = shared_template_path
        && !same_path
        && sp.exists()
    {
        let txt = fs::read_to_string(sp).map_err(|e| {
            error::Error::Root(format!("template read error ({}): {e}", sp.display()))
        })?;
        let tpl = template::Template::parse(&txt)?;
        out.push_str(&tpl.render(root)?);
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
        Some(p) => {
            let p = expand_tilde(p);
            if p.is_absolute() { p } else { root.join(p) }
        }
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

fn expand_tilde(p: &Path) -> PathBuf {
    let s = p.to_string_lossy();
    PathBuf::from(shellexpand::tilde(&s).into_owned())
}

#[cfg(test)]
mod tests {
    use super::{compute_output_path, render_combined, resolve_shared_template_path};
    use crate::Args;
    use crate::test_support::EnvGuard;
    use std::fs;
    use std::io::Write;
    use std::path::PathBuf;
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
        let out = render_combined(&root, Some(&shared)).unwrap();
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
        let out = render_combined(&root, Some(&shared)).unwrap();
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
        let out = render_combined(&root, Some(&local)).unwrap();
        assert_eq!(out, "OnlyOnce\n");
    }

    #[test]
    fn errors_when_both_templates_missing() {
        let td = TempDir::new().unwrap();
        let root = td.path().to_path_buf();
        fs::create_dir_all(root.join(".git")).unwrap();
        let shared = root.join("nope.md");
        let err = render_combined(&root, Some(&shared)).unwrap_err();
        match err {
            crate::error::Error::Root(msg) => assert!(msg.contains("no template found")),
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn succeeds_with_only_local_present() {
        let td = TempDir::new().unwrap();
        let root = td.path().to_path_buf();
        fs::create_dir_all(root.join(".git")).unwrap();
        let local = root.join(".agents.md");
        write(&local, "LocalOnly\n");
        // Shared path missing
        let shared = root.join("nope.md");
        let out = render_combined(&root, Some(&shared)).unwrap();
        assert_eq!(out, "LocalOnly\n");
    }

    #[test]
    fn tilde_expansion_in_paths() {
        // Set up a fake HOME
        let td = TempDir::new().unwrap();
        let home = td.path().to_path_buf();
        fs::create_dir_all(home.join(".git")).unwrap();
        let home_guard = EnvGuard::new("HOME");
        home_guard.set(&home);

        // ~ in --template
        let args = Args {
            path: None,
            template: Some(PathBuf::from("~/shared.md")),
            root: None,
            stdout: false,
            diff: false,
            quiet: false,
            claude: false,
            out: None,
        };
        let p = resolve_shared_template_path(&args).unwrap();
        assert_eq!(p, home.join("shared.md"));

        // ~ in --out
        let args2 = Args {
            out: Some(PathBuf::from("~/AGENTS.md")),
            ..args
        };
        let out_path = compute_output_path(&args2, td.path());
        assert!(out_path.is_absolute());
        assert_eq!(out_path, home.join("AGENTS.md"));

        // EnvGuard drop restores HOME
    }
}
