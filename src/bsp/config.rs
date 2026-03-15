use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::util::{parse_cli_json, run_cmd};
use crate::workspace::{Workspace, WorkspaceType};

const BSP_VERSION: &str = "2.2.0";

/// Persisted BSP state stored under `.xcraft/bsp.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BspConfig {
    /// The workspace kind chosen by the user (`xcode` or `tuist` in v1).
    pub workspace_kind: WorkspaceType,
    /// The original workspace argument, kept so build hooks can verify they are
    /// refreshing metadata for the same logical project.
    pub workspace_input: String,
    /// The real `.xcworkspace` used for `xcodebuild` and BSP source roots.
    pub workspace_effective: String,
    pub scheme: String,
    /// DerivedData root used to find activity logs and the index store.
    pub build_root: String,
    /// On-disk compile metadata consumed by `xcraft bsp serve`.
    pub compile_db_path: String,
}

impl BspConfig {
    pub fn new(
        root: &Path,
        input_ws: &Workspace,
        effective_ws: &Workspace,
        scheme: &str,
        build_root: &Path,
    ) -> Self {
        Self {
            workspace_kind: input_ws.ws_type,
            workspace_input: absolutize(root, &input_ws.path).display().to_string(),
            workspace_effective: absolutize(root, &effective_ws.path).display().to_string(),
            scheme: scheme.to_string(),
            build_root: build_root.display().to_string(),
            compile_db_path: compile_db_path(root).display().to_string(),
        }
    }

    pub fn load(root: &Path) -> Result<Self> {
        let path = config_path(root);
        let content = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        toml::from_str(&content).with_context(|| format!("failed to parse {}", path.display()))
    }

    pub fn save(&self, root: &Path) -> Result<()> {
        fs::create_dir_all(bsp_data_dir(root))?;
        let path = config_path(root);
        fs::write(&path, toml::to_string_pretty(self)?)
            .with_context(|| format!("failed to write {}", path.display()))
    }

    pub fn matches(&self, input_ws: &Workspace, scheme: &str, root: &Path) -> bool {
        self.workspace_kind == input_ws.ws_type
            && self.scheme == scheme
            && Path::new(&self.workspace_input) == absolutize(root, &input_ws.path)
    }

    pub fn index_store_path(&self) -> PathBuf {
        Path::new(&self.build_root)
            .join("Index.noindex")
            .join("DataStore")
    }

    pub fn bsp_version() -> &'static str {
        BSP_VERSION
    }
}

pub fn bsp_dir(root: &Path) -> PathBuf {
    root.join(".bsp")
}

/// xcraft-owned BSP artifacts live under `.xcraft/bsp/` so they do not pollute
/// the standardized `.bsp/` entrypoint directory.
pub fn bsp_data_dir(root: &Path) -> PathBuf {
    root.join(".xcraft").join("bsp")
}

/// Location of the persisted BSP state file.
pub fn config_path(root: &Path) -> PathBuf {
    root.join(".xcraft").join("bsp.toml")
}

/// Location of the generated Swift compile database.
pub fn compile_db_path(root: &Path) -> PathBuf {
    bsp_data_dir(root).join("compile-db.json")
}

/// Reuse the same directory namespace for the sourcekit index database cache.
pub fn index_db_dir(root: &Path) -> PathBuf {
    bsp_data_dir(root)
}

/// Resolve paths eagerly relative to the project root so the persisted config is stable
/// regardless of the cwd used to invoke xcraft later.
pub fn absolutize(root: &Path, path: &Path) -> PathBuf {
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    fs::canonicalize(&joined).unwrap_or(joined)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BuildSettingsEntry {
    build_settings: serde_json::Map<String, serde_json::Value>,
}

/// Resolve the DerivedData root by reading `SYMROOT` from `xcodebuild -showBuildSettings`.
pub fn derive_build_root(ws: &Workspace, scheme: &str) -> Result<PathBuf> {
    if ws.ws_type != WorkspaceType::Xcode {
        bail!("bsp init expects an effective Xcode workspace");
    }

    let output = run_cmd(
        Command::new("xcodebuild")
            .args(["-showBuildSettings", "-json", "-workspace"])
            .arg(&ws.path)
            .args(["-scheme", scheme]),
    )?;
    let settings: Vec<BuildSettingsEntry> = parse_cli_json(&output)?;
    let symroot = settings
        .first()
        .and_then(|entry| entry.build_settings.get("SYMROOT"))
        .and_then(|value| value.as_str())
        .context("SYMROOT not found in xcodebuild settings")?;
    Ok(build_root_from_symroot(symroot))
}

/// `SYMROOT` points at `.../Build/Products`, while BSP needs the DerivedData root.
pub fn build_root_from_symroot(symroot: &str) -> PathBuf {
    let symroot = PathBuf::from(symroot);
    normalize_lexically(&symroot.join("../.."))
}

fn normalize_lexically(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            // Keep lexical normalization local so the path can be derived even if
            // the target directory does not exist yet.
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::build_root_from_symroot;

    #[test]
    fn build_root_is_derived_from_symroot() {
        let actual = build_root_from_symroot(
            "/Users/me/Library/Developer/Xcode/DerivedData/App-123/Build/Products",
        );
        assert_eq!(
            actual,
            std::path::PathBuf::from("/Users/me/Library/Developer/Xcode/DerivedData/App-123")
        );
    }
}
