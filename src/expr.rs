use crate::error::Error;
use globset::{GlobBuilder, GlobSetBuilder};
use ignore::WalkBuilder;
use std::collections::HashSet;
use std::env;
use std::path::Path;

/// Primitive conditions available in the template language.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Matcher {
    Exists(String),
    EnvExists(String),
    EnvEquals { name: String, value: String },
    Lang(String),
}

/// Boolean expression AST built from matchers and logical operators.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    Matcher(Matcher),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Not(Box<Expr>),
}

impl Expr {
    /// Evaluate this expression against a project at `root`.
    pub fn is_match(&self, root: &Path) -> Result<bool, Error> {
        match self {
            Expr::Matcher(m) => match m {
                Matcher::Exists(pattern) => exists_match(root, pattern),
                Matcher::EnvExists(name) => {
                    Ok(env::var(name).map(|v| !v.is_empty()).unwrap_or(false))
                }
                Matcher::EnvEquals { name, value } => {
                    Ok(env::var(name).map(|v| v == *value).unwrap_or(false))
                }
                Matcher::Lang(name) => lang_match(root, name),
            },
            Expr::And(a, b) => Ok(a.is_match(root)? && b.is_match(root)?),
            Expr::Or(a, b) => Ok(a.is_match(root)? || b.is_match(root)?),
            Expr::Not(e) => Ok(!e.is_match(root)?),
        }
    }
}

fn exists_match(root: &Path, pattern: &str) -> Result<bool, Error> {
    let glob = GlobBuilder::new(pattern)
        .case_insensitive(false)
        .build()
        .map_err(|e| Error::Template(format!("invalid exists() pattern: {e}")))?;
    let mut gsb = GlobSetBuilder::new();
    gsb.add(glob);
    let gs = gsb
        .build()
        .map_err(|e| Error::Template(format!("glob build failed: {e}")))?;

    let mut wb = WalkBuilder::new(root);
    wb.hidden(false)
        .parents(false)
        .follow_links(false)
        .git_ignore(true)
        .git_exclude(true)
        .git_global(true);

    for dent in wb.build() {
        let dent = match dent {
            Ok(d) => d,
            Err(_) => continue,
        };
        let ft = match dent.file_type() {
            Some(t) => t,
            None => continue,
        };
        if ft.is_file() {
            let path = dent.path();
            let rel = path.strip_prefix(root).unwrap_or(path);
            if gs.is_match(rel) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn lang_match(root: &Path, name: &str) -> Result<bool, Error> {
    let lang = match languages::from_name(name) {
        Some(l) => l,
        None => return Err(Error::Template(format!("unknown language: {name}"))),
    };
    let mut exts: HashSet<String> = HashSet::new();
    if let Some(list) = lang.extensions {
        for e in list {
            let trimmed = e.strip_prefix('.').unwrap_or(e).to_ascii_lowercase();
            if !trimmed.is_empty() {
                exts.insert(trimmed);
            }
        }
    }
    if exts.is_empty() {
        return Ok(false);
    }

    let mut wb = WalkBuilder::new(root);
    wb.hidden(false)
        .parents(false)
        .follow_links(false)
        .git_ignore(true)
        .git_exclude(true)
        .git_global(true);

    for dent in wb.build() {
        let dent = match dent {
            Ok(d) => d,
            Err(_) => continue,
        };
        let ft = match dent.file_type() {
            Some(t) => t,
            None => continue,
        };
        if ft.is_file() {
            let path = dent.path();
            if let Some(ext) = path.extension().and_then(|s| s.to_str())
                && exts.contains(&ext.to_ascii_lowercase())
            {
                return Ok(true);
            }
        }
    }

    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use tempfile::TempDir;

    fn touch(path: &std::path::Path) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::File::create(path).unwrap();
    }

    fn write(path: &std::path::Path, contents: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut f = fs::File::create(path).unwrap();
        writeln!(f, "{contents}").unwrap();
    }

    fn setup(files: &[&str]) -> (TempDir, std::path::PathBuf) {
        let td = TempDir::new().unwrap();
        let root_buf = td.path().to_path_buf();
        fs::create_dir_all(root_buf.join(".git")).unwrap();
        for f in files {
            touch(&root_buf.join(f));
        }
        (td, root_buf)
    }

    #[test]
    fn exists_supports_common_patterns() {
        struct Case {
            name: &'static str,
            files: &'static [&'static str],
            expr: Expr,
            expect: bool,
        }
        let cases = vec![
            Case {
                name: "simple true",
                files: &["Cargo.toml"],
                expr: Expr::Matcher(Matcher::Exists("Cargo.toml".into())),
                expect: true,
            },
            Case {
                name: "simple false",
                files: &["Cargo.toml"],
                expr: Expr::Matcher(Matcher::Exists("README.md".into())),
                expect: false,
            },
            Case {
                name: "recursive glob",
                files: &["src/main.rs"],
                expr: Expr::Matcher(Matcher::Exists("**/*.rs".into())),
                expect: true,
            },
            Case {
                name: "brace alternation",
                files: &["src/lib.rs"],
                expr: Expr::Matcher(Matcher::Exists("src/**/{main,lib}.rs".into())),
                expect: true,
            },
        ];

        for c in cases {
            let (_td, root) = setup(c.files);
            let got = c.expr.is_match(&root).unwrap();
            assert_eq!(got, c.expect, "case: {}", c.name);
        }
    }

    #[test]
    fn lang_matches_rust() {
        let (_td, root) = setup(&["src/lib.rs"]);
        let e = Expr::Matcher(Matcher::Lang("rust".into()));
        assert!(e.is_match(&root).unwrap());
    }

    #[test]
    fn lang_unknown_errors() {
        let (_td, root) = setup(&[]);
        let e = Expr::Matcher(Matcher::Lang("definitely-not-a-language".into()));
        let err = e.is_match(&root).unwrap_err();
        match err {
            Error::Template(msg) => assert!(msg.contains("unknown language")),
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn exists_ignores_dotgitignore() {
        let (_td, root) = setup(&[]);
        write(&root.join(".gitignore"), "*.log\n");
        touch(&root.join("app.log"));
        let e = Expr::Matcher(Matcher::Exists("**/*.log".into()));
        assert!(!e.is_match(&root).unwrap());
    }

    #[test]
    fn exists_directories_do_not_match() {
        let (_td, root) = setup(&[]);
        fs::create_dir_all(root.join("src")).unwrap();
        let e = Expr::Matcher(Matcher::Exists("src".into()));
        assert!(!e.is_match(&root).unwrap());
    }

    #[test]
    fn env_exists_and_equals() {
        struct Case {
            name: &'static str,
            set: Option<&'static str>,
            expr: Expr,
            expect: bool,
        }
        let key = "AGENTS_TEST_ENV_TABLE";
        let cases = vec![
            Case {
                name: "unset => exists false",
                set: None,
                expr: Expr::Matcher(Matcher::EnvExists(key.into())),
                expect: false,
            },
            Case {
                name: "empty => exists false",
                set: Some(""),
                expr: Expr::Matcher(Matcher::EnvExists(key.into())),
                expect: false,
            },
            Case {
                name: "non-empty => exists true",
                set: Some("value"),
                expr: Expr::Matcher(Matcher::EnvExists(key.into())),
                expect: true,
            },
            Case {
                name: "equals match",
                set: Some("value"),
                expr: Expr::Matcher(Matcher::EnvEquals {
                    name: key.into(),
                    value: "value".into(),
                }),
                expect: true,
            },
            Case {
                name: "equals mismatch",
                set: Some("value"),
                expr: Expr::Matcher(Matcher::EnvEquals {
                    name: key.into(),
                    value: "other".into(),
                }),
                expect: false,
            },
        ];

        let (_td, root) = setup(&[]);
        for c in cases {
            match c.set {
                Some(v) => unsafe { std::env::set_var(key, v) },
                None => unsafe { std::env::remove_var(key) },
            }
            let got = c.expr.is_match(&root).unwrap();
            assert_eq!(got, c.expect, "case: {}", c.name);
        }
    }

    #[test]
    fn boolean_ops_and_precedence() {
        let (_td, root) = setup(&["a.txt"]);
        let exists_a = Expr::Matcher(Matcher::Exists("a.txt".into()));
        let exists_b = Expr::Matcher(Matcher::Exists("b.txt".into()));
        unsafe { std::env::remove_var("FOO") };
        let expr = Expr::Or(
            Box::new(Expr::And(
                Box::new(exists_a.clone()),
                Box::new(Expr::Matcher(Matcher::EnvExists("FOO".into()))),
            )),
            Box::new(Expr::Not(Box::new(exists_b.clone()))),
        );
        assert!(expr.is_match(&root).unwrap());
        touch(&root.join("b.txt"));
        assert!(!expr.is_match(&root).unwrap());
        unsafe { std::env::set_var("FOO", "1") };
        assert!(expr.is_match(&root).unwrap());
    }

    #[test]
    fn invalid_glob_reports_error() {
        let (_td, root) = setup(&[]);
        let e = Expr::Matcher(Matcher::Exists("{foo".into()));
        let err = e.is_match(&root).unwrap_err();
        match err {
            Error::Template(msg) => assert!(
                msg.contains("invalid exists() pattern") || msg.contains("glob build failed")
            ),
            other => panic!("unexpected error: {other:?}"),
        }
    }
}
