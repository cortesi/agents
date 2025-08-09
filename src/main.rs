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
    about = "Render AGENTS.md from a shared template with simple matchers",
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
    // Resolve template path: --template > AGENTS_TEMPLATE > ~/.agents.md
    let template_path = match resolve_template_path(&args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            process::exit(1);
        }
    };

    // Read and parse template; stub prints the parsed structure
    let tpl_text = match fs::read_to_string(&template_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("template read error ({}): {}", template_path.display(), e);
            process::exit(1);
        }
    };

    let tpl = match template::Template::parse(&tpl_text) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{e}");
            process::exit(1);
        }
    };

    // Load optional prefix and strip maintainer-only notes.
    let prefix = match load_prefix(&root) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            process::exit(1);
        }
    };

    // Render template; support --stdout and --diff for now.
    let rendered = match tpl.render(&root, prefix.as_deref()) {
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

fn load_prefix(root: &Path) -> Result<Option<String>, error::Error> {
    let path = root.join(".agents-prefix.md");
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| error::Error::Root(format!("prefix read error ({}): {e}", path.display())))?;
    Ok(Some(strip_maintainer_notes(&raw)))
}

fn strip_maintainer_notes(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_code = false;
    let mut in_note_comment = false;

    for mut line in input.split_inclusive('\n') {
        // Handle code fences (``` or ~~~) — do not strip inside.
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_code = !in_code;
        }
        if in_code {
            out.push_str(line);
            continue;
        }

        let mut buf = String::with_capacity(line.len());
        let mut i = 0usize;
        while i < line.len() {
            if in_note_comment {
                if let Some(end) = line[i..].find("-->") {
                    i += end + 3; // skip -->
                    in_note_comment = false;
                    continue;
                } else {
                    // Consume rest of line
                    break;
                }
            }

            // Strip HTML note comments starting with <!-- note:
            if let Some(start) = line[i..].find("<!--") {
                let start_abs = i + start;
                // Push any text before the comment
                buf.push_str(&line[i..start_abs]);
                let after = &line[start_abs + 4..];
                let after_trim = after.trim_start();
                if after_trim.starts_with("note:") {
                    // Start stripping
                    if let Some(end) = after.find("-->") {
                        // Strip within same line
                        i = start_abs + 4 + end + 3; // move after -->
                        continue;
                    } else {
                        // Multiline comment continues
                        in_note_comment = true;
                        break;
                    }
                } else {
                    // Not a maintainer note; keep the literal comment
                    buf.push_str("<!--");
                    i = start_abs + 4;
                    continue;
                }
            }

            // Strip single-line [//]: <> (note: ...)
            if let Some(pos) = line[i..].find("[//]: <> (note:") {
                let pos_abs = i + pos;
                buf.push_str(&line[i..pos_abs]);
                // Drop through end of the line segment (up to next ')', but it's safe to drop to EOL)
                // Find closing ')'
                if let Some(close) = line[pos_abs..].find(')') {
                    i = pos_abs + close + 1;
                } else {
                    i = line.len();
                }
                continue;
            }

            // No special patterns; copy rest and break
            buf.push_str(&line[i..]);
            i = line.len();
        }
        line = &buf;
        out.push_str(line);
    }
    out
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
    use super::strip_maintainer_notes;

    #[test]
    fn strip_inline_html_note() {
        let input = "# Title\n<!-- note: remove me -->\nBody\n";
        let out = strip_maintainer_notes(input);
        assert!(out.contains("# Title\n"));
        assert!(out.contains("Body\n"));
        assert!(!out.contains("note:"));
        assert!(!out.contains("<!--"));
    }

    #[test]
    fn strip_multiline_html_note_block() {
        let input = "Intro\n<!-- note:\nInternal\nNotes\n-->\nAfter\n";
        let out = strip_maintainer_notes(input);
        assert!(out.starts_with("Intro\n"));
        assert!(out.ends_with("After\n"));
        assert!(!out.contains("Internal"));
        assert!(!out.contains("note:"));
    }

    #[test]
    fn strip_bracket_note_syntax() {
        let input = "A\n[//]: <> (note: hidden)\nB\n";
        let out = strip_maintainer_notes(input);
        assert!(out.contains("A\n"));
        assert!(out.contains("B\n"));
        assert!(!out.contains("note:"));
        assert!(!out.contains("[//]: <>"));
    }

    #[test]
    fn preserve_notes_inside_backtick_fences() {
        let input = "```md\n<!-- note: keep -->\n```\n";
        let out = strip_maintainer_notes(input);
        assert_eq!(out, input);
    }

    #[test]
    fn preserve_notes_inside_tilde_fences() {
        let input = "~~~\n[//]: <> (note: keep)\n~~~\n";
        let out = strip_maintainer_notes(input);
        assert_eq!(out, input);
    }

    #[test]
    fn handle_crlf_line_endings() {
        let input = "Head\r\n<!-- note: gone -->\r\nTail\r\n";
        let out = strip_maintainer_notes(input);
        assert!(out.contains("Head\r\n"));
        assert!(out.contains("Tail\r\n"));
        assert!(!out.contains("note:"));
    }
}
