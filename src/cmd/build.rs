use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;

use crate::destination::Destination;
use crate::workspace::Workspace;
use crate::{bsp, build, cache, destination, scheme, workspace};

/// Shared options for resolving workspace, scheme, configuration, and destination.
#[derive(Parser)]
pub struct ResolveArgs {
    /// Path to .xcworkspace or Package.swift; if omitted, uses cached value or prompts for selection
    #[arg(long)]
    pub workspace: Option<PathBuf>,

    /// Scheme name; if omitted, uses cached value or prompts for selection
    #[arg(long)]
    pub scheme: Option<String>,

    /// Build configuration (default: Debug); if omitted, uses cached value or prompts for selection
    #[arg(long)]
    pub configuration: Option<String>,

    /// Destination spec: "simulator:<udid>", "device:<udid>", or "macos"; if omitted, uses cached value or prompts for selection
    #[arg(long)]
    pub destination: Option<String>,

    /// Use a named profile for cached selections (stored separately from the default cache)
    #[arg(long)]
    pub profile: Option<String>,
}

/// Shared options for any xcodebuild action (build, clean, etc.).
#[derive(Parser)]
pub struct XcodeActionArgs {
    /// Ignore cached selections and re-prompt for all options (selections are still saved)
    #[arg(long)]
    pub configure: bool,

    #[command(flatten)]
    pub resolve: ResolveArgs,

    /// Path to derived data
    #[arg(long)]
    pub derived_data: Option<String>,

    /// Pipe output through xcbeautify (auto-detected from PATH if not specified)
    #[arg(long, num_args = 0..=1, default_missing_value = "true")]
    pub xcbeautify: Option<bool>,
}

#[derive(Parser)]
pub struct BuildArgs {
    #[command(flatten)]
    pub action: XcodeActionArgs,

    /// Allow provisioning updates (default: true)
    #[arg(long, default_value_t = true)]
    pub allow_provisioning_updates: bool,

    /// Extra build arguments (repeatable)
    #[arg(long = "build-arg")]
    pub build_args: Vec<String>,

    /// Skip code signing (sets CODE_SIGN_IDENTITY='', CODE_SIGNING_REQUIRED=NO, CODE_SIGNING_ALLOWED=NO)
    #[arg(long)]
    pub skip_codesigning: bool,

    /// Extra build environment KEY=VALUE (repeatable)
    #[arg(long = "build-env", value_parser = parse_key_val)]
    pub build_env: Vec<(String, String)>,
}

fn parse_key_val(s: &str) -> Result<(String, String), String> {
    let pos = s
        .find('=')
        .ok_or_else(|| format!("invalid KEY=VALUE: no `=` found in `{s}`"))?;
    Ok((s[..pos].to_string(), s[pos + 1..].to_string()))
}

pub struct ResolvedBuild {
    pub input_ws: Workspace,
    /// The workspace to use for xcodebuild operations (same as original for Xcode/SPM,
    /// the generated .xcworkspace for Tuist).
    pub effective_ws: Workspace,
    pub scheme_name: String,
    pub config: String,
    pub dest: Destination,
}

/// Resolve inputs (with optional re-prompting) and save to cache.
pub fn resolve_and_cache(args: &ResolveArgs, configure: bool) -> Result<ResolvedBuild> {
    let cache_root = cache::CachedState::root()?;
    let profile = args.profile.as_deref();
    let mut state = cache::CachedState::load(&cache_root, profile);

    // When `configure`, cached values become default hints (pre-selected in
    // prompts) instead of being used as explicit values (which skip prompts).
    // The `default` parameter is harmless when `explicit` is set (early return).
    let cached_ws_path = state.workspace.as_ref().map(|p| cache_root.join(p));
    let ws_explicit = if configure {
        args.workspace.as_deref()
    } else {
        args.workspace.as_deref().or(cached_ws_path.as_deref())
    };
    let ws = workspace::resolve_workspace(ws_explicit, cached_ws_path.as_deref())?;
    let effective_ws = ws.ensure_generated()?;

    let scheme_explicit = if configure {
        args.scheme.as_deref()
    } else {
        args.scheme.as_deref().or(state.scheme.as_deref())
    };
    let scheme_name =
        scheme::resolve_scheme(&effective_ws, scheme_explicit, state.scheme.as_deref())?;

    let config_explicit = if configure {
        args.configuration.as_deref()
    } else {
        args.configuration
            .as_deref()
            .or(state.configuration.as_deref())
    };
    let config = scheme::resolve_configuration(
        &effective_ws,
        config_explicit,
        state.configuration.as_deref(),
    )?;

    let dest_explicit = args.destination.as_deref();
    let dest = if let Some(spec) = dest_explicit {
        destination::resolve_destination(Some(spec), None)?
    } else if let Some(cached) = &state.destination {
        if configure {
            destination::resolve_destination(None, Some(cached))?
        } else {
            cached.clone()
        }
    } else {
        destination::resolve_destination(None, None)?
    };

    if let Some(p) = profile {
        eprintln!("Profile:       {p}");
    }
    eprintln!("Workspace:     {}", ws.path.display());
    eprintln!("Scheme:        {scheme_name}");
    eprintln!("Configuration: {config}");
    eprintln!("Destination:   {dest}");
    eprintln!();

    // Save resolved values to cache.
    state.workspace = Some(
        ws.path
            .strip_prefix(&cache_root)
            .unwrap_or(&ws.path)
            .display()
            .to_string(),
    );
    state.scheme = Some(scheme_name.clone());
    state.configuration = Some(config.clone());
    state.destination = Some(dest.clone());
    if let Err(e) = state.save(&cache_root, profile) {
        eprintln!("Warning: failed to save cache: {e}");
    }

    Ok(ResolvedBuild {
        input_ws: ws,
        effective_ws,
        scheme_name,
        config,
        dest,
    })
}

/// Resolve inputs, build, and return the resolved state.
pub fn resolve_and_build(args: &BuildArgs) -> Result<ResolvedBuild> {
    let cache_root = cache::CachedState::root()?;
    let resolved = resolve_and_cache(&args.action.resolve, args.action.configure)?;

    let dest_raw = resolved.dest.xcodebuild_destination_string();

    let build_opts = build::BuildOptions {
        ws: &resolved.effective_ws,
        scheme: &resolved.scheme_name,
        configuration: &resolved.config,
        destination_raw: &dest_raw,
        derived_data: args.action.derived_data.as_deref(),
        allow_provisioning_updates: args.allow_provisioning_updates,
        skip_codesigning: args.skip_codesigning,
        xcbeautify: args.action.xcbeautify,
        extra_args: &args.build_args,
        extra_env: &args.build_env,
    };
    build::build(&build_opts)?;
    if let Err(err) = bsp::maybe_sync_after_build(
        &cache_root,
        args.action.resolve.profile.as_deref(),
        &resolved.input_ws,
        &resolved.scheme_name,
    ) {
        eprintln!("Warning: failed to sync BSP compile metadata: {err}");
    }

    Ok(resolved)
}

pub fn cmd_build(args: BuildArgs) -> Result<()> {
    resolve_and_build(&args)?;
    Ok(())
}
