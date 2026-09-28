use std::fs;

use zed_extension_api::settings::LspSettings;
use zed_extension_api::{
    self as zed, Architecture, Command, DownloadedFileType, GithubReleaseOptions, LanguageServerId,
    LanguageServerInstallationStatus, Os, Result, Worktree,
};

// Downloads release binaries named `jbehave-language-server-<os>-<arch>.<ext>`
// from this repo once CI publishes them (see server/README.md for the exact
// naming this expects).
const SERVER_REPO: &str = "HenrikNilsson/zed-jbehave";
const SERVER_BINARY_NAME: &str = "jbehave-language-server";
const LSP_SERVER_NAME: &str = "jbehave-language-server";

struct JBehaveExtension {
    cached_binary_path: Option<String>,
}

fn asset_name_for(os: Os, arch: Architecture) -> (String, DownloadedFileType) {
    let os_str = match os {
        Os::Mac => "macos",
        Os::Linux => "linux",
        Os::Windows => "windows",
    };
    let arch_str = match arch {
        Architecture::Aarch64 => "aarch64",
        Architecture::X86 => "x86",
        Architecture::X8664 => "x86_64",
    };
    match os {
        Os::Windows => (
            format!("{SERVER_BINARY_NAME}-{os_str}-{arch_str}.zip"),
            DownloadedFileType::Zip,
        ),
        _ => (
            format!("{SERVER_BINARY_NAME}-{os_str}-{arch_str}.tar.gz"),
            DownloadedFileType::GzipTar,
        ),
    }
}

impl JBehaveExtension {
    /// Locates (downloading if necessary) the `jbehave-language-server`
    /// binary. Prefers one already on `$PATH` so local development builds
    /// (`cargo build` in `server/`, then symlinked/installed) are picked up
    /// without needing a GitHub release.
    fn language_server_binary_path(
        &mut self,
        language_server_id: &LanguageServerId,
        worktree: &Worktree,
    ) -> Result<String> {
        if let Some(path) = worktree.which(SERVER_BINARY_NAME) {
            return Ok(path);
        }

        if let Some(path) = &self.cached_binary_path
            && fs::metadata(path).is_ok()
        {
            return Ok(path.clone());
        }

        zed::set_language_server_installation_status(
            language_server_id,
            &LanguageServerInstallationStatus::CheckingForUpdate,
        );

        let release = zed::latest_github_release(
            SERVER_REPO,
            GithubReleaseOptions {
                require_assets: true,
                pre_release: false,
            },
        )?;

        let (os, arch) = zed::current_platform();
        let (asset_name, file_type) = asset_name_for(os, arch);
        let asset = release
            .assets
            .iter()
            .find(|a| a.name == asset_name)
            .ok_or_else(|| {
                format!(
                    "no release asset named `{asset_name}` found in {SERVER_REPO} {}",
                    release.version
                )
            })?;

        let version_dir = format!("{SERVER_BINARY_NAME}-{}", release.version);
        let binary_name = if matches!(os, Os::Windows) {
            format!("{SERVER_BINARY_NAME}.exe")
        } else {
            SERVER_BINARY_NAME.to_string()
        };
        let binary_path = format!("{version_dir}/{binary_name}");

        if fs::metadata(&binary_path).is_err() {
            zed::set_language_server_installation_status(
                language_server_id,
                &LanguageServerInstallationStatus::Downloading,
            );
            zed::download_file(&asset.download_url, &version_dir, file_type)?;
            zed::make_file_executable(&binary_path)?;
        }

        zed::set_language_server_installation_status(
            language_server_id,
            &LanguageServerInstallationStatus::None,
        );

        self.cached_binary_path = Some(binary_path.clone());
        Ok(binary_path)
    }
}

impl zed::Extension for JBehaveExtension {
    fn new() -> Self {
        Self {
            cached_binary_path: None,
        }
    }

    fn language_server_command(
        &mut self,
        language_server_id: &LanguageServerId,
        worktree: &Worktree,
    ) -> Result<Command> {
        let path = self.language_server_binary_path(language_server_id, worktree)?;
        Ok(Command {
            command: path,
            args: Vec::new(),
            env: Vec::new(),
        })
    }

    /// Forwards the user's `"lsp": { "jbehave-language-server": { "settings":
    /// {...} } }` block verbatim to the language server. Zed sends it as the
    /// `settings` of a `workspace/didChangeConfiguration` notification right
    /// after the server initializes, so the server can pick up e.g. a custom
    /// `parameter_prefix` without restarting.
    fn language_server_workspace_configuration(
        &mut self,
        _language_server_id: &LanguageServerId,
        worktree: &Worktree,
    ) -> Result<Option<zed::serde_json::Value>> {
        let settings = LspSettings::for_worktree(LSP_SERVER_NAME, worktree)
            .ok()
            .and_then(|lsp| lsp.settings);
        Ok(settings)
    }
}

zed::register_extension!(JBehaveExtension);
