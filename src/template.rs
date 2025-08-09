use crate::error::Error;
use crate::expr::Expr;
use std::path::Path;

/// A top‑level template unit: literal text or a conditional block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    Text(String),
    If { cond: Expr, body: Vec<Block> },
}

/// Parsed representation of a template: a linear sequence of blocks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    pub blocks: Vec<Block>,
}

impl Template {
    pub fn parse(input: &str) -> Result<Self, Error> {
        crate::parse::parse_template(input)
    }

    /// Render this template against the given project root.
    ///
    /// Prepends `prefix` verbatim if provided, then appends all literal text
    /// blocks and the bodies of conditional blocks whose expressions evaluate
    /// to true.
    pub fn render(&self, root: &Path, prefix: Option<&str>) -> Result<String, Error> {
        let mut out = String::new();
        if let Some(p) = prefix {
            out.push_str(p);
        }
        render_blocks(&self.blocks, root, &mut out)?;
        Ok(out)
    }
}

fn render_blocks(blocks: &[Block], root: &Path, out: &mut String) -> Result<(), Error> {
    for b in blocks {
        match b {
            Block::Text(s) => out.push_str(s),
            Block::If { cond, body } => {
                if cond.is_match(root)? {
                    render_blocks(body, root, out)?;
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn render_includes_text_and_prefix() {
        let tpl = Template::parse("hello world").unwrap();
        let td = TempDir::new().unwrap();
        fs::create_dir_all(td.path().join(".git")).unwrap();
        let out = tpl.render(td.path(), Some("prefix\n")).unwrap();
        assert!(out.contains("prefix\nhello world"));
    }

    #[test]
    fn render_conditionals_respected() {
        let src = "Before\n<!-- if exists(\"Cargo.toml\") -->\nMatched\n<!-- endif -->\nAfter\n";
        let tpl = Template::parse(src).unwrap();
        let td = TempDir::new().unwrap();
        fs::create_dir_all(td.path().join(".git")).unwrap();
        // No file -> block excluded
        let out1 = tpl.render(td.path(), None).unwrap();
        assert!(out1.contains("Before"));
        assert!(out1.contains("After"));
        assert!(!out1.contains("Matched"));
        // Create file -> block included
        fs::File::create(td.path().join("Cargo.toml")).unwrap();
        let out2 = tpl.render(td.path(), None).unwrap();
        assert!(out2.contains("Matched"));
    }

    #[test]
    fn render_propagates_expr_errors() {
        let tpl = Template::parse("<!-- if exists('{oops') -->x<!-- endif -->").unwrap();
        let td = TempDir::new().unwrap();
        fs::create_dir_all(td.path().join(".git")).unwrap();
        let err = tpl.render(td.path(), None).unwrap_err();
        match err {
            Error::Template(_) => {}
            other => panic!("unexpected error: {other:?}"),
        }
    }
}
