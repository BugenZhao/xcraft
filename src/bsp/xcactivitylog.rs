use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use plist::Value;

/// Return candidate activity logs ordered from most to least promising.
/// Prefer manifest order first, then fall back to recent non-empty files on disk.
pub fn candidate_log_paths(build_root: &Path, scheme: Option<&str>) -> Result<Vec<PathBuf>> {
    let manifest_path = build_root
        .join("Logs")
        .join("Build")
        .join("LogStoreManifest.plist");
    let log_dir = manifest_path
        .parent()
        .context("manifest has no parent directory")?;

    let mut candidates = Vec::new();
    if manifest_path.exists() {
        let manifest = Value::from_file(&manifest_path)
            .with_context(|| format!("failed to read {}", manifest_path.display()))?;
        let logs = manifest
            .as_dictionary()
            .and_then(|dict| dict.get("logs"))
            .and_then(Value::as_dictionary)
            .context("LogStoreManifest.plist missing logs dictionary")?;

        let mut manifest_candidates = Vec::new();
        for value in logs.values() {
            let Some(log) = value.as_dictionary() else {
                continue;
            };
            if let Some(scheme) = scheme
                && log
                    .get("schemeIdentifier-schemeName")
                    .and_then(Value::as_string)
                    != Some(scheme)
            {
                continue;
            }
            let Some(file_name) = log.get("fileName").and_then(Value::as_string) else {
                continue;
            };
            manifest_candidates.push((
                numeric_sort_key(log.get("timeStoppedRecording")),
                log_dir.join(file_name),
            ));
        }
        // Use the manifest's logical ordering first. This is more stable than filesystem mtime,
        // especially when Xcode leaves behind zero-byte or truncated log files.
        manifest_candidates
            .sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        candidates.extend(manifest_candidates.into_iter().map(|(_, path)| path));
    }

    let mut fs_candidates = fs::read_dir(log_dir)?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "xcactivitylog"))
        .filter_map(|path| {
            let metadata = fs::metadata(&path).ok()?;
            if metadata.len() == 0 {
                return None;
            }
            Some((metadata.modified().ok()?, path))
        })
        .collect::<Vec<_>>();
    fs_candidates.sort_by(|a, b| b.0.cmp(&a.0));
    candidates.extend(fs_candidates.into_iter().map(|(_, path)| path));

    let mut unique = Vec::new();
    for path in candidates {
        // The same file can appear in both manifest-ordered and filesystem-ordered passes.
        if !unique.contains(&path) {
            unique.push(path);
        }
    }
    if unique.is_empty() {
        bail!("no matching xcactivitylog found");
    }
    Ok(unique)
}

/// Extract the log lines relevant to Swift compilation from an `.xcactivitylog` file.
pub fn extract_compile_lines(path: &Path) -> Result<Vec<String>> {
    let data = read_decompressed(path)?;
    if data.len() < 4 || &data[..4] != b"SLF0" {
        bail!("{} is not a valid xcactivitylog", path.display());
    }

    let mut lines = Vec::new();
    for string in string_tokens(&data[4..])? {
        if !string.starts_with("CompileSwiftSources ")
            && !string.starts_with("SwiftDriver ")
            && !string.starts_with("SwiftDriver\\ Compilation ")
        {
            continue;
        }
        // Xcode often stores nested log lines separated by bare `\r`, not `\n`.
        for line in string.split(['\r', '\n']) {
            if !line.is_empty() {
                lines.push(line.to_string());
            }
        }
        lines.push(String::new());
    }
    Ok(lines)
}

// Xcode logs may be truncated while still containing valid prefixes, so use `gunzip`
// and keep whatever bytes it managed to emit.
fn read_decompressed(path: &Path) -> Result<Vec<u8>> {
    let output = Command::new("gunzip")
        .args(["--stdout"])
        .arg(path)
        .output()
        .with_context(|| format!("failed to run gunzip for {}", path.display()))?;
    if output.stdout.is_empty() {
        bail!(
            "failed to decompress {}: {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(output.stdout)
}

/// Walk the binary activity log format and collect string payloads.
fn string_tokens(data: &[u8]) -> Result<Vec<String>> {
    let mut tokens = Vec::new();
    let mut pos = 0;
    while pos < data.len() {
        let Some(marker_index) = data[pos..].iter().position(|byte| {
            matches!(*byte, b'"' | b'-' | b'#' | b'^' | b'(' | b'%' | b'@' | b'*')
        }) else {
            break;
        };
        let marker_index = pos + marker_index;
        let prefix = &data[pos..marker_index];
        match data[marker_index] {
            b'"' | b'%' | b'*' => {
                let Ok(length) = parse_number(prefix) else {
                    // Truncated logs can leave an incomplete length prefix at the tail.
                    break;
                };
                let start = marker_index + 1;
                let end = start + length;
                if end > data.len() {
                    // Best effort: truncated logs are common enough that partial extraction is better.
                    break;
                }
                if data[marker_index] == b'"' {
                    tokens.push(String::from_utf8_lossy(&data[start..end]).into_owned());
                }
                pos = end;
            }
            b'-' | b'#' | b'^' | b'(' | b'@' => {
                pos = marker_index + 1;
            }
            _ => unreachable!(),
        }
    }
    Ok(tokens)
}

fn parse_number(prefix: &[u8]) -> Result<usize> {
    let raw = std::str::from_utf8(prefix)?.trim();
    if raw.is_empty() {
        bail!("empty numeric token in xcactivitylog");
    }
    Ok(raw.parse()?)
}

fn numeric_sort_key(value: Option<&Value>) -> f64 {
    match value {
        Some(Value::Real(v)) => *v,
        Some(Value::Integer(v)) => v.as_signed().unwrap_or_default() as f64,
        Some(Value::String(v)) => v.parse().unwrap_or(0.0),
        _ => 0.0,
    }
}
