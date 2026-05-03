use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;

use crate::cmd::build::BuildArgs;
use crate::cmd::build::resolve_and_cache;
use crate::export;

#[derive(Parser)]
pub struct ExportArgs {
    #[command(flatten)]
    pub build: BuildArgs,

    /// Output directory for the exported IPA (default: ./export)
    #[arg(long, default_value = "export")]
    pub output: PathBuf,
}

pub fn cmd_export(args: ExportArgs) -> Result<()> {
    let resolved = resolve_and_cache(&args.build.action.resolve, args.build.action.configure)?;

    // Default to generic/platform=iOS for archive (device-agnostic).
    // If the user explicitly passed --destination, use that raw string instead.
    let default_dest = "generic/platform=iOS".to_string();
    let archive_dest_raw = args
        .build
        .action
        .resolve
        .destination
        .as_deref()
        .unwrap_or(&default_dest);

    let opts = export::ExportOptions {
        ws: &resolved.effective_ws,
        scheme: &resolved.scheme_name,
        configuration: &resolved.config,
        archive_dest_raw,
        derived_data: args.build.action.derived_data.as_deref(),
        skip_codesigning: args.build.skip_codesigning,
        xcbeautify: args.build.action.xcbeautify,
        extra_args: &args.build.build_args,
        extra_env: &args.build.build_env,
        output_dir: &args.output,
    };
    export::archive_and_export(&opts)?;

    Ok(())
}
