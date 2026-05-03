mod bsp;
mod build;
mod cache;
mod cmd;
mod destination;
mod export;
mod launch;
mod scheme;
mod util;
mod workspace;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "xcraft",
    version,
    author = "Bugen Zhao",
    about = "CLI for building & running Xcode projects"
)]
struct Cli {
    /// Enable verbose output (print executed commands)
    #[arg(long, short, global = true)]
    verbose: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// List available workspaces (.xcworkspace / Package.swift)
    Workspaces {
        /// Output as JSON with name and arg fields
        #[arg(long)]
        json: bool,
    },

    /// List schemes for a workspace
    Schemes {
        /// Path to .xcworkspace or Package.swift
        #[arg(long)]
        workspace: Option<PathBuf>,
        /// Output as JSON with name and arg fields
        #[arg(long)]
        json: bool,
    },

    /// List build configurations for a workspace
    Configs {
        /// Path to .xcworkspace or Package.swift
        #[arg(long)]
        workspace: Option<PathBuf>,
        /// Output as JSON with name and arg fields
        #[arg(long)]
        json: bool,
    },

    /// List available destinations (simulators, devices, macOS)
    Destinations {
        /// Output as JSON with name and arg fields
        #[arg(long)]
        json: bool,
    },

    /// Interactively select and cache workspace, scheme, configuration, and destination
    Configure(cmd::build::ResolveArgs),

    /// Clear cached selections
    Reset {
        /// Clear a named profile's cache instead of the default cache
        #[arg(long)]
        profile: Option<String>,
    },

    /// Build the project without launching
    Build(cmd::build::BuildArgs),

    /// Clean build products
    Clean(cmd::clean::CleanArgs),

    /// Build and run the project
    Launch(cmd::launch::LaunchArgs),

    /// Archive and export an IPA
    Export(cmd::export::ExportArgs),

    /// Build Server Protocol integration (xcode-build-server)
    Bsp {
        #[command(subcommand)]
        command: BspCommands,
    },
}

#[derive(Subcommand)]
enum BspCommands {
    /// Generate buildServer.json from current xcraft state
    #[command(alias = "config")]
    Configure(cmd::bsp::BspConfigArgs),
    /// Start BSP server (proxies to xcode-build-server)
    Serve(cmd::bsp::BspServeArgs),
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    util::set_verbose(cli.verbose);

    match cli.command {
        Commands::Workspaces { json } => cmd::cmd_workspaces(json),
        Commands::Schemes { workspace, json } => cmd::cmd_schemes(workspace, json),
        Commands::Configs { workspace, json } => cmd::cmd_configs(workspace, json),
        Commands::Destinations { json } => cmd::cmd_destinations(json),
        Commands::Configure(args) => cmd::cmd_configure(args),
        Commands::Reset { profile } => cmd::cmd_reset(profile),
        Commands::Build(args) => cmd::cmd_build(args),
        Commands::Clean(args) => cmd::cmd_clean(args),
        Commands::Launch(args) => cmd::cmd_launch(args),
        Commands::Export(args) => cmd::cmd_export(args),
        Commands::Bsp { command } => match command {
            BspCommands::Configure(args) => cmd::cmd_bsp_config(args),
            BspCommands::Serve(args) => cmd::cmd_bsp_serve(args),
        },
    }
}
