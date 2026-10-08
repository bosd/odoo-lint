//! Automatic fixes: text edits attached to a violation.
//!
//! As in Ruff, a fix is either safe (`--fix`) or unsafe (`--unsafe-fixes`):
//! an unsafe fix may change behaviour or lose information, and needs review.

use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Applicability {
    Unsafe,
    Safe,
}

/// Replace `start..end` (byte offsets) of a file with `content`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    /// The file to edit; `None` is the file the violation is in.
    pub path: Option<PathBuf>,
    pub start: usize,
    pub end: usize,
    pub content: String,
}

impl Edit {
    pub fn replace(start: usize, end: usize, content: impl Into<String>) -> Self {
        Self {
            path: None,
            start,
            end,
            content: content.into(),
        }
    }

    pub fn insert(at: usize, content: impl Into<String>) -> Self {
        Self::replace(at, at, content)
    }

    pub fn delete(start: usize, end: usize) -> Self {
        Self::replace(start, end, "")
    }

    /// The same edit, in another file.
    pub fn in_file(mut self, path: PathBuf) -> Self {
        self.path = Some(path);
        self
    }

    /// Whether two edits of the same file touch the same text. Insertions
    /// never conflict with each other: at the same position they are applied
    /// in the order the fixes were chosen (e.g. several entries appended to
    /// the end of a `.pot`).
    fn overlaps(&self, other: &Edit) -> bool {
        if self.start == self.end && other.start == other.end {
            return false;
        }
        // Strict comparisons: an insertion at either end of a replaced range
        // does not conflict with it.
        self.start < other.end && other.start < self.end
    }
}

/// A fix: edits applied together, with a short description.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Fix {
    pub applicability: Applicability,
    /// What the fix does, e.g. "Use `self.env.cr`".
    pub title: String,
    #[serde(skip)]
    pub edits: Vec<Edit>,
}

impl Fix {
    pub fn safe(title: impl Into<String>, edits: Vec<Edit>) -> Self {
        Self {
            applicability: Applicability::Safe,
            title: title.into(),
            edits,
        }
    }

    pub fn unsafe_(title: impl Into<String>, edits: Vec<Edit>) -> Self {
        Self {
            applicability: Applicability::Unsafe,
            title: title.into(),
            edits,
        }
    }
}

/// Which fixes to apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixMode {
    Safe,
    Unsafe,
}

impl FixMode {
    pub fn allows(self, applicability: Applicability) -> bool {
        match self {
            FixMode::Safe => applicability == Applicability::Safe,
            FixMode::Unsafe => true,
        }
    }
}

/// Picks fixes whose edits do not overlap each other, in order: every edit of
/// a chosen fix is applied, or none of them.
pub fn select_non_overlapping<'a>(fixes: impl IntoIterator<Item = (PathBuf, &'a Fix)>) -> Vec<(PathBuf, &'a Fix)> {
    let mut chosen: Vec<(PathBuf, &Fix)> = Vec::new();
    let mut taken: HashMap<PathBuf, Vec<&Edit>> = HashMap::new();
    for (file, fix) in fixes {
        let targets: Vec<(PathBuf, &Edit)> = fix
            .edits
            .iter()
            .map(|e| (e.path.clone().unwrap_or_else(|| file.clone()), e))
            .collect();
        let conflict = targets.iter().any(|(path, edit)| {
            taken
                .get(path)
                .is_some_and(|edits| edits.iter().any(|e| e.overlaps(edit)))
        });
        if !conflict {
            for (path, edit) in targets {
                taken.entry(path).or_default().push(edit);
            }
            chosen.push((file, fix));
        }
    }
    chosen
}

/// Applies non-overlapping edits to `source`. Insertions at the same position
/// keep their order; an insertion identical to an earlier one is dropped, so
/// two fixes adding the same text add it once.
pub fn apply_edits(source: &str, edits: &[&Edit]) -> String {
    let mut sorted: Vec<&Edit> = Vec::with_capacity(edits.len());
    for edit in edits {
        if !sorted.contains(edit) {
            sorted.push(edit);
        }
    }
    // A stable sort: same-position insertions stay in order.
    sorted.sort_by_key(|e| (e.start, e.end));
    let mut out = String::with_capacity(source.len());
    let mut position = 0;
    for edit in sorted {
        let start = edit.start.clamp(position, source.len());
        out.push_str(&source[position..start]);
        out.push_str(&edit.content);
        position = edit.end.clamp(start, source.len());
    }
    out.push_str(&source[position..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applies_edits_in_order() {
        let source = "self._cr.execute(x)";
        let a = Edit::replace(5, 8, "env.cr");
        let b = Edit::insert(0, "# fixed\n");
        assert_eq!(apply_edits(source, &[&a, &b]), "# fixed\nself.env.cr.execute(x)");
    }

    #[test]
    fn overlapping_fixes_wait_for_the_next_pass() {
        let whole = Fix::safe("rewrite", vec![Edit::replace(0, 10, "x")]);
        let local = Fix::safe("local", vec![Edit::replace(2, 3, "y")]);
        let elsewhere = Fix::safe("other file", vec![Edit::replace(2, 3, "y").in_file("b.po".into())]);
        let chosen = select_non_overlapping([
            ("a.po".into(), &whole),
            ("a.po".into(), &local),
            ("a.po".into(), &elsewhere),
        ]);
        let titles: Vec<_> = chosen.iter().map(|(_, f)| f.title.as_str()).collect();
        assert_eq!(titles, vec!["rewrite", "other file"]);
    }

    #[test]
    fn insertions_at_the_same_place_keep_their_order() {
        let a = Edit::insert(5, "a");
        let b = Edit::insert(5, "b");
        assert!(!a.overlaps(&b));
        assert!(a.overlaps(&Edit::replace(4, 6, "")));
        assert!(!a.overlaps(&Edit::replace(5, 6, "")));
        let dup = Edit::insert(5, "a");
        assert_eq!(apply_edits("01234567", &[&a, &b, &dup]), "01234ab567");
    }
}
