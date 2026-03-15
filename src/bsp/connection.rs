use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::Serialize;

use super::config::{BspConfig, bsp_dir};

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
    fs::create_dir_all(bsp_dir(root))?;
    let path = bsp_dir(root).join("xcraft.json");
    let exe = std::env::current_exe().context("failed to resolve current executable")?;
    let connection = ConnectionFile {
        name: "xcraft".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        bsp_version: BspConfig::bsp_version().to_string(),
        languages: vec!["swift".to_string()],
        argv: connection_argv(&exe, profile),
    };
    fs::write(&path, serde_json::to_vec_pretty(&connection)?)
        .with_context(|| format!("failed to write {}", path.display()))
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
