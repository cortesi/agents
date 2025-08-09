use crate::error::Error;
use crate::expr::{Expr, Matcher};
use crate::template::{Block, Template};

pub fn parse_template(input: &str) -> Result<Template, Error> {
    TemplateParser::new(input).parse()
}

struct TemplateParser<'a> {
    src: &'a str,
    idx: usize,
}

impl<'a> TemplateParser<'a> {
    fn new(src: &'a str) -> Self {
        Self { src, idx: 0 }
    }

    fn parse(mut self) -> Result<Template, Error> {
        let mut stack: Vec<(Expr, Vec<Block>)> = Vec::new();
        let mut cur: Vec<Block> = Vec::new();

        while self.idx < self.src.len() {
            if let Some(tag_start) = self.find("<!--") {
                if tag_start > self.idx {
                    let txt = &self.src[self.idx..tag_start];
                    if !txt.is_empty() {
                        cur.push(Block::Text(txt.to_string()))
                    }
                }
                self.idx = tag_start + 4; // after <!--
                self.skip_ws();
                if self.consume_if("if") {
                    self.skip_ws();
                    let expr_str = self.read_until("-->")?;
                    let expr = ExprParser::new(expr_str.trim()).parse_expr()?;
                    self.idx += 3; // -->
                    stack.push((expr, std::mem::take(&mut cur)));
                } else if self.consume_if("endif") {
                    let tail = self.read_until("-->")?;
                    let rest = tail.trim();
                    if !rest.is_empty() {
                        return Err(Error::Template("unexpected content after 'endif'".into()));
                    }
                    self.idx += 3; // -->
                    let (expr, parent) = match stack.pop() {
                        Some(v) => v,
                        None => return Err(Error::Template("stray 'endif'".into())),
                    };
                    let completed = Block::If {
                        cond: expr,
                        body: cur,
                    };
                    cur = parent;
                    cur.push(completed);
                } else {
                    // literal comment
                    let inner = self.read_until("-->")?;
                    self.idx += 3; // -->
                    let mut s = String::from("<!--");
                    s.push_str(inner);
                    s.push_str("-->");
                    cur.push(Block::Text(s));
                }
            } else {
                let txt = &self.src[self.idx..];
                if !txt.is_empty() {
                    cur.push(Block::Text(txt.to_string()))
                }
                self.idx = self.src.len();
            }
        }

        if !stack.is_empty() {
            return Err(Error::Template("unclosed 'if' block".into()));
        }

        Ok(Template { blocks: cur })
    }

    fn find(&self, needle: &str) -> Option<usize> {
        self.src[self.idx..].find(needle).map(|i| self.idx + i)
    }

    fn skip_ws(&mut self) {
        while let Some(ch) = self.peek() {
            if ch.is_whitespace() {
                self.idx += ch.len_utf8();
            } else {
                break;
            }
        }
    }

    fn consume_if(&mut self, s: &str) -> bool {
        if self.src[self.idx..].starts_with(s) {
            self.idx += s.len();
            true
        } else {
            false
        }
    }

    fn read_until(&mut self, delim: &str) -> Result<&'a str, Error> {
        if let Some(pos) = self.src[self.idx..].find(delim) {
            let s = &self.src[self.idx..self.idx + pos];
            self.idx += pos; // advance to the start of the delimiter
            Ok(s)
        } else {
            Err(Error::Template(format!(
                "unterminated tag; missing '{delim}'"
            )))
        }
    }

    fn peek(&self) -> Option<char> {
        self.src[self.idx..].chars().next()
    }
}

struct ExprParser<'a> {
    src: &'a str,
    idx: usize,
}

impl<'a> ExprParser<'a> {
    fn new(src: &'a str) -> Self {
        Self { src, idx: 0 }
    }

    fn parse_expr(mut self) -> Result<Expr, Error> {
        let expr = self.parse_or()?;
        self.skip_ws();
        if self.idx != self.src.len() {
            return Err(Error::Template("trailing characters in expression".into()));
        }
        Ok(expr)
    }

    fn parse_or(&mut self) -> Result<Expr, Error> {
        let mut left = self.parse_and()?;
        loop {
            self.skip_ws();
            if self.consume("||") {
                let right = self.parse_and()?;
                left = Expr::Or(Box::new(left), Box::new(right));
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expr, Error> {
        let mut left = self.parse_not()?;
        loop {
            self.skip_ws();
            if self.consume("&&") {
                let right = self.parse_not()?;
                left = Expr::And(Box::new(left), Box::new(right));
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn parse_not(&mut self) -> Result<Expr, Error> {
        self.skip_ws();
        if self.consume("!") {
            let inner = self.parse_not()?;
            Ok(Expr::Not(Box::new(inner)))
        } else {
            self.parse_primary()
        }
    }

    fn parse_primary(&mut self) -> Result<Expr, Error> {
        self.skip_ws();
        if self.consume("(") {
            let e = self.parse_or()?;
            self.skip_ws();
            if !self.consume(")") {
                return Err(Error::Template("expected ')'".into()));
            }
            return Ok(e);
        }

        if self.consume_ident("exists") {
            let arg = self.parse_paren_string()?;
            return Ok(Expr::Matcher(Matcher::Exists(arg)));
        }
        if self.consume_ident("lang") {
            let arg = self.parse_paren_string()?;
            return Ok(Expr::Matcher(Matcher::Lang(arg)));
        }
        if self.consume_ident("env") {
            self.skip_ws();
            if !self.consume("(") {
                return Err(Error::Template("expected '(' after env".into()));
            }
            self.skip_ws();
            let content = self.read_until(")")?;
            let (name, value_opt) = parse_env_arg(content.trim())?;
            self.consume(")");
            let m = match value_opt {
                Some(v) => Matcher::EnvEquals { name, value: v },
                None => Matcher::EnvExists(name),
            };
            return Ok(Expr::Matcher(m));
        }

        Err(Error::Template("expected matcher or '('".into()))
    }

    fn parse_paren_string(&mut self) -> Result<String, Error> {
        self.skip_ws();
        if !self.consume("(") {
            return Err(Error::Template("expected '('".into()));
        }
        self.skip_ws();
        let s = self.parse_string_like()?;
        self.skip_ws();
        if !self.consume(")") {
            return Err(Error::Template("expected ')'".into()));
        }
        Ok(s)
    }

    fn parse_string_like(&mut self) -> Result<String, Error> {
        self.skip_ws();
        if self.peek() == Some('"') || self.peek() == Some('\'') {
            self.parse_quoted_string()
        } else if self.peek() == Some('r') && self.peek_n(1) == Some('"') {
            self.idx += 1; // skip r
            self.parse_raw_string()
        } else {
            let start = self.idx;
            while let Some(ch) = self.peek() {
                if ch.is_whitespace() || ch == ')' {
                    break;
                }
                self.idx += ch.len_utf8();
            }
            if self.idx == start {
                Err(Error::Template("expected string".into()))
            } else {
                Ok(self.src[start..self.idx].to_string())
            }
        }
    }

    fn parse_quoted_string(&mut self) -> Result<String, Error> {
        let quote = self
            .next()
            .ok_or_else(|| Error::Template("expected quote".into()))?;
        let mut out = String::new();
        while let Some(ch) = self.next() {
            if ch == quote {
                return Ok(out);
            }
            if ch == '\\' {
                if let Some(esc) = self.next() {
                    match esc {
                        'n' => out.push('\n'),
                        'r' => out.push('\r'),
                        't' => out.push('\t'),
                        '\\' => out.push('\\'),
                        '\'' => out.push('\''),
                        '"' => out.push('"'),
                        other => {
                            out.push('\\');
                            out.push(other);
                        }
                    }
                    continue;
                } else {
                    return Err(Error::Template("unterminated escape".into()));
                }
            }
            out.push(ch);
        }
        Err(Error::Template("unterminated string".into()))
    }

    fn parse_raw_string(&mut self) -> Result<String, Error> {
        if !self.consume("\"") {
            return Err(Error::Template(r#"expected '"' after r"#.into()));
        }
        let start = self.idx;
        while let Some(ch) = self.next() {
            if ch == '"' {
                return Ok(self.src[start..self.idx - 1].to_string());
            }
        }
        Err(Error::Template("unterminated raw string".into()))
    }

    fn consume_ident(&mut self, ident: &str) -> bool {
        let mut i = self.idx;
        for ch in ident.chars() {
            if self.src[i..].starts_with(ch) {
                i += ch.len_utf8();
            } else {
                return false;
            }
        }
        if let Some(next) = self.src[i..].chars().next()
            && (next.is_alphanumeric() || next == '_')
        {
            return false;
        }
        self.idx = i;
        true
    }

    fn read_until(&mut self, delim: &str) -> Result<&'a str, Error> {
        if let Some(pos) = self.src[self.idx..].find(delim) {
            let s = &self.src[self.idx..self.idx + pos];
            self.idx += pos;
            Ok(s)
        } else {
            Err(Error::Template(format!("missing '{delim}' in expression")))
        }
    }

    fn consume(&mut self, s: &str) -> bool {
        if self.src[self.idx..].starts_with(s) {
            self.idx += s.len();
            true
        } else {
            false
        }
    }

    fn skip_ws(&mut self) {
        while let Some(ch) = self.peek() {
            if ch.is_whitespace() {
                self.idx += ch.len_utf8();
            } else {
                break;
            }
        }
    }
    fn peek(&self) -> Option<char> {
        self.src[self.idx..].chars().next()
    }
    fn peek_n(&self, n: usize) -> Option<char> {
        self.src[self.idx..].chars().nth(n)
    }
    fn next(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.idx += ch.len_utf8();
        Some(ch)
    }
}

fn parse_env_arg(s: &str) -> Result<(String, Option<String>), Error> {
    let s = s.trim();
    if s.is_empty() {
        return Err(Error::Template("empty env() argument".into()));
    }
    let (name, mut i) = parse_token(s)?;
    if name.is_empty() {
        return Err(Error::Template("empty env var name".into()));
    }
    while let Some(ch) = s[i..].chars().next() {
        if ch.is_whitespace() {
            i += ch.len_utf8();
        } else {
            break;
        }
    }
    if i >= s.len() || !s[i..].starts_with('=') {
        return Ok((name, None));
    }
    i += 1;
    while let Some(ch) = s[i..].chars().next() {
        if ch.is_whitespace() {
            i += ch.len_utf8();
        } else {
            break;
        }
    }
    let (value, _end) = parse_token(&s[i..])?;
    Ok((name, Some(value)))
}

fn parse_token(s: &str) -> Result<(String, usize), Error> {
    if s.is_empty() {
        return Ok((String::new(), 0));
    }
    let mut ep = ExprParser::new(s);
    if ep.peek() == Some('"') || ep.peek() == Some('\'') {
        let v = ep.parse_quoted_string()?;
        return Ok((v, ep.idx));
    }
    if ep.peek() == Some('r') && ep.peek_n(1) == Some('"') {
        ep.next();
        let v = ep.parse_raw_string()?;
        return Ok((v, ep.idx));
    }
    let mut i = 0usize;
    for ch in s.chars() {
        if ch.is_whitespace() || ch == '=' || ch == ')' {
            break;
        }
        i += ch.len_utf8();
    }
    Ok((s[..i].to_string(), i))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    enum Check {
        BlocksLen(usize),
        HasMatcher(Matcher),
        HasText(&'static str),
    }

    fn ast_contains_matcher(blocks: &[Block], target: &Matcher) -> bool {
        for b in blocks {
            match b {
                Block::Text(_) => {}
                Block::If { cond, body } => {
                    if expr_contains_matcher(cond, target) {
                        return true;
                    }
                    if ast_contains_matcher(body, target) {
                        return true;
                    }
                }
            }
        }
        false
    }

    fn expr_contains_matcher(expr: &Expr, target: &Matcher) -> bool {
        match expr {
            Expr::Matcher(m) => m == target,
            Expr::And(a, b) | Expr::Or(a, b) => {
                expr_contains_matcher(a, target) || expr_contains_matcher(b, target)
            }
            Expr::Not(e) => expr_contains_matcher(e, target),
        }
    }

    fn ast_contains_text(blocks: &[Block], needle: &str) -> bool {
        for b in blocks {
            match b {
                Block::Text(s) => {
                    if s.contains(needle) {
                        return true;
                    }
                }
                Block::If { body, .. } => {
                    if ast_contains_text(body, needle) {
                        return true;
                    }
                }
            }
        }
        false
    }

    fn validate(template: &Template, checks: &[Check]) {
        for c in checks {
            match c {
                Check::BlocksLen(n) => assert_eq!(template.blocks.len(), *n),
                Check::HasMatcher(m) => assert!(ast_contains_matcher(&template.blocks, m)),
                Check::HasText(s) => assert!(ast_contains_text(&template.blocks, s)),
            }
        }
    }

    #[test]
    fn test_parse_success() {
        struct Case {
            name: &'static str,
            input: &'static str,
            checks: Vec<Check>,
        }
        let cases = vec![
            Case {
                name: "plain text",
                input: "Hello world",
                checks: vec![Check::BlocksLen(1), Check::HasText("Hello world")],
            },
            Case {
                name: "simple if exists",
                input: "<!-- if exists(\"Cargo.toml\") -->\nRun cargo build\n<!-- endif -->",
                checks: vec![
                    Check::BlocksLen(1),
                    Check::HasMatcher(Matcher::Exists("Cargo.toml".into())),
                    Check::HasText("Run cargo build"),
                ],
            },
            Case {
                name: "nested if",
                input: "<!-- if env(CI) -->\nA\n<!-- if exists('src/**') -->B<!-- endif -->\n<!-- endif -->",
                checks: vec![
                    Check::BlocksLen(1),
                    Check::HasMatcher(Matcher::EnvExists("CI".into())),
                    Check::HasMatcher(Matcher::Exists("src/**".into())),
                    Check::HasText("B"),
                ],
            },
            Case {
                name: "complex expr",
                input: "<!-- if env(CI) && !env(NODE_ENV=\"production\") || exists(r\"**/*.rs\") -->x<!-- endif -->",
                checks: vec![Check::BlocksLen(1), Check::HasText("x")],
            },
            Case {
                name: "note comments are preserved in template",
                input: "<!-- note:\nInternal only\n-->\nVisible text\n",
                checks: vec![
                    Check::HasText("<!--"),
                    Check::HasText("note:"),
                    Check::HasText("Internal only"),
                    Check::HasText("Visible text"),
                ],
            },
        ];

        for c in cases {
            let tpl = parse_template(c.input).unwrap_or_else(|e| panic!("{}: {e}", c.name));
            validate(&tpl, &c.checks);
        }
    }

    #[test]
    fn test_parse_error() {
        struct ErrCase {
            name: &'static str,
            input: &'static str,
            contains: &'static str,
        }
        let cases = vec![
            ErrCase {
                name: "unmatched endif",
                input: "oops <!-- endif -->",
                contains: "stray",
            },
            ErrCase {
                name: "unclosed if",
                input: "<!-- if env(CI) -->",
                contains: "unclosed",
            },
        ];
        for c in cases {
            let err = parse_template(c.input).unwrap_err();
            match err {
                Error::Template(msg) => assert!(msg.contains(c.contains), "{}: {msg}", c.name),
                other => panic!("{}: unexpected error {other:?}", c.name),
            }
        }
    }
}
