//! odoo-lint for Zed: runs `odl server` for Python files.
//!
//! The binary is, in order: `lsp.odoo-lint.binary.path` from the settings,
//! `odl` on the project's PATH (e.g. an activated virtualenv), or the latest
//! `odoo-linter` wheel from PyPI, downloaded once.

use std::fs;
use zed_extension_api::http_client::{HttpMethod, HttpRequest, RedirectPolicy};
use zed_extension_api::settings::LspSettings;
use zed_extension_api::{self as zed, LanguageServerId, LanguageServerInstallationStatus, Result};

const SERVER: &str = "odoo-lint";

struct OdooLint {
    cached: Option<String>,
}

/// The wheel tag fragments that run on this platform.
fn wheel_tags() -> Result<&'static [&'static str]> {
    let (os, arch) = zed::current_platform();
    Ok(match (os, arch) {
        (zed::Os::Linux, zed::Architecture::X8664) => &["manylinux", "x86_64"],
        (zed::Os::Linux, zed::Architecture::Aarch64) => &["manylinux", "aarch64"],
        (zed::Os::Mac, zed::Architecture::Aarch64) => &["macosx", "arm64"],
        (zed::Os::Mac, zed::Architecture::X8664) => &["macosx", "x86_64"],
        (zed::Os::Windows, zed::Architecture::X8664) => &["win_amd64"],
        _ => return Err("odoo-lint: no build for this platform; install odl and put it on PATH".into()),
    })
}

impl OdooLint {
    /// Downloads the latest wheel from PyPI and returns the path of `odl` in it.
    fn download(&mut self, id: &LanguageServerId) -> Result<String> {
        zed::set_language_server_installation_status(id, &LanguageServerInstallationStatus::CheckingForUpdate);
        let response = HttpRequest::builder()
            .method(HttpMethod::Get)
            .url("https://pypi.org/pypi/odoo-linter/json")
            .redirect_policy(RedirectPolicy::FollowAll)
            .build()?
            .fetch()?;
        let index: serde_json::Value = serde_json::from_slice(&response.body).map_err(|e| format!("PyPI: {e}"))?;
        let version = index["info"]["version"].as_str().ok_or("PyPI: no version")?.to_string();
        let tags = wheel_tags()?;
        let wheel = index["urls"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|file| {
                let name = file["filename"].as_str().unwrap_or_default();
                name.ends_with(".whl") && tags.iter().all(|tag| name.contains(tag))
            })
            .ok_or_else(|| format!("odoo-linter {version} has no wheel for this platform"))?;
        let url = wheel["url"].as_str().ok_or("PyPI: no wheel URL")?;

        let dir = format!("odoo-linter-{version}");
        let exe = if tags == ["win_amd64"] { "odl.exe" } else { "odl" };
        let binary = format!("{dir}/odoo_linter-{version}.data/scripts/{exe}");
        if !fs::metadata(&binary).is_ok_and(|m| m.is_file()) {
            zed::set_language_server_installation_status(id, &LanguageServerInstallationStatus::Downloading);
            zed::download_file(url, &dir, zed::DownloadedFileType::Zip)?;
            zed::make_file_executable(&binary)?;
            // Older downloads are not needed any more.
            for entry in fs::read_dir(".").into_iter().flatten().flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with("odoo-linter-") && name != dir {
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
