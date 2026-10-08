//! A port of CPython's `textwrap.wrap` (Lib/textwrap.py, PSF license) for the
//! options polib uses, so PO files wrap exactly as polib writes them.
//!
//! Lengths are in characters (code points), as in Python.

use fancy_regex::Regex;
use std::sync::LazyLock;

/// `textwrap._whitespace`.
const WHITESPACE: &[char] = &['\t', '\n', '\x0b', '\x0c', '\r', ' '];

/// `TextWrapper.wordsep_re`.
static WORDSEP_RE: LazyLock<Regex> = LazyLock::new(|| {
    let ws = r"[\t\n\x0b\x0c\r ]";
    let nws = r"[^\t\n\x0b\x0c\r ]";
    let wp = r#"[\w!"'&.,?]"#;
    let lt = r"[^\d\W]";
    let pattern = format!(
        r"({ws}+|(?<={wp})-{{2,}}(?=\w)|{nws}+?(?:-(?:(?<={lt}{{2}}-)|(?<={lt}-{lt}-))(?={lt}-?{lt})|(?={ws}|\z)|(?<={wp})(?=-{{2,}}\w)))"
    );
    Regex::new(&pattern).expect("valid wordsep regex")
});

pub struct Options<'a> {
    pub width: usize,
    pub initial_indent: &'a str,
    pub subsequent_indent: &'a str,
    pub drop_whitespace: bool,
}

impl Default for Options<'_> {
    fn default() -> Self {
        Self {
            width: 70,
            initial_indent: "",
            subsequent_indent: "",
            drop_whitespace: true,
        }
    }
}

fn char_len(s: &str) -> usize {
    s.chars().count()
}

/// `str.expandtabs(8)` followed by replacing whitespace with spaces.
fn munge_whitespace(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut column = 0;
    for c in text.chars() {
        match c {
            '\t' => {
                let spaces = 8 - (column % 8);
                out.extend(std::iter::repeat_n(' ', spaces));
                column += spaces;
            }
            '\n' | '\r' => {
                out.push(' ');
                column = 0;
            }
            c if WHITESPACE.contains(&c) => {
                out.push(' ');
                column += 1;
            }
            c => {
                out.push(c);
                column += 1;
            }
        }
    }
    out
}

/// `wordsep_re.split(text)` without empty strings. The regex only splits
/// runs of non-whitespace at hyphens, and whitespace is neither `\w` nor
/// punctuation for its lookarounds, so runs are split on their own and the
/// slow regex only sees runs containing `-`.
fn split_chunks(text: &str) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut start = 0;
    let mut in_space = None;
    for (i, c) in text.char_indices() {
        let space = WHITESPACE.contains(&c);
        if in_space.is_some_and(|s| s != space) {
            push_run(&mut chunks, &text[start..i]);
            start = i;
        }
        in_space = Some(space);
    }
    push_run(&mut chunks, &text[start..]);
    chunks
}

fn push_run(chunks: &mut Vec<String>, run: &str) {
    if run.is_empty() {
        return;
    }
    if run.contains('-') && !run.starts_with(WHITESPACE) {
        chunks.extend(split_chunks_regex(run));
    } else {
        chunks.push(run.to_string());
    }
}

/// `wordsep_re.split(text)` without empty strings: the chunks matched by the
/// regex and the text between matches.
fn split_chunks_regex(text: &str) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut last = 0;
    let mut position = 0;
    while position <= text.len() {
        let Ok(Some(found)) = WORDSEP_RE.find_from_pos(text, position) else {
            break;
        };
        if found.start() > last {
            chunks.push(text[last..found.start()].to_string());
        }
        if !found.as_str().is_empty() {
            chunks.push(found.as_str().to_string());
        }
        last = found.end();
        // Empty matches advance by one character, as Python's split does.
        position = if found.end() > found.start() {
            found.end()
        } else {
            match text[found.end()..].chars().next() {
                Some(c) => found.end() + c.len_utf8(),
                None => break,
            }
        };
    }
    if last < text.len() {
        chunks.push(text[last..].to_string());
    }
    chunks.retain(|c| !c.is_empty());
    chunks
}

fn is_blank(chunk: &str) -> bool {
    chunk.trim().is_empty()
}

/// `textwrap.wrap(text, width, ..., break_long_words=False)`.
pub fn wrap(text: &str, options: &Options) -> Vec<String> {
    let mut chunks = split_chunks(&munge_whitespace(text));
    chunks.reverse();
    let mut lines: Vec<String> = Vec::new();
    while !chunks.is_empty() {
        let mut current: Vec<String> = Vec::new();
        let mut current_len = 0;
        let indent = if lines.is_empty() {
            options.initial_indent
        } else {
            options.subsequent_indent
        };
        let width = options.width.saturating_sub(char_len(indent));
        if options.drop_whitespace && !lines.is_empty() && chunks.last().is_some_and(|c| is_blank(c)) {
            chunks.pop();
        }
        while let Some(chunk) = chunks.last() {
            let length = char_len(chunk);
            if current_len + length <= width {
                current_len += length;
                current.push(chunks.pop().expect("checked by last()"));
            } else {
                break;
            }
        }
        // Long words are never broken: a word wider than the line gets a
        // line of its own.
        if chunks.last().is_some_and(|c| char_len(c) > width) && current.is_empty() {
            current.push(chunks.pop().expect("checked by last()"));
        }
        if options.drop_whitespace && current.last().is_some_and(|c| is_blank(c)) {
            current.pop();
        }
        if !current.is_empty() {
            lines.push(format!("{indent}{}", current.concat()));
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_like_python() {
        // The example from CPython's documentation of wordsep_re.
        assert_eq!(
            split_chunks("Hello there -- you goof-ball, use the -b option!"),
            vec![
                "Hello", " ", "there", " ", "--", " ", "you", " ", "goof-", "ball,", " ", "use", " ", "the", " ", "-b",
                " ", "option!"
            ]
        );
    }

    #[test]
    fn wraps_like_python() {
        let options = Options {
            width: 20,
            ..Options::default()
        };
        assert_eq!(
            wrap("The quick brown fox jumps over the lazy dog", &options),
            vec!["The quick brown fox", "jumps over the lazy", "dog"]
        );
        let keep = Options {
            width: 20,
            drop_whitespace: false,
            ..Options::default()
        };
        assert_eq!(
            wrap("The quick brown fox jumps over the lazy dog", &keep),
            vec!["The quick brown fox ", "jumps over the lazy ", "dog"]
        );
        assert_eq!(
            wrap("averyveryverylongwordthatdoesnotfit and more", &options),
            vec!["averyveryverylongwordthatdoesnotfit", "and more"]
        );
    }
}
