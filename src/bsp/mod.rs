pub mod compile_db;
pub mod config;
pub mod connection;
pub mod parser;
pub mod server;
pub mod xcactivitylog;

use std::path::Path;

use anyhow::{Context, Result};

use crate::workspace::Workspace;

pub use config::{BspConfig, derive_build_root};

/// Initialize BSP metadata for the current project root.
/// This writes `buildServer.json` in the workspace root and the xcraft-specific
/// state under `.xcraft/`.
pub fn init(
    root: &Path,
    input_ws: &Workspace,
    effective_ws: &Workspace,
    scheme: &str,
    profile: Option<&str>,
) -> Result<()> {
    let build_root = derive_build_root(effective_ws, scheme)?;
    let config = BspConfig::new(root, input_ws, effective_ws, scheme, &build_root, profile);
    config.save(root, profile)?;
    connection::write_connection_file(root, profile)?;
    Ok(())
}

pub fn sync(root: &Path, profile: Option<&str>) -> Result<()> {
    let config = BspConfig::load(root, profile)?;
    sync_with_config(&config)
}

/// Refresh the compile database from the newest usable Xcode activity log.
pub fn sync_with_config(config: &BspConfig) -> Result<()> {
    let existing_db = compile_db::CompileDb::load_json(Path::new(&config.compile_db_path)).ok();
    let mut db = existing_db.unwrap_or_else(|| compile_db::CompileDb::new(Vec::new()));
    let mut parsed_any = false;
    let mut last_error = None;

    // Xcode frequently leaves behind truncated or metadata-only activity logs, so
    // merge candidates from newest to oldest instead of letting the newest incremental
    // build erase previously discovered compile units.
    for log_path in
        xcactivitylog::candidate_log_paths(Path::new(&config.build_root), Some(&config.scheme))?
    {
        match xcactivitylog::extract_compile_lines(&log_path) {
            Ok(lines) => match parser::parse_compile_db(&lines) {
                Ok(parsed) if !parsed.swift_units.is_empty() => {
                    db = db.merged_with(parsed);
                    parsed_any = true;
                }
                Ok(_) => {}
                Err(err) => last_error = Some((log_path, err)),
            },
            Err(err) => {
                last_error = Some((log_path, err));
            }
        }
    }

    if !parsed_any
        && db.swift_units.is_empty()
        && let Some((path, err)) = last_error
    {
        return Err(err).with_context(|| format!("failed to parse {}", path.display()));
    }

    db.save(Path::new(&config.compile_db_path))
        .with_context(|| format!("failed to save compile db to {}", config.compile_db_path))?;
    Ok(())
}

/// Sync after a successful build when the cached BSP config matches the build inputs.
pub fn maybe_sync_after_build(
    root: &Path,
    profile: Option<&str>,
    input_ws: &Workspace,
    scheme: &str,
) -> Result<()> {
    let Ok(config) = BspConfig::load(root, profile) else {
        return Ok(());
    };

    if !config.matches(input_ws, scheme, root) {
        return Ok(());
    }

    sync_with_config(&config)
}
