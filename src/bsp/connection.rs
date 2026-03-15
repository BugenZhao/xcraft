use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Serialize;

use super::config::BspConfig;

#[derive(Debug, Serialize)]
struct ConnectionFile {
    name: String,
    version: String,
    #[serde(rename = "bspVersion")]
    bsp_version: String,
    languages: Vec<String>,
    argv: Vec<String>,
}

pub fn write_connection_file(root: &Path, profile: Option<&str>) -> Result<()> {
    let exe = std::env::current_exe().context("failed to resolve current executable")?;
    let connection = ConnectionFile {
        name: "xcraft".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        bsp_version: BspConfig::bsp_version().to_string(),
        languages: vec!["swift".to_string()],
        argv: connection_argv(&exe, profile),
    };
    let content = serde_json::to_vec_pretty(&connection)?;
    let path = connection_path(root);
    fs::write(&path, &content).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

fn connection_argv(exe: &Path, profile: Option<&str>) -> Vec<String> {
    let mut argv = vec![
        exe.display().to_string(),
        "bsp".to_string(),
        "serve".to_string(),
    ];
    if let Some(profile) = profile {
        argv.push("--profile".to_string());
        argv.push(profile.to_string());
    }
    argv
}

fn connection_path(root: &Path) -> PathBuf {
    root.join("buildServer.json")
}
