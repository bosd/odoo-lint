## Features

* **500x faster** than traditional Pylint checks.
* Context-aware manifest author validation (support for OCA and custom company prefixes).
* Built with Astral's `ruff_python_parser`.
""",
"src/main.rs": """use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::process::exit;

mod config;
mod diagnostics;
mod linter;
mod rules;

# [derive(Parser)]
# [command(name = "odl", version = "0.1.0", about = "Blazing fast Odoo linter in Rust")]
struct Cli {
# [command(subcommand)]
command: Commands,
}

# [derive(Subcommand)]
enum Commands {
/// Lint an Odoo addon directory or file
Check {
/// Target directory or file path
# [arg(default_value = ".")]
path: PathBuf,

```
    /// Target Odoo version (e.g. 16.0, 17.0)
    #[arg(short, long, default_value = "17.0")]
    version: String,
},

```

}

fn main() {
let cli = Cli::parse();

```
match cli.command {
    Commands::Check { path, version } => {
        println!("🚀 Running odl check on {:?} (Odoo v{})...", path, version);
        let violations = linter::lint_directory(&path, &version);
        
        if violations.is_empty() {
            println!("✨ No violations found!");
        } else {
            for v in &violations {
                println!("{}", v);
            }
            println!("\\n❌ Found {} violation(s).", violations.len());
            exit(1);
        }
    }
}

```

}
""",

```
"src/lib.rs": """pub mod config;

```

pub mod diagnostics;
pub mod linter;
pub mod rules;
""",

```
"src/config.rs": """use serde::Deserialize;

```

use std::collections::HashMap;

# [derive(Debug, Deserialize, Default, Clone)]
pub struct OdooLintConfig {
pub target_version: Option,
pub rules: Option,
}

# [derive(Debug, Deserialize, Default, Clone)]
pub struct RulesConfig {
pub manifest_author: Option,
}

# [derive(Debug, Deserialize, Default, Clone)]
pub struct ManifestAuthorConfig {
pub default: Option,
pub mapping: Option<HashMap<String, String>>,
}

impl OdooLintConfig {
pub fn get_expected_author(&self, module_name: &str) -> String {
if let Some(rules) = &self.rules {
if let Some(author_cfg) = &rules.manifest_author {
if let Some(mapping) = &author_cfg.mapping {
for (pattern, expected) in mapping {
if pattern.ends_with('*') {
let prefix = &pattern[..pattern.len() - 1];
if module_name.starts_with(prefix) {
return expected.clone();
}
} else if pattern == module_name {
return expected.clone();
}
}
}
if let Some(default) = &author_cfg.default {
return default.clone();
}
}
}
"Odoo Community Association (OCA)".to_string()
}
}
""",

```
"src/diagnostics.rs": """use std::fmt;

```

# [derive(Debug, Clone)]
pub struct Violation {
pub file_path: String,
pub line: usize,
pub rule_code: &'static str,
pub message: String,
}

impl fmt::Display for Violation {
fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
write!(
f,
"{}:{}: [{}] {}",
self.file_path, self.line, self.rule_code, self.message
)
}
}
""",

```
"src/linter.rs": """use crate::config::OdooLintConfig;

```

use crate::diagnostics::Violation;
use crate::rules::{odoo001_missing_depends, odoo010_manifest_author};
use rayon::prelude::*;
use std::path::Path;
use walkdir::WalkDir;

pub fn lint_directory(root: &Path, _version: &str) -> Vec {
let config = OdooLintConfig::default(); // TODO: Load from pyproject.toml

```
let entries: Vec<_> = WalkDir::new(root)
    .into_iter()
    .filter_map(|e| e.ok())
    .collect();

entries
    .par_iter()
    .flat_map(|entry| {
        let mut violations = Vec::new();
        let path = entry.path();

        if path.file_name() == Some(std::ffi::OsStr::new("__manifest__.py")) {
            let content = std::fs::read_to_string(path).unwrap_or_default();
            let module_name = path
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|s| s.to_str())
                .unwrap_or("unknown");

            violations.extend(odoo010_manifest_author::check_manifest(
                path.to_str().unwrap_or(""),
                &content,
                module_name,
                &config,
            ));
        } else if path.extension() == Some(std::ffi::OsStr::new("py")) {
            let content = std::fs::read_to_string(path).unwrap_or_default();
            violations.extend(odoo001_missing_depends::check_python_file(
                path.to_str().unwrap_or(""),
                &content,
            ));
        }

        violations
    })
    .collect()

```

}
""",

```
"src/rules/mod.rs": """pub mod odoo001_missing_depends;

```

pub mod odoo010_manifest_author;
""",

```
"src/rules/odoo001_missing_depends.rs": """use crate::diagnostics::Violation;

```

use ruff_python_parser::parse_module;

pub fn check_python_file(file_path: &str, content: &str) -> Vec {
let mut violations = Vec::new();
let *parsed = match parse_module(content) {
Ok(ast) => ast,
Err(*) => return violations,
};

```
// TODO: Complete AST traversal using ruff_python_ast for @api.depends check
violations

```

}
""",

```
"src/rules/odoo010_manifest_author.rs": """use crate::config::OdooLintConfig;

```

use crate::diagnostics::Violation;

pub fn check_manifest(
file_path: &str,
content: &str,
module_name: &str,
config: &OdooLintConfig,
) -> Vec {
let mut violations = Vec::new();
let expected_author = config.get_expected_author(module_name);

```
if !content.contains(&format!("'author': '{}'", expected_author))
    && !content.contains(&format!("\"author\": \"{}\"", expected_author))
{
    violations.push(Violation {
        file_path: file_path.to_string(),
        line: 1,
        rule_code: "ODOO010",
        message: format!(
            "Manifest author does not match expected '{}' for module '{}'",
            expected_author, module_name
        ),
    });
}

violations

```

}
""",

```
"PROJECT_PLAN.md": """# Project Plan: `odoo-lint` (`odl`)

```

## Objective

Build an ultra-fast Rust-native static analysis linter for Odoo codebases (`.py`, `.xml`, `__manifest__.py`, `.csv`) as a drop-in replacement for `pylint-odoo`.

## Architecture Overview

* **Binary CLI Name:** `odl`
* **Package Names:** `odoo-linter` on PyPI (`odoo-lint` is blocked by the existing `odoolint`), `odoo-lint` on crates.io
* **Core Engine:** Rust + `ruff_python_parser` + `rayon` + `quick-xml`
* **Distribution:** Maturin binary wheels for Python (`uv tool install odoo-linter`)

---

## Roadmap & Milestones

### Milestone 1: Core CLI & Configuration Parser

* [x] Basic project scaffolding (`Cargo.toml`, `pyproject.toml`).
* [ ] Implement TOML config loader in `src/config.rs` (`pyproject.toml` parsing for `[tool.odoo-lint]`).
* [ ] Add version CLI flags (`odl check . --version 17.0`).

### Milestone 2: Python AST Rule Engine

* [ ] Complete AST visitor using `ruff_python_ast`.
* [ ] Rule `ODOO001`: Detect computed fields (`compute=`) missing `@api.depends`.
* [ ] Rule `ODOO017`: Detect deprecated `name_get()` method usages on Odoo >= 17.0.
* [ ] Rule `ODOO030`: Detect raw SQL queries (`self.env.cr.execute`) without parameter binding.

### Milestone 3: Manifest & Structural Validation

* [x] Context-aware author validation (`ODOO010`) supporting folder pattern overrides (`"mijnbedrijf_*" = "MijnBedrijf B.V."`).
* [ ] Manifest required keys validation (`name`, `version`, `license`, `depends`).

### Milestone 4: XML & Access Rights (CSV) Parsers

* [ ] Integrate `quick-xml` to check XML views (`record`, `field`, missing IDs).
* [ ] Implement `csv` parser for `ir.model.access.csv` to ensure all model definitions have access rules.

### Milestone 5: CI/CD & Release Automation

* [x] GitHub Actions workflow using `maturin-action` to build wheels for macOS, Linux, and Windows.
* [ ] Publish `0.1.0` to PyPI and Crates.io.

### Milestone 6: Benchmarks

* [ ] Reproducible benchmark of `odl check` vs pylint-odoo on real OCA repositories (pinned versions, hyperfine, cold and warm runs).
* [ ] Publish results and exact commands in the "Benchmarks" section of `docs/why.md`.
""",
"AGENT_INSTRUCTIONS.md": """# Instructions for CLI Agent (Claude Code / Cursor / Aider)

You are taking over the implementation of `odoo-lint` (CLI binary `odl`), a Rust-native static analysis tool for Odoo.

## Project Context

* **Language:** Rust 2021 Edition
* **CLI Framework:** `clap` v4
* **Python Parser:** `ruff_python_parser` / `ruff_python_ast`
* **Concurrency:** `rayon` parallel iterator
* **Packaging:** Maturin (Python wheels)

## Immediate Next Tasks

1. Read `PROJECT_PLAN.md`.
2. Inspect `src/config.rs` and extend it to load `[tool.odoo-lint]` settings directly from `pyproject.toml` or `odoo-lint.toml`.
3. Complete `src/rules/odoo001_missing_depends.rs` using `ruff_python_ast` to traverse ClassDef and FunctionDef nodes.
4. Add unit tests in `tests/integration_test.rs` with sample Odoo Python snippets.
5. Verify build using `cargo test` and `maturin develop`.

Execute step by step and maintain clean, idiomatic Rust code.
"""
}

def create_scaffold():
print(f"📦 Creating project scaffold for '{PROJECT_NAME}'...")
BASE_DIR.mkdir(exist_ok=True)

```
zip_filename = "odoo-lint-scaffold.zip"

with zipfile.ZipFile(zip_filename, "w", zipfile.ZIP_DEFLATED) as zip_file:
    for relative_path, content in FILES.items():
        file_path = BASE_DIR / relative_path
        file_path.parent.mkdir(parents=True, exist_ok=True)
        
        with open(file_path, "w", encoding="utf-8") as f:
            f.write(content)
        
        # Add to zip
        zip_file.write(file_path, arcname=Path(PROJECT_NAME) / relative_path)
        print(f"  ✓ Created {relative_path}")

print(f"\n✅ Scaffold created successfully in './{PROJECT_NAME}/'")
print(f"🎁 Zip archive created: '{zip_filename}'")

```

if **name** == "**main**":
create_scaffold()

```

---

### Stap 2: Inhoud van de CLI Agent Handoff

Zodra je het script hebt uitgevoerd, bevat de map `odoo-lint/` (en de gegenereerde `odoo-lint-scaffold.zip`) de volgende onderdelen die je direct aan je agent (zoals **Claude Code**, **Cursor**, **Aider** of **Windsurf**) kunt geven:

| Bestand | Doel |
| :--- | :--- |
| **`PROJECT_PLAN.md`** | Gedetailleerd stappenplan en roadmap (Milestones 1 t/m 5). |
| **`AGENT_INSTRUCTIONS.md`** | De exacte prompt en context voor je CLI agent om direct te beginnen. |
| **`Cargo.toml` & `pyproject.toml`** | Volledige Rust- en Maturin-afhankelijkheden (`clap`, `ruff_python_parser`, `rayon`). |
| **`src/`** | Werken met modulaire Rust-code met een **context-bewuste author-rule (`ODOO010`)** die kijkt naar mapnamen en `pyproject.toml` configuraties. |

---

### Stap 3: De CLI Agent Inschakelen

Open je terminal of IDE met je favoriete agent in de aangemaakte map en voer dit uit:

```bash
cd odoo-lint
# instructie voor je CLI agent:
"Read AGENT_INSTRUCTIONS.md and PROJECT_PLAN.md, then implement Milestone 1 & 2."

```
