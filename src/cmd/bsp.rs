use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};

use super::build::{BuildArgs, ResolveArgs, XcodeActionArgs, resolve_and_build};
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
    Sync(BspProfileArgs),
    /// Start the BSP server over stdio
    Serve(BspProfileArgs),
}

/// Shared options for `xcraft bsp init`.
#[derive(Parser)]
pub struct BspInitArgs {
    /// Path to .xcworkspace, Project.swift, or a Tuist project directory; if omitted,
    /// uses the cached workspace for the selected profile before prompting
    #[arg(long)]
    pub workspace: Option<PathBuf>,

    /// Scheme name; if omitted, uses the cached scheme for the selected profile before prompting
    #[arg(long)]
    pub scheme: Option<String>,

    /// Use a named profile for cached workspace/scheme defaults
    #[arg(long)]
    pub profile: Option<String>,

    /// Run an initial build after BSP initialization so compile metadata is guaranteed
    /// to exist for the selected profile
    #[arg(long)]
    pub build: bool,
}

/// Shared profile selector for BSP commands that operate on persisted state.
#[derive(Parser)]
pub struct BspProfileArgs {
    /// Use a named profile for BSP state and compile metadata
    #[arg(long)]
    pub profile: Option<String>,
}

/// Dispatch the `bsp` command family.
pub fn cmd_bsp(args: BspArgs) -> Result<()> {
    match args {
        BspArgs::Init(args) => cmd_bsp_init(args),
        BspArgs::Sync(args) => cmd_bsp_sync(args),
        BspArgs::Serve(args) => bsp::server::serve(args.profile.as_deref()),
    }
}

fn cmd_bsp_init(args: BspInitArgs) -> Result<()> {
    let root = CachedState::root()?;
    let state = CachedState::load(&root, args.profile.as_deref());
    let cached_workspace = state.workspace.as_ref().map(|p| root.join(p));
    let workspace_explicit = args.workspace.as_deref().or(cached_workspace.as_deref());
    let input_ws = workspace::resolve_workspace(workspace_explicit, cached_workspace.as_deref())?;
    if input_ws.ws_type == WorkspaceType::Spm {
        bail!("bsp init does not support SwiftPM projects");
    }

    let effective_ws = input_ws.ensure_generated()?;
    let scheme_explicit = args.scheme.as_deref().or(state.scheme.as_deref());
    let scheme = scheme::resolve_scheme(&effective_ws, scheme_explicit, state.scheme.as_deref())?;

    bsp::init(
        &root,
        &input_ws,
        &effective_ws,
        &scheme,
        args.profile.as_deref(),
    )?;
    eprintln!("Initialized BSP config:");
    eprintln!("  Connection: {}", root.join("buildServer.json").display());
    eprintln!(
        "  State:      {} [bsp]",
        state_path(&root, args.profile.as_deref()).display()
    );
    if args.build {
        resolve_and_build(&BuildArgs {
            action: XcodeActionArgs {
                configure: false,
                resolve: ResolveArgs {
                    workspace: Some(input_ws.path.clone()),
                    scheme: Some(scheme.clone()),
                    configuration: None,
                    destination: None,
                    profile: args.profile.clone(),
                },
                derived_data: None,
                xcbeautify: None,
            },
            allow_provisioning_updates: true,
            build_args: Vec::new(),
            skip_codesigning: false,
            build_env: Vec::new(),
        })?;
        eprintln!(
            "  Compile DB: {}",
            compile_db_path(&root, args.profile.as_deref()).display()
        );
    } else if let Err(err) = bsp::sync(&root, args.profile.as_deref()) {
        eprintln!("Warning: initial BSP sync failed: {err}");
    } else {
        eprintln!(
            "  Compile DB: {}",
            compile_db_path(&root, args.profile.as_deref()).display()
        );
    }
    Ok(())
}

fn cmd_bsp_sync(args: BspProfileArgs) -> Result<()> {
    let root = CachedState::root()?;
    bsp::sync(&root, args.profile.as_deref()).context("failed to sync compile metadata")?;
    eprintln!(
        "Updated compile database: {}",
        compile_db_path(&root, args.profile.as_deref()).display()
    );
    Ok(())
}

fn state_path(root: &std::path::Path, profile: Option<&str>) -> PathBuf {
    match profile {
        Some(profile) => root.join(".xcraft").join(format!("state.{profile}.toml")),
        None => root.join(".xcraft").join("state.toml"),
    }
}

fn compile_db_path(root: &std::path::Path, profile: Option<&str>) -> PathBuf {
    match profile {
        Some(profile) => root
            .join(".xcraft")
            .join("bsp")
            .join(format!("compile-db.{profile}.json")),
        None => root.join(".xcraft").join("bsp").join("compile-db.json"),
    }
}
