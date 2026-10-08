//! Gettext PO files: a port of polib's parser and writer.
//!
//! polib (<https://github.com/izimobil/polib>) is MIT licensed,
//! Copyright (c) 2006-2024 David Jean Louis. The parser state machine, its
//! syntax errors and the serialisation (`str(pofile)`, wrapping at 78
//! columns) follow polib 1.2.0, so `po-pretty-format` gives the same results
//! as OCA's `oca-checks-po`, which uses polib.

pub mod pyformat;
pub mod textwrap;

use std::collections::BTreeMap;

/// One entry (message) of a PO file.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PoEntry {
    pub msgid: String,
    pub msgstr: String,
    pub msgid_plural: String,
    pub msgstr_plural: BTreeMap<u32, String>,
    pub msgctxt: Option<String>,
    pub obsolete: bool,
    /// Extracted comments (`#.`).
    pub comment: String,
    /// Translator comments (`#`).
    pub tcomment: String,
    /// References (`#:`) as `(path, line)`; `line` may be empty.
    pub occurrences: Vec<(String, String)>,
    pub flags: Vec<String>,
    pub previous_msgctxt: Option<String>,
    pub previous_msgid: Option<String>,
    pub previous_msgid_plural: Option<String>,
    /// polib's `linenum`: the line where the entry starts (1-based; 0 for an
    /// entry that starts before any line was read).
    pub linenum: usize,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PoFile {
    /// Header comment lines, without the leading `# `.
    pub header: String,
    /// Metadata of the `msgid ""` entry, in file order.
    pub metadata: Vec<(String, String)>,
    pub metadata_is_fuzzy: bool,
    pub entries: Vec<PoEntry>,
}

/// A syntax error, with polib's message and line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoError {
    pub line: usize,
    /// e.g. `Syntax error in po file: unescaped double quote found`.
    pub message: String,
}

const WRAP_WIDTH: usize = 78;

/// polib's `escape()`.
pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '\n' => out.push_str("\\n"),
            '\x0b' => out.push_str("\\v"),
            '\x08' => out.push_str("\\b"),
            '\x0c' => out.push_str("\\f"),
            '"' => out.push_str("\\\""),
            c => out.push(c),
        }
    }
    out
}

/// polib's `unescape()`.
pub fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            let replacement = match chars.peek() {
                Some('n') => Some('\n'),
                Some('t') => Some('\t'),
                Some('r') => Some('\r'),
                Some('v') => Some('\x0b'),
                Some('b') => Some('\x08'),
                Some('f') => Some('\x0c'),
                Some('\\') => Some('\\'),
                Some('"') => Some('"'),
                _ => None,
            };
            if let Some(r) = replacement {
                out.push(r);
                chars.next();
                continue;
            }
        }
        out.push(c);
    }
    out
}

/// Python's `str.splitlines()`; with `keepends` the separators are kept.
pub fn splitlines(s: &str, keepends: bool) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut iter = s.char_indices().peekable();
    while let Some((i, c)) = iter.next() {
        let is_break = matches!(
            c,
            '\n' | '\r' | '\x0b' | '\x0c' | '\x1c' | '\x1d' | '\x1e' | '\u{85}' | '\u{2028}' | '\u{2029}'
        );
        if !is_break {
            continue;
        }
        let mut end = i + c.len_utf8();
        if c == '\r' {
            if let Some((_, '\n')) = iter.peek() {
                iter.next();
                end += 1;
            }
        }
        lines.push(if keepends { &s[start..end] } else { &s[start..i] });
        start = end;
    }
    if start < s.len() {
        lines.push(&s[start..]);
    }
    lines
}

/// `s[1:-1]` in Python, by characters.
fn strip_quotes(s: &str) -> &str {
    let mut chars = s.char_indices();
    let start = chars.next().map_or(0, |(_, c)| c.len_utf8());
    let end = s.char_indices().next_back().map_or(0, |(i, _)| i);
    if end <= start {
        ""
    } else {
        &s[start..end]
    }
}

/// polib's check for `([^\\]|^)"` in the quoted value.
fn has_unescaped_quote(token: &str) -> bool {
    let inner: Vec<char> = strip_quotes(token).chars().collect();
    inner
        .iter()
        .enumerate()
        .any(|(i, c)| *c == '"' && (i == 0 || inner[i - 1] != '\\'))
}

/// `str.split(None, 2)`.
fn split_tokens(line: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    let mut rest = line.trim_start();
    while !rest.is_empty() {
        if tokens.len() == 2 {
            tokens.push(rest);
            break;
        }
        match rest.find(char::is_whitespace) {
            Some(end) => {
                tokens.push(&rest[..end]);
                rest = rest[end..].trim_start();
            }
            None => {
                tokens.push(rest);
                break;
            }
        }
    }
    tokens
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    St,
    He,
    Tc,
    Gc,
    Oc,
    Fl,
    Ct,
    Pc,
    Pm,
    Pp,
    Mi,
    Mp,
    Ms,
    Mx,
    Mc,
}

/// The transitions of polib's state machine: whether `symbol` may follow
/// `state`, and the state it leads to.
fn transition(symbol: State, state: State) -> Option<State> {
    use State::*;
    let allowed: &[State] = match symbol {
        Tc if matches!(state, St | He) => return Some(He),
        Tc => &[Gc, Oc, Fl, Tc, Pc, Pm, Pp, Ms, Mp, Mx, Mi],
        Gc | Oc | Fl | Pc | Pm | Pp => &[St, He, Gc, Oc, Fl, Ct, Pc, Pm, Pp, Tc, Ms, Mp, Mx, Mi],
        Ct => &[St, He, Gc, Oc, Fl, Tc, Pc, Pm, Pp, Ms, Mx],
        Mi => &[St, He, Gc, Oc, Fl, Ct, Tc, Pc, Pm, Pp, Ms, Mx],
        Mp => &[Tc, Gc, Pc, Pm, Pp, Mi],
        Ms => &[Mi, Mp, Tc],
        Mx => &[Mi, Mx, Mp, Tc],
        Mc => &[Ct, Mi, Mp, Ms, Mx, Pm, Pp, Pc],
        St | He => &[],
    };
    allowed.contains(&state).then_some(symbol)
}

struct Parser {
    file: PoFile,
    entry: PoEntry,
    state: State,
    line: usize,
    token: String,
    msgstr_index: u32,
    obsolete: bool,
}

impl Parser {
    fn error(&self, detail: &str) -> PoError {
        PoError {
            line: self.line,
            message: format!("Syntax error in po file{detail}"),
        }
    }

    /// Starts a new entry if the current one is complete.
    fn new_entry_if_done(&mut self) {
        if matches!(self.state, State::Mc | State::Ms | State::Mx) {
            let done = std::mem::replace(
                &mut self.entry,
                PoEntry {
                    linenum: self.line,
                    ..PoEntry::default()
                },
            );
            self.file.entries.push(done);
        }
    }

    fn process(&mut self, symbol: State) -> Result<(), PoError> {
        let Some(next) = transition(symbol, self.state) else {
            return Err(self.error(""));
        };
        let token = self.token.clone();
        let value = || unescape(strip_quotes(&token));
        let keep_state = match symbol {
            State::He => unreachable!("header is reached through Tc"),
            State::Tc if next == State::He => {
                if !self.file.header.is_empty() {
                    self.file.header.push('\n');
                }
                self.file.header.push_str(token.get(2..).unwrap_or(""));
                false
            }
            State::Tc => {
                self.new_entry_if_done();
                if !self.entry.tcomment.is_empty() {
                    self.entry.tcomment.push('\n');
                }
                let comment = token.trim_start_matches('#');
                self.entry
                    .tcomment
                    .push_str(comment.strip_prefix(' ').unwrap_or(comment));
                false
            }
            State::Gc => {
                self.new_entry_if_done();
                if !self.entry.comment.is_empty() {
                    self.entry.comment.push('\n');
                }
                self.entry.comment.push_str(token.get(3..).unwrap_or(""));
                false
            }
            State::Oc => {
                self.new_entry_if_done();
                for occurrence in token.get(3..).unwrap_or("").split_whitespace() {
                    let reference = match occurrence.rsplit_once(':') {
                        Some((path, line)) if !line.is_empty() && line.chars().all(|c| c.is_ascii_digit()) => {
                            (path.to_string(), line.to_string())
                        }
                        _ => (occurrence.to_string(), String::new()),
                    };
                    self.entry.occurrences.push(reference);
                }
                false
            }
            State::Fl => {
                self.new_entry_if_done();
                self.entry
                    .flags
                    .extend(token.get(3..).unwrap_or("").split(',').map(|f| f.trim().to_string()));
                false
            }
            State::Pp => {
                self.new_entry_if_done();
                self.entry.previous_msgid_plural = Some(value());
                false
            }
            State::Pm => {
                self.new_entry_if_done();
                self.entry.previous_msgid = Some(value());
                false
            }
            State::Pc => {
                self.new_entry_if_done();
                self.entry.previous_msgctxt = Some(value());
                false
            }
            State::Ct => {
                self.new_entry_if_done();
                self.entry.msgctxt = Some(value());
                false
            }
            State::Mi => {
                self.new_entry_if_done();
                self.entry.obsolete = self.obsolete;
                self.entry.msgid = value();
                false
            }
            State::Mp => {
                self.entry.msgid_plural = value();
                false
            }
            State::Ms => {
                self.entry.msgstr = value();
                false
            }
            State::Mx => {
                let index = token
                    .chars()
                    .nth(7)
                    .and_then(|c| c.to_digit(10))
                    .ok_or_else(|| self.error(""))?;
                let start = token.find('"').map_or(token.len(), |p| p + 1);
                let end = token.char_indices().next_back().map_or(0, |(i, _)| i);
                let raw = if start <= end { &token[start..end] } else { "" };
                self.entry.msgstr_plural.insert(index, unescape(raw));
                self.msgstr_index = index;
                false
            }
            State::Mc => {
                let text = value();
                let target = match self.state {
                    State::Ct => self.entry.msgctxt.get_or_insert_with(String::new),
                    State::Mi => &mut self.entry.msgid,
                    State::Mp => &mut self.entry.msgid_plural,
                    State::Ms => &mut self.entry.msgstr,
                    State::Mx => self.entry.msgstr_plural.entry(self.msgstr_index).or_default(),
                    State::Pp => self.entry.previous_msgid_plural.get_or_insert_with(String::new),
                    State::Pm => self.entry.previous_msgid.get_or_insert_with(String::new),
                    State::Pc => self.entry.previous_msgctxt.get_or_insert_with(String::new),
                    _ => return Ok(()),
                };
                target.push_str(&text);
                true
            }
            State::St => unreachable!("never a symbol"),
        };
        if !keep_state {
            self.state = next;
        }
        Ok(())
    }
}

impl PoFile {
    /// Parses PO file contents the way polib does.
    pub fn parse(contents: &str) -> Result<PoFile, PoError> {
        let mut parser = Parser {
            file: PoFile::default(),
            entry: PoEntry::default(),
            state: State::St,
            line: 0,
            token: String::new(),
            msgstr_index: 0,
            obsolete: false,
        };
        let mut last_tokens_comment = true;
        let mut seen_tokens = false;
        for raw in splitlines(contents, false) {
            parser.line += 1;
            let raw = if parser.line == 1 {
                raw.strip_prefix('\u{feff}').unwrap_or(raw)
            } else {
                raw
            };
            let mut line = raw.trim().to_string();
            if line.is_empty() {
                continue;
            }
            let mut tokens: Vec<String> = split_tokens(&line).into_iter().map(str::to_string).collect();
            seen_tokens = true;
            if tokens[0] == "#~|" {
                last_tokens_comment = true;
                continue;
            }
            if tokens[0] == "#~" && tokens.len() > 1 {
                line = line.chars().skip(3).collect::<String>().trim().to_string();
                tokens.remove(0);
                parser.obsolete = true;
            } else {
                parser.obsolete = false;
            }
            // polib decides at the end, from the last line's tokens, whether
            // the file ended inside an entry.
            last_tokens_comment = tokens[0].starts_with('#');
            let keyword = match tokens[0].as_str() {
                "msgctxt" => Some(State::Ct),
                "msgid" => Some(State::Mi),
                "msgstr" => Some(State::Ms),
                "msgid_plural" => Some(State::Mp),
                _ => None,
            };
            if let (Some(symbol), true) = (keyword, tokens.len() > 1) {
                let rest = line[tokens[0].len()..].trim_start().to_string();
                if has_unescaped_quote(&rest) {
                    return Err(parser.error(": unescaped double quote found"));
                }
                parser.token = rest;
                parser.process(symbol)?;
                continue;
            }
            parser.token = line.clone();
            let first = tokens[0].as_str();
            if first == "#:" {
                if tokens.len() > 1 {
                    parser.process(State::Oc)?;
                }
            } else if line.starts_with('"') {
                if has_unescaped_quote(&line) {
                    return Err(parser.error(": unescaped double quote found"));
                }
                parser.process(State::Mc)?;
            } else if line.starts_with("msgstr[") {
                parser.process(State::Mx)?;
            } else if first == "#," {
                if tokens.len() > 1 {
                    parser.process(State::Fl)?;
                }
            } else if first == "#" || first.starts_with("##") {
                parser.process(State::Tc)?;
            } else if first == "#." {
                if tokens.len() > 1 {
                    parser.process(State::Gc)?;
                }
            } else if first == "#|" {
                if tokens.len() <= 1 {
                    return Err(parser.error(""));
                }
                parser.token = line[2..].trim_start().to_string();
                if tokens[1].starts_with('"') {
                    parser.process(State::Mc)?;
                    continue;
                }
                if tokens.len() == 2 {
                    return Err(parser.error(": invalid continuation line"));
                }
                let symbol = match tokens[1].as_str() {
                    "msgid_plural" => State::Pp,
                    "msgid" => State::Pm,
                    "msgctxt" => State::Pc,
                    other => return Err(parser.error(&format!(": unknown keyword {other}"))),
                };
                parser.token = parser.token[tokens[1].len()..].trim_start().to_string();
                parser.process(symbol)?;
            } else {
                return Err(parser.error(""));
            }
        }
        // The last entry is added unless the file ends in comments.
        if seen_tokens && !last_tokens_comment {
            let entry = std::mem::take(&mut parser.entry);
            parser.file.entries.push(entry);
        }
        let mut file = parser.file;
        file.extract_metadata();
        Ok(file)
    }

    /// polib's handling of the `msgid ""` entry: it becomes the metadata.
    fn extract_metadata(&mut self) {
        let candidates: Vec<usize> = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| !e.obsolete && e.msgid.is_empty())
            .map(|(i, _)| i)
            .collect();
        let index = match candidates.as_slice() {
            [] => return,
            [only] => *only,
            many => many
                .iter()
                .rev()
                .find(|i| self.entries[**i].msgctxt.as_deref().unwrap_or("").is_empty())
                .copied()
                .unwrap_or(many[0]),
        };
        let entry = self.entries.remove(index);
        self.metadata_is_fuzzy = !entry.flags.is_empty();
        let mut key: Option<String> = None;
        for line in splitlines(&entry.msgstr, false) {
            match line.split_once(':') {
                Some((name, value)) => {
                    let name = name.to_string();
                    let value = value.trim().to_string();
                    match self.metadata.iter_mut().find(|(k, _)| *k == name) {
                        Some(existing) => existing.1 = value,
                        None => self.metadata.push((name.clone(), value)),
                    }
                    key = Some(name);
                }
                None => {
                    if let Some(key) = &key {
                        if let Some(existing) = self.metadata.iter_mut().find(|(k, _)| k == key) {
                            existing.1.push('\n');
                            existing.1.push_str(line.trim());
                        }
                    }
                }
            }
        }
    }

    /// polib's `ordered_metadata()`.
    fn ordered_metadata(&self) -> Vec<(String, String)> {
        const ORDER: &[&str] = &[
            "Project-Id-Version",
            "Report-Msgid-Bugs-To",
            "POT-Creation-Date",
            "PO-Revision-Date",
            "Last-Translator",
            "Language-Team",
            "Language",
            "MIME-Version",
            "Content-Type",
            "Content-Transfer-Encoding",
            "Plural-Forms",
        ];
        let mut ordered: Vec<(String, String)> = ORDER
            .iter()
            .filter_map(|name| self.metadata.iter().find(|(k, _)| k == name).cloned())
            .collect();
        let mut rest: Vec<(String, String)> = self
            .metadata
            .iter()
            .filter(|(k, _)| !ORDER.contains(&k.as_str()))
            .cloned()
            .collect();
        rest.sort_by_key(|(k, _)| natural_key(k));
        ordered.extend(rest);
        ordered
    }

    fn metadata_entry(&self) -> PoEntry {
        let mut entry = PoEntry::default();
        let metadata = self.ordered_metadata();
        if !metadata.is_empty() {
            let lines: Vec<String> = metadata.iter().map(|(k, v)| format!("{k}: {v}")).collect();
            entry.msgstr = format!("{}\n", lines.join("\n"));
        }
        if self.metadata_is_fuzzy {
            entry.flags.push("fuzzy".into());
        }
        entry
    }

    /// `str(pofile)` as polib writes it.
    pub fn to_po_string(&self) -> String {
        let mut out = String::new();
        for header in self.header.split('\n') {
            if header.is_empty() {
                out.push_str("#\n");
            } else if header.starts_with(',') || header.starts_with(':') {
                out.push_str(&format!("#{header}\n"));
            } else {
                out.push_str(&format!("# {header}\n"));
            }
        }
        let mut parts = vec![self.metadata_entry().to_po_string()];
        parts.extend(self.entries.iter().filter(|e| !e.obsolete).map(PoEntry::to_po_string));
        parts.extend(self.entries.iter().filter(|e| e.obsolete).map(PoEntry::to_po_string));
        out.push_str(&parts.join("\n"));
        out
    }
}

/// polib's `natural_sort` key: digit runs compare as numbers, the rest
/// case-insensitively.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
enum NaturalPart {
    Text(String),
    Number(u128),
}

fn natural_key(s: &str) -> Vec<NaturalPart> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut digits = false;
    for c in s.chars() {
        if c.is_ascii_digit() != digits && !current.is_empty() {
            parts.push(natural_part(&current, digits));
            current.clear();
        }
        digits = c.is_ascii_digit();
        current.push(c);
    }
    if !current.is_empty() {
        parts.push(natural_part(&current, digits));
    }
    parts
}

fn natural_part(s: &str, digits: bool) -> NaturalPart {
    match (digits, s.parse::<u128>()) {
        (true, Ok(n)) => NaturalPart::Number(n),
        _ => NaturalPart::Text(s.to_lowercase()),
    }
}

impl PoEntry {
    /// polib's `_str_field`.
    fn str_field(fieldname: &str, delflag: &str, plural_index: &str, field: &str) -> Vec<String> {
        let mut lines: Vec<String> = splitlines(field, true).into_iter().map(str::to_string).collect();
        if lines.len() > 1 {
            lines.insert(0, String::new());
        } else {
            let escaped = escape(field);
            let special = field
                .chars()
                .filter(|c| matches!(c, '\\' | '\n' | '\r' | '\t' | '\x0b' | '\x08' | '\x0c' | '"'))
                .count();
            let mut flength = fieldname.chars().count() + 3;
            if !plural_index.is_empty() {
                flength += plural_index.chars().count();
            }
            let real_wrapwidth = (WRAP_WIDTH + special) as isize - flength as isize;
            if (field.chars().count() as isize) > real_wrapwidth {
                let options = textwrap::Options {
                    width: WRAP_WIDTH - 2,
                    drop_whitespace: false,
                    ..textwrap::Options::default()
                };
                lines = std::iter::once(String::new())
                    .chain(textwrap::wrap(&escaped, &options).iter().map(|item| unescape(item)))
                    .collect();
            } else {
                lines = vec![field.to_string()];
            }
        }
        let fieldname = fieldname.strip_prefix("previous_").unwrap_or(fieldname);
        let mut out = vec![format!("{delflag}{fieldname}{plural_index} \"{}\"", escape(&lines[0]))];
        out.extend(lines[1..].iter().map(|line| format!("{delflag}\"{}\"", escape(line))));
        out
    }

    /// polib's `_BaseEntry.__unicode__`: the message fields.
    fn fields_string(&self) -> String {
        let delflag = if self.obsolete { "#~ " } else { "" };
        let mut out: Vec<String> = Vec::new();
        if let Some(msgctxt) = &self.msgctxt {
            out.extend(Self::str_field("msgctxt", delflag, "", msgctxt));
        }
        out.extend(Self::str_field("msgid", delflag, "", &self.msgid));
        if !self.msgid_plural.is_empty() {
            out.extend(Self::str_field("msgid_plural", delflag, "", &self.msgid_plural));
        }
        if self.msgstr_plural.is_empty() {
            out.extend(Self::str_field("msgstr", delflag, "", &self.msgstr));
        } else {
            for (index, msgstr) in &self.msgstr_plural {
                out.extend(Self::str_field("msgstr", delflag, &format!("[{index}]"), msgstr));
            }
        }
        out.push(String::new());
        out.join("\n")
    }

    /// `str(entry)` as polib writes it.
    pub fn to_po_string(&self) -> String {
        let mut out: Vec<String> = Vec::new();
        let comments: &[(&str, &str)] = if self.obsolete {
            &[("tcomment", "# ")]
        } else {
            &[("tcomment", "# "), ("comment", "#. ")]
        };
        for (field, prefix) in comments {
            let value = if *field == "tcomment" {
                &self.tcomment
            } else {
                &self.comment
            };
            if value.is_empty() {
                continue;
            }
            for comment in value.split('\n') {
                if comment.chars().count() + prefix.chars().count() > WRAP_WIDTH {
                    out.extend(textwrap::wrap(
                        comment,
                        &textwrap::Options {
                            width: WRAP_WIDTH,
                            initial_indent: prefix,
                            subsequent_indent: prefix,
                            ..textwrap::Options::default()
                        },
                    ));
                } else {
                    out.push(format!("{prefix}{comment}"));
                }
            }
        }
        if !self.obsolete && !self.occurrences.is_empty() {
            let files: Vec<String> = self
                .occurrences
                .iter()
                .map(|(path, line)| {
                    if line.is_empty() {
                        path.clone()
                    } else {
                        format!("{path}:{line}")
                    }
                })
                .collect();
            let files = files.join(" ");
            if files.chars().count() + 3 > WRAP_WIDTH {
                // polib keeps hyphenated file names together.
                out.extend(
                    textwrap::wrap(
                        &files.replace('-', "*"),
                        &textwrap::Options {
                            width: WRAP_WIDTH,
                            initial_indent: "#: ",
                            subsequent_indent: "#: ",
                            ..textwrap::Options::default()
                        },
                    )
                    .into_iter()
                    .map(|line| line.replace('*', "-")),
                );
            } else {
                out.push(format!("#: {files}"));
            }
        }
        if !self.flags.is_empty() {
            out.push(format!("#, {}", self.flags.join(", ")));
        }
        let prefix = if self.obsolete { "#~| " } else { "#| " };
        for (name, value) in [
            ("previous_msgctxt", &self.previous_msgctxt),
            ("previous_msgid", &self.previous_msgid),
            ("previous_msgid_plural", &self.previous_msgid_plural),
        ] {
            if let Some(value) = value {
                out.extend(Self::str_field(name, prefix, "", value));
            }
        }
        out.push(self.fields_string());
        out.join("\n")
    }

    /// The line of the `msgid`, as `msgfmt` reports it (OCA's
    /// `_get_po_line_number`): `linenum` plus the comment lines before it.
    pub fn msgid_line(&self) -> usize {
        let comment_lines = self
            .to_po_string()
            .split('\n')
            .take_while(|line| line.starts_with('#'))
            .count();
        self.linenum + comment_lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"# Translation of Odoo Server.
# This file contains the translation of the following modules:
#	* acme_sale
#
msgid ""
msgstr ""
"Project-Id-Version: Odoo Server 17.0\n"
"Content-Type: text/plain; charset=UTF-8\n"
"Language: nl\n"

#. module: acme_sale
#: model:ir.model.fields,field_description:acme_sale.field_sale_order__note
msgid "Note"
msgstr "Notitie"

#. module: acme_sale
#: code:addons/acme_sale/models/sale.py:0
#, python-format
msgid "Order %s"
msgstr "Order %s"
"#;

    #[test]
    fn parses_entries_and_metadata() {
        let po = PoFile::parse(SAMPLE).unwrap();
        assert_eq!(po.entries.len(), 2);
        assert_eq!(po.metadata[0], ("Project-Id-Version".into(), "Odoo Server 17.0".into()));
        let note = &po.entries[0];
        assert_eq!(note.msgid, "Note");
        assert_eq!(note.comment, "module: acme_sale");
        assert_eq!(
            note.occurrences[0].0,
            "model:ir.model.fields,field_description:acme_sale.field_sale_order__note"
        );
        assert_eq!(note.linenum, 11);
        assert_eq!(note.msgid_line(), 13);
        assert_eq!(po.entries[1].flags, vec!["python-format"]);
    }

    #[test]
    fn round_trips_pretty_files() {
        let po = PoFile::parse(SAMPLE).unwrap();
        let out = po.to_po_string();
        assert!(out.starts_with("# Translation of Odoo Server.\n"));
        assert!(out.contains("msgid \"Note\"\nmsgstr \"Notitie\"\n"));
        // Writing is stable: the written file writes back identically.
        assert_eq!(PoFile::parse(&out).unwrap().to_po_string(), out);
    }

    #[test]
    fn syntax_errors() {
        let err = PoFile::parse("msgid \"a\"\nmsgstr \"b\" c\"\n").unwrap_err();
        assert_eq!(err.line, 2);
        assert_eq!(err.message, "Syntax error in po file: unescaped double quote found");
        let err = PoFile::parse("msgstr \"b\"\n").unwrap_err();
        assert_eq!((err.line, err.message.as_str()), (1, "Syntax error in po file"));
        assert!(PoFile::parse("#nospace\n").is_err());
    }

    #[test]
    fn long_lines_wrap_at_78() {
        let entry = PoEntry {
            msgid: "word ".repeat(30).trim_end().to_string(),
            ..PoEntry::default()
        };
        let text = entry.to_po_string();
        assert!(text.starts_with("msgid \"\"\n\"word word"));
        assert!(text.lines().all(|l| l.chars().count() <= 78));
    }

    #[test]
    fn escapes() {
        assert_eq!(escape("a\"b\\c\n"), "a\\\"b\\\\c\\n");
        assert_eq!(unescape("a\\\"b\\\\c\\n\\q"), "a\"b\\c\n\\q");
        assert_eq!(splitlines("a\r\nb\nc", true), vec!["a\r\n", "b\n", "c"]);
    }
}
