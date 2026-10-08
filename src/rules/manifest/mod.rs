//! Rules on `__manifest__.py`, ported from pylint-odoo with its codes, names
//! and messages. Positions follow pylint-odoo: on the offending key when there
//! is one, otherwise on the manifest dict.

use crate::checker::ManifestContext;
use ruff_python_ast::Expr;
use ruff_text_size::{Ranged, TextSize};

pub mod author;
pub mod files;
pub mod keys;
pub mod values;

/// Offset of `key` in the manifest, or of the dict when the key is absent.
pub(crate) fn key_or_dict(ctx: &ManifestContext, key: &str) -> TextSize {
    ctx.manifest
        .entry(key)
        .map_or_else(|| ctx.manifest.dict().start(), |(key, _)| key.start())
}

/// String elements of a list or tuple literal.
pub(crate) fn string_elements(expr: &Expr) -> Vec<(&str, &Expr)> {
    let elts = match expr {
        Expr::List(list) => &list.elts[..],
        Expr::Tuple(tuple) => &tuple.elts[..],
        _ => return Vec::new(),
    };
    elts.iter()
        .filter_map(|e| e.as_string_literal_expr().map(|s| (s.value.to_str(), e)))
        .collect()
}
