use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};

use crate::bsp;
use crate::cache::CachedState;
use crate::scheme;
use crate::workspace::{self, WorkspaceType};

/// `xcraft bsp ...` subcommands for BSP / SourceKit-LSP integration.
#[derive(Subcommand)]
pub enum BspArgs {
    /// Initialize BSP config and connection files
    Init(BspInitArgs),
    /// Refresh compile metadata from the latest Xcode build log
    Sync,
    /// Start the BSP server over stdio
    Serve,
}

/// Shared options for `xcraft bsp init`.
#[derive(Parser)]
pub struct BspInitArgs {
    /// Path to .xcworkspace, Project.swift, or a Tuist project directory
    #[arg(long)]
    pub workspace: Option<PathBuf>,

    /// Scheme name; if omitted, uses cached value or prompts for selection
    #[arg(long)]
    pub scheme: Option<String>,

    /// Use a named profile for cached workspace/scheme defaults
    #[arg(long)]
    pub profile: Option<String>,
}

/// Dispatch the `bsp` command family.
pub fn cmd_bsp(args: BspArgs) -> Result<()> {
    match args {
        BspArgs::Init(args) => cmd_bsp_init(args),
        BspArgs::Sync => cmd_bsp_sync(),
        BspArgs::Serve => bsp::server::serve(),
    }
}

fn cmd_bsp_init(args: BspInitArgs) -> Result<()> {
    let root = CachedState::root()?;
    let state = CachedState::load(&root, args.profile.as_deref());
    let default_workspace = state.workspace.as_ref().map(|p| root.join(p));
    let input_ws =
        workspace::resolve_workspace(args.workspace.as_deref(), default_workspace.as_deref())?;
    if input_ws.ws_type == WorkspaceType::Spm {
        bail!("bsp init does not support SwiftPM projects");
    }

    let effective_ws = input_ws.ensure_generated()?;
    let default_scheme = state.scheme.as_deref();
    let scheme = scheme::resolve_scheme(&effective_ws, args.scheme.as_deref(), default_scheme)?;

    bsp::init(&root, &input_ws, &effective_ws, &scheme)?;
    eprintln!("Initialized BSP config:");
    eprintln!(
        "  Connection: {}",
        root.join(".bsp").join("xcraft.json").display()
    );
    eprintln!(
        "  Config:     {}",
        root.join(".xcraft").join("bsp.toml").display()
    );
    Ok(())
}

fn cmd_bsp_sync() -> Result<()> {
    let root = CachedState::root()?;
    bsp::sync(&root).context("failed to sync compile metadata")?;
    eprintln!(
        "Updated compile database: {}",
        root.join(".xcraft")
            .join("bsp")
            .join("compile-db.json")
            .display()
    );
    Ok(())
}
