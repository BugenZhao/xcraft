use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use tempfile::NamedTempFile;

use crate::build::{self, BuildSettingsEntry};
use crate::workspace::{Workspace, WorkspaceType};

/// Options for archiving and exporting an IPA.
pub struct ExportOptions<'a> {
    pub ws: &'a Workspace,
    pub scheme: &'a str,
    pub configuration: &'a str,
    /// Raw `-destination` string for `xcodebuild archive`
    /// (e.g. `generic/platform=iOS`).
    pub archive_dest_raw: &'a str,
    pub derived_data: Option<&'a str>,
    pub skip_codesigning: bool,
    pub xcbeautify: Option<bool>,
    pub extra_args: &'a [String],
    pub extra_env: &'a [(String, String)],
    pub output_dir: &'a Path,
}

/// Archive the project and export an IPA file.
///
/// Returns the path to the exported `.ipa` file.
pub fn archive_and_export(opts: &ExportOptions) -> Result<PathBuf> {
    // 1. Query build settings to determine signing and product name.
    let entries = build::get_build_settings(
        opts.ws,
        opts.scheme,
        opts.configuration,
        Some(opts.archive_dest_raw),
        opts.derived_data,
    )
    .context("failed to get build settings")?;
    let entry = entries
        .first()
        .context("no build settings returned by xcodebuild")?;

    let team_id = setting_str(entry, "DEVELOPMENT_TEAM");
    let signed = !opts.skip_codesigning && team_id.as_deref().is_some_and(|t| !t.is_empty());
    let team_id = team_id.unwrap_or_default();

    let product_name = setting_str(entry, "PRODUCT_NAME")
        .or_else(|| setting_str(entry, "WRAPPER_NAME"))
        .unwrap_or_else(|| "App".to_string());

    // 2. Archive.
    let archive_dir = tempfile::TempDir::new()?;
    let archive_path = archive_dir.path().join("archive.xcarchive");

    eprintln!(
        "Export mode:   {}",
        if signed {
            format!("signed (team: {team_id})")
        } else {
            "unsigned".to_string()
        }
    );
    eprintln!("Archiving...");

    let mut args: Vec<String> = Vec::new();

    // Build settings from extra_args (KEY=VALUE).
    for arg in opts.extra_args {
        if arg.contains('=') && !arg.starts_with('-') {
            args.push(arg.clone());
        }
    }

    if opts.skip_codesigning {
        args.extend([
            "CODE_SIGNING_ALLOWED=NO".into(),
            "CODE_SIGNING_REQUIRED=NO".into(),
            "CODE_SIGN_IDENTITY=\"\"".into(),
        ]);
    }

    args.extend([
        "-scheme".into(),
        opts.scheme.into(),
        "-configuration".into(),
        opts.configuration.into(),
        "-destination".into(),
        opts.archive_dest_raw.into(),
        "-archivePath".into(),
        archive_path.display().to_string(),
    ]);

    if let Some(dd) = opts.derived_data {
        args.extend(["-derivedDataPath".into(), dd.into()]);
    }
    if opts.ws.ws_type == WorkspaceType::Xcode {
        args.extend(["-workspace".into(), opts.ws.path.display().to_string()]);
    }

    args.push("archive".into());

    // Non-build-setting extra args (flags).
    for arg in opts.extra_args {
        if (!arg.contains('=') || arg.starts_with('-')) && arg != "archive" {
            args.push(arg.clone());
        }
    }

    run_xcodebuild(&args, opts.ws, opts.extra_env, opts.xcbeautify)?;

    // 3. Generate ExportOptions.plist.
    let plist_file = generate_export_options_plist(signed, &team_id)?;

    // 4. Export.
    eprintln!("Exporting IPA...");

    let export_args = vec![
        "-exportArchive".to_string(),
        "-archivePath".to_string(),
        archive_path.display().to_string(),
        "-exportPath".to_string(),
        opts.output_dir.display().to_string(),
        "-exportOptionsPlist".to_string(),
        plist_file.path().display().to_string(),
    ];

    run_xcodebuild(&export_args, opts.ws, opts.extra_env, opts.xcbeautify)?;

    // 5. Find the exported IPA.
    let ipa_path = opts.output_dir.join(format!("{product_name}.ipa"));
    if !ipa_path.exists() {
        // Fallback: look for any .ipa in the output directory.
        let fallback = std::fs::read_dir(opts.output_dir)?
            .filter_map(|e| e.ok())
            .find(|e| e.path().extension().is_some_and(|ext| ext == "ipa"))
            .map(|e| e.path());
        match fallback {
            Some(path) => {
                eprintln!("IPA exported:  {}", path.display());
                return Ok(path);
            }
            None => bail!(
                "IPA not found in {} after export",
                opts.output_dir.display()
            ),
        }
    }

    eprintln!("IPA exported:  {}", ipa_path.display());
    Ok(ipa_path)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn setting_str(entry: &BuildSettingsEntry, key: &str) -> Option<String> {
    entry
        .build_settings
        .get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// Run an xcodebuild command, optionally piping through xcbeautify.
fn run_xcodebuild(
    args: &[String],
    ws: &Workspace,
    extra_env: &[(String, String)],
    xcbeautify: Option<bool>,
) -> Result<()> {
    let use_beautify = xcbeautify.unwrap_or_else(|| {
        Command::new("which")
            .arg("xcbeautify")
            .output()
            .is_ok_and(|o| o.status.success())
    });

    if use_beautify {
        run_xcodebuild_piped(args, ws, extra_env)
    } else {
        run_xcodebuild_plain(args, ws, extra_env)
    }
}

fn run_xcodebuild_plain(
    args: &[String],
    ws: &Workspace,
    extra_env: &[(String, String)],
) -> Result<()> {
    let mut cmd = Command::new("xcodebuild");
    cmd.args(args);
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    if ws.ws_type == WorkspaceType::Spm {
        cmd.current_dir(ws.working_dir());
    }
    crate::util::run_cmd_inherit(&mut cmd).context("xcodebuild failed")
}

fn run_xcodebuild_piped(
    args: &[String],
    ws: &Workspace,
    extra_env: &[(String, String)],
) -> Result<()> {
    use std::process::Stdio;

    let mut cmd = Command::new("xcodebuild");
    cmd.args(args).stdout(Stdio::piped());
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    if ws.ws_type == WorkspaceType::Spm {
        cmd.current_dir(ws.working_dir());
    }
    crate::util::print_cmd(&cmd);

    let mut child = cmd.spawn().context("failed to spawn xcodebuild")?;
    let stdout = child.stdout.take().unwrap();

    let beautify = Command::new("xcbeautify")
        .stdin(stdout)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .context("failed to spawn xcbeautify")?;

    let status = child.wait()?;
    let _ = beautify.wait_with_output();

    if !status.success() {
        bail!("xcodebuild failed ({status})");
    }
    Ok(())
}

/// Generate an ExportOptions.plist for `xcodebuild -exportArchive`.
fn generate_export_options_plist(signed: bool, team_id: &str) -> Result<NamedTempFile> {
    let mut file = NamedTempFile::new()?;
    if signed {
        write!(
            file,
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>method</key>
    <string>debugging</string>
    <key>teamID</key>
    <string>{team_id}</string>
    <key>signingStyle</key>
    <string>automatic</string>
</dict>
</plist>
"#
        )?;
    } else {
        write!(
            file,
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>method</key>
    <string>debugging</string>
    <key>signingStyle</key>
    <string>manual</string>
    <key>signingCertificate</key>
    <string>-</string>
    <key>provisioningProfiles</key>
    <dict/>
</dict>
</plist>
"#
        )?;
    }
    Ok(file)
}
