use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::bsp::config::StoredBspState;
use crate::destination::Destination;

const CACHE_DIR: &str = ".xcraft";

fn cache_file(profile: Option<&str>) -> String {
    match profile {
        Some(p) => format!("state.{p}.toml"),
        None => "state.toml".to_string(),
    }
}

/// Persisted state from the last `launch` invocation.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct CachedState {
    pub workspace: Option<String>,
    pub scheme: Option<String>,
    pub configuration: Option<String>,
    pub destination: Option<Destination>,
    /// BSP-specific state lives under `[bsp]`, while reusing the top-level
    /// `workspace` and `scheme` fields as the logical project key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bsp: Option<StoredBspState>,
}

impl CachedState {
    /// Load cached state from `.xcraft/state[.profile].toml` relative to `root`.
    pub fn load(root: &Path, profile: Option<&str>) -> Self {
        let path = root.join(CACHE_DIR).join(cache_file(profile));
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// Save cached state to `.xcraft/state[.profile].toml` relative to `root`.
    pub fn save(&self, root: &Path, profile: Option<&str>) -> Result<()> {
        let dir = root.join(CACHE_DIR);
        std::fs::create_dir_all(&dir)?;
        let content = toml::to_string_pretty(self)?;
        std::fs::write(dir.join(cache_file(profile)), content)?;
        Ok(())
    }

    /// Root directory for the cache (current working directory).
    pub fn root() -> Result<PathBuf> {
        Ok(std::env::current_dir()?)
    }

    /// Remove the cache file. Returns `Ok(true)` if the file was removed,
    /// `Ok(false)` if it didn't exist.
    pub fn reset(root: &Path, profile: Option<&str>) -> Result<bool> {
        let path = root.join(CACHE_DIR).join(cache_file(profile));
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e.into()),
        }
    }
}
