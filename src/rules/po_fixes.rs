//! Positions in PO source text for fixes: lines, entry spans and where an
//! entry's comments end. Offsets are bytes in the normalised source.

use crate::po::{PoEntry, PoFile};

/// Byte offset of the start of every line; index 0 is line 1. A final entry
/// is the end of the text.
pub fn line_starts(source: &str) -> Vec<usize> {
    let mut starts = vec![0];
    starts.extend(source.match_indices('\n').map(|(i, _)| i + 1));
    starts
}

/// Byte offset where 1-based `line` starts (the end of the text if past it).
pub fn line_offset(starts: &[usize], line: usize) -> usize {
    let last = *starts.last().unwrap_or(&0);
    line.checked_sub(1).and_then(|i| starts.get(i)).copied().unwrap_or(last)
}

/// The text of 1-based `line`, without its newline.
pub fn line_text<'a>(source: &'a str, starts: &[usize], line: usize) -> &'a str {
    let start = line_offset(starts, line);
    let end = starts.get(line).copied().unwrap_or(source.len());
    source[start..end].trim_end_matches('\n')
}

/// The 1-based line of an entry's first non-comment line (`msgctxt` or
/// `msgid`), found in the source rather than from the serialised entry.
pub fn first_field_line(source: &str, starts: &[usize], entry: &PoEntry) -> usize {
    let mut line = entry.linenum.max(1);
    while line < starts.len() {
        let text = line_text(source, starts, line).trim_start();
        if !text.is_empty() && !(text.starts_with('#') && !text.starts_with("#~ ")) {
            break;
        }
        line += 1;
    }
    line
}

/// Byte range of an entry in the source: from its first line to the first
/// line of the next entry (so the blank line after it is included).
pub fn entry_span(po: &PoFile, source: &str, starts: &[usize], entry: &PoEntry) -> (usize, usize) {
    let start = line_offset(starts, entry.linenum.max(1));
    let next = po
        .entries
        .iter()
        .map(|e| e.linenum)
        .filter(|&l| l > entry.linenum)
        .min()
        .map_or(source.len(), |l| line_offset(starts, l));
    (start, next)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines() {
        let source = "a\nbb\n\nc";
        let starts = line_starts(source);
        assert_eq!(line_offset(&starts, 2), 2);
        assert_eq!(line_text(source, &starts, 2), "bb");
        assert_eq!(line_text(source, &starts, 4), "c");
    }

    #[test]
    fn entry_positions() {
        let source = "msgid \"\"\nmsgstr \"\"\n\n#. module: m\n#: code:a.py:0\nmsgid \"A\"\nmsgstr \"a\"\n\n#. module: m\nmsgid \"B\"\nmsgstr \"\"\n";
        let po = PoFile::parse(source).unwrap();
        let starts = line_starts(source);
        let a = &po.entries[0];
        assert_eq!(first_field_line(source, &starts, a), 6);
        let (start, end) = entry_span(&po, source, &starts, a);
        assert_eq!(
            &source[start..end],
            "#. module: m\n#: code:a.py:0\nmsgid \"A\"\nmsgstr \"a\"\n\n"
        );
        let (start, end) = entry_span(&po, source, &starts, &po.entries[1]);
        assert_eq!(&source[start..end], "#. module: m\nmsgid \"B\"\nmsgstr \"\"\n");
    }
}
