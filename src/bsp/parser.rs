use std::path::{Path, PathBuf};

use anyhow::Result;

use super::compile_db::{CompileDb, SwiftCompileUnit, normalize_path};

/// Convert extracted Xcode log lines into the persisted Swift compile database.
pub fn parse_compile_db(lines: &[String]) -> Result<CompileDb> {
    let mut units = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = &lines[i];
        // Keep the parser intentionally narrow: v1 only cares about Swift driver /
        // Swift compile sections, not the rest of the build log noise.
        if !line.starts_with("CompileSwiftSources ")
            && !line.starts_with("SwiftDriver ")
            && !line.starts_with("SwiftDriver\\ Compilation ")
        {
            i += 1;
            continue;
        }

        let start = i;
        while i < lines.len() && !lines[i].is_empty() {
            i += 1;
        }
        if let Some(unit) = parse_swift_section(&lines[start..i]) {
            units.push(unit);
        }
        i += 1;
    }

    Ok(CompileDb::new(units))
}

/// Parse a single Swift build section, preferring the full driver command when available.
fn parse_swift_section(lines: &[String]) -> Option<SwiftCompileUnit> {
    if lines.is_empty() {
        return None;
    }

    let header = &lines[0];
    let mut command = lines.last()?.trim().to_string();
    if header.starts_with("SwiftDriver ") || header.starts_with("SwiftDriver\\ Compilation ") {
        // SwiftDriver sections wrap the real `swiftc` invocation in a builtin launcher.
        if !(command.starts_with("builtin-Swift-Compilation -- ")
            || command.starts_with("builtin-SwiftDriver -- "))
        {
            return None;
        }
        command = command.split_once(" -- ")?.1.to_string();
    } else if !header.starts_with("CompileSwiftSources ") {
        return None;
    }

    let directory = lines
        .iter()
        // Xcode prefixes nested log lines with indentation; trim before pattern matching.
        .find_map(|line| line.trim().strip_prefix("cd "))
        .and_then(|raw| shlex::split(raw).and_then(|parts| parts.first().cloned()))
        .unwrap_or_else(|| {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .display()
                .to_string()
        });

    let (files, file_lists) = extract_files(&command, Path::new(&directory));
    Some(SwiftCompileUnit {
        directory,
        command,
        files,
        file_lists,
    })
}

/// Collect Swift sources and `@...SwiftFileList` references from the compiler command.
fn extract_files(command: &str, base: &Path) -> (Vec<String>, Vec<String>) {
    let mut files = Vec::new();
    let mut file_lists = Vec::new();
    let args = shlex::split(command).unwrap_or_default();
    for arg in args {
        // Driver commands commonly use `@...SwiftFileList`; preserve the referenced file
        // instead of trying to inline it here.
        if let Some(path) = arg.strip_prefix('@')
            && path.ends_with(".SwiftFileList")
        {
            file_lists.push(
                normalize_path(Path::new(path), Some(base))
                    .display()
                    .to_string(),
            );
            continue;
        }
        if arg.ends_with(".swift") {
            // Some compile commands still list sources inline, especially in smaller targets.
            files.push(
                normalize_path(Path::new(&arg), Some(base))
                    .display()
                    .to_string(),
            );
            continue;
        }
        if arg.ends_with(".SwiftFileList") {
            file_lists.push(
                normalize_path(Path::new(&arg), Some(base))
                    .display()
                    .to_string(),
            );
            continue;
        }
    }
    (files, file_lists)
}

#[cfg(test)]
mod tests {
    use super::parse_compile_db;

    #[test]
    fn parses_swift_driver_sections() {
        let lines = vec![
            "SwiftDriver\\ Compilation App normal arm64 com.apple.xcode.tools.swift.compiler".to_string(),
            "cd /tmp/project".to_string(),
            "builtin-Swift-Compilation -- /usr/bin/swiftc -module-name App /tmp/project/Sources/App.swift".to_string(),
            "".to_string(),
        ];

        let db = parse_compile_db(&lines).unwrap();
        assert_eq!(db.swift_units.len(), 1);
        assert!(
            db.swift_units[0]
                .command
                .contains("swiftc -module-name App")
        );
        assert_eq!(db.swift_units[0].files.len(), 1);
    }
}
