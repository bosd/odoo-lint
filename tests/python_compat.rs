//! Checks odoo-lint's emulation of Python's `%` and `str.format` errors and
//! its port of polib's PO writer against the real implementations.
//!
//! The expected results come from `tests/data/python_compat_cases.json`,
//! generated with CPython and polib by `scripts/gen_python_compat_cases.py`.

use odoo_lint::po::pyformat::{percent_format, str_format, PercentArgs, Value};
use odoo_lint::po::PoFile;
use serde_json::Value as Json;
use std::collections::BTreeMap;

fn cases() -> Json {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/python_compat_cases.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn value(spec: &Json) -> Value {
    if spec == "s" {
        Value::Str
    } else {
        Value::Int(0)
    }
}

fn outcome(result: Result<(), odoo_lint::po::pyformat::PyError>) -> String {
    match result {
        Ok(()) => "ok".to_string(),
        Err(error) => error.repr(),
    }
}

#[test]
fn percent_formatting_matches_python() {
    let mut failures = Vec::new();
    for case in cases()["percent"].as_array().unwrap() {
        let format = case["format"].as_str().unwrap();
        let args = if let Some(tuple) = case["args"].get("tuple") {
            PercentArgs::Tuple(tuple.as_array().unwrap().iter().map(value).collect())
        } else {
            let dict = case["args"]["dict"].as_object().unwrap();
            PercentArgs::Dict(dict.iter().map(|(k, v)| (k.clone(), value(v))).collect())
        };
        let got = outcome(percent_format(format, &args));
        let expected = case["expected"].as_str().unwrap();
        if got != expected {
            failures.push(format!("{format:?} % {}: expected {expected}, got {got}", case["args"]));
        }
    }
    assert!(
        failures.is_empty(),
        "{} mismatches:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn str_format_matches_python() {
    let mut failures = Vec::new();
    for case in cases()["format"].as_array().unwrap() {
        let format = case["format"].as_str().unwrap();
        let args: Vec<Value> = case["args"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| Value::Int(n.as_i64().unwrap()))
            .collect();
        let kwargs: BTreeMap<String, Value> = case["kwargs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|k| (k.as_str().unwrap().to_string(), Value::Int(0)))
            .collect();
        let got = outcome(str_format(format, &args, &kwargs));
        let expected = case["expected"].as_str().unwrap();
        if got != expected {
            failures.push(format!("{format:?}: expected {expected}, got {got}"));
        }
    }
    assert!(
        failures.is_empty(),
        "{} mismatches:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn po_writer_matches_polib() {
    let mut failures = Vec::new();
    for (i, case) in cases()["po"].as_array().unwrap().iter().enumerate() {
        let input = case["input"].as_str().unwrap();
        let expected = case["expected"].as_str().unwrap();
        match PoFile::parse(input) {
            Ok(po) => {
                let got = po.to_po_string();
                if got != expected {
                    let line = got.lines().zip(expected.lines()).position(|(a, b)| a != b).unwrap_or(0);
                    failures.push(format!(
                        "case {i}, line {}:\n  got:      {:?}\n  expected: {:?}",
                        line + 1,
                        got.lines().nth(line),
                        expected.lines().nth(line)
                    ));
                }
            }
            Err(error) => failures.push(format!("case {i}: parse error {error:?}")),
        }
    }
    assert!(
        failures.is_empty(),
        "{} mismatches:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
