use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::cache::CachedState;
use crate::util::{parse_cli_json, run_cmd};
use crate::workspace::{Workspace, WorkspaceType};

const BSP_VERSION: &str = "2.2.0";

/// BSP-only fields persisted under `[bsp]` inside `.xcraft/state[.profile].toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredBspState {
    /// The generated `.xcworkspace` used for `xcodebuild` when the logical
    /// workspace input is not directly buildable, e.g. Tuist `Project.swift`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generated_workspace: Option<String>,
    /// DerivedData root used to find activity logs and the index store.
    pub build_root: String,
}

/// Hydrated BSP config assembled from `.xcraft/state[.profile].toml`.
///
/// The serialized `[bsp]` section only stores fields unique to BSP. The logical
/// project identity still comes from the existing top-level `workspace` and
/// `scheme` fields so the state file does not duplicate them.
#[derive(Debug, Clone)]
pub struct BspConfig {
    /// The original workspace argument, resolved from the shared cache fields.
    pub workspace_input: String,
    /// Generated `.xcworkspace` used for build operations when needed.
    pub generated_workspace: Option<String>,
    pub scheme: String,
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
        profile: Option<&str>,
    ) -> Self {
        Self {
            workspace_input: absolutize(root, &input_ws.path).display().to_string(),
            generated_workspace: (input_ws.path != effective_ws.path)
                .then(|| absolutize(root, &effective_ws.path).display().to_string()),
            scheme: scheme.to_string(),
            build_root: build_root.display().to_string(),
            compile_db_path: compile_db_path(root, profile).display().to_string(),
        }
    }

    pub fn load(root: &Path, profile: Option<&str>) -> Result<Self> {
        let state = CachedState::load(root, profile);
        let stored = state
            .bsp
            .context("BSP is not initialized; run `xcraft bsp init`")?;
        let workspace_input = state
            .workspace
            .context("BSP workspace missing from state cache")?;
        let scheme = state
            .scheme
            .context("BSP scheme missing from state cache")?;

        Ok(Self {
            workspace_input: absolutize(root, Path::new(&workspace_input))
                .display()
                .to_string(),
            generated_workspace: stored
                .generated_workspace
                .as_deref()
                .map(|path| absolutize(root, Path::new(path)).display().to_string()),
            scheme,
            build_root: stored.build_root,
            compile_db_path: compile_db_path(root, profile).display().to_string(),
        })
    }

    pub fn save(&self, root: &Path, profile: Option<&str>) -> Result<()> {
        fs::create_dir_all(bsp_data_dir(root))?;
        let mut state = CachedState::load(root, profile);
        state.workspace = Some(relativize(root, Path::new(&self.workspace_input)));
        state.scheme = Some(self.scheme.clone());
        state.bsp = Some(StoredBspState {
            generated_workspace: self
                .generated_workspace
                .as_deref()
                .map(|path| relativize(root, Path::new(path))),
            build_root: self.build_root.clone(),
        });
        state
            .save(root, profile)
            .with_context(|| format!("failed to write {}", state_path(root, profile).display()))
    }

    pub fn matches(&self, input_ws: &Workspace, scheme: &str, root: &Path) -> bool {
        self.scheme == scheme
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

/// xcraft-owned BSP artifacts live under `.xcraft/bsp/` so they do not pollute
/// the standardized `.bsp/` entrypoint directory.
pub fn bsp_data_dir(root: &Path) -> PathBuf {
    root.join(".xcraft").join("bsp")
}

/// Location of the generated Swift compile database.
pub fn compile_db_path(root: &Path, profile: Option<&str>) -> PathBuf {
    match profile {
        Some(profile) => bsp_data_dir(root).join(format!("compile-db.{profile}.json")),
        None => bsp_data_dir(root).join("compile-db.json"),
    }
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

/// Mirror the existing cache convention: store project-local paths relative to
/// the workspace root when possible, and fall back to absolute paths otherwise.
pub fn relativize(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

fn state_path(root: &Path, profile: Option<&str>) -> PathBuf {
    match profile {
        Some(profile) => root.join(".xcraft").join(format!("state.{profile}.toml")),
        None => root.join(".xcraft").join("state.toml"),
    }
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
    use std::fs;

    use tempfile::tempdir;

    use crate::workspace::Workspace;

    use super::{BspConfig, build_root_from_symroot};

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

    #[test]
    fn bsp_state_reuses_top_level_workspace_and_scheme_fields() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let input_ws = Workspace::new(root.join("app").join("Project.swift"));
        let effective_ws = Workspace::new(root.join("app").join("App.xcworkspace"));
        let config = BspConfig::new(
            root,
            &input_ws,
            &effective_ws,
            "App",
            &root.join("DerivedData").join("App-123"),
            None,
        );

        config.save(root, None).unwrap();
        let state = fs::read_to_string(root.join(".xcraft").join("state.toml")).unwrap();
        assert!(state.contains("workspace = \"app/Project.swift\""));
        assert!(state.contains("scheme = \"App\""));
        assert!(state.contains("[bsp]"));
        assert!(state.contains("generated_workspace = \"app/App.xcworkspace\""));
        assert!(!state.contains("compile_db_path"));
    }

    #[test]
    fn xcode_bsp_state_omits_generated_workspace_and_uses_profile_paths() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let input_ws = Workspace::new(root.join("App.xcworkspace"));
        let config = BspConfig::new(
            root,
            &input_ws,
            &input_ws,
            "App",
            &root.join("DerivedData").join("App-123"),
            Some("sim"),
        );

        config.save(root, Some("sim")).unwrap();
        let state = fs::read_to_string(root.join(".xcraft").join("state.sim.toml")).unwrap();
        assert!(state.contains("[bsp]"));
        assert!(!state.contains("generated_workspace"));
        assert_eq!(
            BspConfig::load(root, Some("sim")).unwrap().compile_db_path,
            root.join(".xcraft")
                .join("bsp")
                .join("compile-db.sim.json")
                .display()
                .to_string()
        );
    }
}
