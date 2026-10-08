//! Inline suppression comments, compatible with Ruff and pylint:
//!
//! - `# noqa` / `# noqa: C8101, print-used` suppress on that line;
//! - `# pylint: disable=...` after code suppresses on that line;
//! - `# pylint: disable=...` on its own line suppresses until the end of the
//!   enclosing block (until the end of the file at column 0);
//! - `# pylint: disable-next=...` suppresses on the next line;
//! - `# pylint: enable=...` ends an earlier block-level `disable`.
//!
//! Rules are matched by code or by name, case-insensitively; `all` matches
//! every rule.

use ruff_python_ast::token::{TokenKind, Tokens};
use ruff_source_file::LineIndex;
use ruff_text_size::Ranged;

#[derive(Debug, Default)]
pub struct Suppressions {
    /// `(first line, last line, rules)`; an empty rule list means "all".
    ranges: Vec<(usize, usize, Vec<String>)>,
}

#[derive(Debug, PartialEq)]
enum Directive {
    Noqa(Vec<String>),
    Disable(Vec<String>),
    DisableNext(Vec<String>),
    Enable(Vec<String>),
}

impl Suppressions {
    pub fn from_tokens(source: &str, tokens: &Tokens, line_index: &LineIndex) -> Self {
        let lines: Vec<&str> = source.lines().collect();
        let mut suppressions = Self::default();
        for token in tokens.iter().filter(|t| t.kind() == TokenKind::Comment) {
            let comment = &source[token.range()];
            let line = line_index.line_index(token.start()).get();
            let line_text = lines.get(line - 1).copied().unwrap_or("");
            let indent = indentation(line_text);
            let standalone = line_text.trim_start().starts_with('#');
            for directive in parse_comment(comment) {
                suppressions.apply(directive, line, standalone, indent, &lines);
            }
        }
        suppressions
    }

    fn apply(&mut self, directive: Directive, line: usize, standalone: bool, indent: usize, lines: &[&str]) {
        match directive {
            Directive::Noqa(rules) => self.ranges.push((line, line, rules)),
            Directive::DisableNext(rules) => self.ranges.push((line + 1, line + 1, rules)),
            Directive::Disable(rules) if !standalone => self.ranges.push((line, line, rules)),
            Directive::Disable(rules) => {
                let end = block_end(lines, line, indent);
                self.ranges.push((line, end, rules));
            }
            Directive::Enable(rules) => {
                for (start, end, disabled) in &mut self.ranges {
                    let open_here = *start < line && line <= *end;
                    if open_here && (rules.iter().any(|r| r == "all") || disabled.iter().any(|d| rules.contains(d))) {
                        *end = line - 1;
                    }
                }
            }
        }
    }

    pub fn is_suppressed(&self, line: usize, code: &str, name: &str) -> bool {
        let (code, name) = (code.to_ascii_lowercase(), name.to_ascii_lowercase());
        self.ranges.iter().any(|(start, end, rules)| {
            *start <= line
                && line <= *end
                && (rules.is_empty() || rules.iter().any(|r| r == "all" || *r == code || *r == name))
        })
    }
}

fn indentation(line: &str) -> usize {
    line.chars()
        .take_while(|c| c.is_whitespace())
        .map(|c| if c == '\t' { 8 } else { 1 })
        .sum()
}

/// Last line of the block a standalone comment at `line` (1-based) belongs to.
fn block_end(lines: &[&str], line: usize, indent: usize) -> usize {
    if indent == 0 {
        return usize::MAX;
    }
    for (i, text) in lines.iter().enumerate().skip(line) {
        let trimmed = text.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if indentation(text) < indent {
            return i; // 0-based index of the dedented line == 1-based previous line
        }
    }
    lines.len()
}

fn split_rules(list: &str) -> Vec<String> {
    list.split(',')
        .map(|r| r.trim().to_ascii_lowercase())
        .filter(|r| !r.is_empty())
        .collect()
}

fn parse_comment(comment: &str) -> Vec<Directive> {
    let mut directives = Vec::new();
    // A comment may hold several `#`-separated parts: `# pylint: disable=x  # noqa`.
    for part in comment.split('#').map(str::trim).filter(|p| !p.is_empty()) {
        let lower = part.to_ascii_lowercase();
        if let Some(rest) = lower.strip_prefix("noqa") {
            let rest = rest.trim_start();
            match rest.strip_prefix(':') {
                Some(list) => directives.push(Directive::Noqa(split_rules(list))),
                None if rest.is_empty() => directives.push(Directive::Noqa(Vec::new())),
                None => {}
            }
        } else if let Some(rest) = lower.strip_prefix("pylint:") {
            for clause in rest.split(';') {
                let Some((key, value)) = clause.split_once('=') else {
                    continue;
                };
                let rules = split_rules(value);
                match key.trim() {
                    "disable" => directives.push(Directive::Disable(rules)),
                    "disable-next" => directives.push(Directive::DisableNext(rules)),
                    "enable" => directives.push(Directive::Enable(rules)),
                    _ => {}
                }
            }
        }
    }
    directives
}

#[cfg(test)]
mod tests {
    use super::*;
    use ruff_python_parser::parse_module;

    fn suppressions(src: &str) -> Suppressions {
        let parsed = parse_module(src).unwrap();
        Suppressions::from_tokens(src, parsed.tokens(), &LineIndex::from_source_text(src))
    }

    #[test]
    fn parses_directives() {
        assert_eq!(parse_comment("# noqa"), vec![Directive::Noqa(vec![])]);
        assert_eq!(
            parse_comment("# noqa: C8101, Print-Used"),
            vec![Directive::Noqa(vec!["c8101".into(), "print-used".into()])]
        );
        assert_eq!(parse_comment("# noqanot"), vec![]);
        assert_eq!(
            parse_comment("# pylint: disable=print-used,W8138  # noqa"),
            vec![
                Directive::Disable(vec!["print-used".into(), "w8138".into()]),
                Directive::Noqa(vec![])
            ]
        );
        assert_eq!(
            parse_comment("# pylint: disable-next=sql-injection"),
            vec![Directive::DisableNext(vec!["sql-injection".into()])]
        );
    }

    #[test]
    fn line_level() {
        let s = suppressions("print(1)  # noqa: W8116\nprint(2)  # pylint: disable=print-used\nprint(3)\n");
        assert!(s.is_suppressed(1, "W8116", "print-used"));
        assert!(s.is_suppressed(2, "W8116", "print-used"));
        assert!(!s.is_suppressed(3, "W8116", "print-used"));
        assert!(!s.is_suppressed(1, "W8138", "except-pass"));
    }

    #[test]
    fn bare_noqa_and_all() {
        let s = suppressions("x = 1  # noqa\ny = 2  # pylint: disable=all\n");
        assert!(s.is_suppressed(1, "C8101", "anything"));
        assert!(s.is_suppressed(2, "E8103", "sql-injection"));
    }

    #[test]
    fn disable_next() {
        let s = suppressions("# pylint: disable-next=print-used\nprint(1)\nprint(2)\n");
        assert!(s.is_suppressed(2, "W8116", "print-used"));
        assert!(!s.is_suppressed(3, "W8116", "print-used"));
    }

    #[test]
    fn block_level_until_dedent() {
        let src = "\
def f():
    # pylint: disable=print-used
    print(1)

    print(2)
def g():
    print(3)
";
        let s = suppressions(src);
        assert!(s.is_suppressed(3, "W8116", "print-used"));
        assert!(s.is_suppressed(5, "W8116", "print-used"));
        assert!(!s.is_suppressed(7, "W8116", "print-used"));
    }

    #[test]
    fn module_level_and_enable() {
        let src = "\
# pylint: disable=print-used
print(1)
# pylint: enable=print-used
print(2)
";
        let s = suppressions(src);
        assert!(s.is_suppressed(2, "W8116", "print-used"));
        assert!(!s.is_suppressed(4, "W8116", "print-used"));
    }
}
