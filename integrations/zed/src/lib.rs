//! odoo-lint for Zed: runs `odl server` for Python, XML and PO files.
//!
//! The binary is, in order: `lsp.odoo-lint.binary.path` from the settings,
//! `odl` on the project's PATH (e.g. an activated virtualenv), or the latest
//! release of odoo-lint on GitHub, downloaded once.

use std::fs;
use zed_extension_api::settings::LspSettings;
use zed_extension_api::{self as zed, LanguageServerId, LanguageServerInstallationStatus, Result};

const SERVER: &str = "odoo-lint";
const REPO: &str = "bosd/odoo-lint";

struct OdooLint {
    cached: Option<String>,
}

/// The release asset for this platform, how it is packed, and `odl` in it.
fn asset() -> Result<(&'static str, zed::DownloadedFileType, &'static str)> {
    let (os, arch) = zed::current_platform();
    let tar = zed::DownloadedFileType::GzipTar;
    Ok(match (os, arch) {
        (zed::Os::Linux, zed::Architecture::X8664) => ("odl-x86_64-unknown-linux-gnu.tar.gz", tar, "odl"),
        (zed::Os::Linux, zed::Architecture::Aarch64) => ("odl-aarch64-unknown-linux-gnu.tar.gz", tar, "odl"),
        (zed::Os::Mac, zed::Architecture::X8664) => ("odl-x86_64-apple-darwin.tar.gz", tar, "odl"),
        (zed::Os::Mac, zed::Architecture::Aarch64) => ("odl-aarch64-apple-darwin.tar.gz", tar, "odl"),
        (zed::Os::Windows, zed::Architecture::X8664) => (
            "odl-x86_64-pc-windows-msvc.zip",
            zed::DownloadedFileType::Zip,
            "odl.exe",
        ),
        _ => return Err("odoo-lint: no build for this platform; install odl and put it on PATH".into()),
    })
}

impl OdooLint {
    /// Downloads `odl` from the latest GitHub release and returns its path.
    fn download(&mut self, id: &LanguageServerId) -> Result<String> {
        zed::set_language_server_installation_status(id, &LanguageServerInstallationStatus::CheckingForUpdate);
        let release = zed::latest_github_release(
            REPO,
            zed::GithubReleaseOptions {
                require_assets: true,
                // odoo-lint is in alpha: every release is a pre-release.
                pre_release: true,
            },
        )?;
        let (name, file_type, exe) = asset()?;
        let asset = release
            .assets
            .iter()
            .find(|asset| asset.name == name)
            .ok_or_else(|| format!("odoo-lint {} has no {name}", release.version))?;

        let dir = format!("odl-{}", release.version);
        let binary = format!("{dir}/{exe}");
        if !fs::metadata(&binary).is_ok_and(|m| m.is_file()) {
            zed::set_language_server_installation_status(id, &LanguageServerInstallationStatus::Downloading);
            zed::download_file(&asset.download_url, &dir, file_type)?;
            zed::make_file_executable(&binary)?;
            // Older downloads are not needed any more.
            for entry in fs::read_dir(".").into_iter().flatten().flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if (name.starts_with("odl-") || name.starts_with("odoo-linter-")) && name != dir {
                    fs::remove_dir_all(entry.path()).ok();
                }
            }
        }
        zed::set_language_server_installation_status(id, &LanguageServerInstallationStatus::None);
        Ok(binary)
    }

    fn binary(&mut self, id: &LanguageServerId, worktree: &zed::Worktree) -> Result<String> {
        let configured = LspSettings::for_worktree(SERVER, worktree)
            .ok()
            .and_then(|s| s.binary)
            .and_then(|b| b.path);
        if let Some(path) = configured {
            return Ok(path);
        }
        if let Some(path) = worktree.which("odl") {
            return Ok(path);
        }
        if let Some(path) = &self.cached {
            if fs::metadata(path).is_ok_and(|m| m.is_file()) {
                return Ok(path.clone());
            }
        }
        let path = self.download(id)?;
        self.cached = Some(path.clone());
        Ok(path)
    }
}

impl zed::Extension for OdooLint {
    fn new() -> Self {
        OdooLint { cached: None }
    }

    fn language_server_command(&mut self, id: &LanguageServerId, worktree: &zed::Worktree) -> Result<zed::Command> {
        let arguments = LspSettings::for_worktree(SERVER, worktree)
            .ok()
            .and_then(|s| s.binary)
            .and_then(|b| b.arguments)
            .unwrap_or_else(|| vec!["server".to_string()]);
        Ok(zed::Command {
            command: self.binary(id, worktree)?,
            args: arguments,
            env: worktree.shell_env(),
        })
    }
}

zed::register_extension!(OdooLint);
