//! `odoo-linter`: runs the `odl` next to it with the same arguments.
//!
//! The PyPI package is called odoo-linter, and MCP clients start a package by
//! its name (`uvx odoo-linter mcp`).

use std::env;
use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    let odl = match env::current_exe() {
        Ok(exe) => exe.with_file_name(format!("odl{}", env::consts::EXE_SUFFIX)),
        Err(err) => {
            eprintln!("odoo-linter: {err}");
            return ExitCode::FAILURE;
        }
    };
    let mut command = Command::new(&odl);
    command.args(env::args_os().skip(1));
    #[cfg(unix)]
    let err = {
        use std::os::unix::process::CommandExt;
        command.exec()
    };
    #[cfg(not(unix))]
    let err = match command.status() {
        Ok(status) => return ExitCode::from(status.code().unwrap_or(1) as u8),
        Err(err) => err,
    };
    eprintln!("odoo-linter: cannot run {}: {err}", odl.display());
    ExitCode::FAILURE
}
