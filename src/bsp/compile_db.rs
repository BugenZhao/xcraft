use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// One Swift compiler invocation plus the files it covers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwiftCompileUnit {
    pub directory: String,
    pub command: String,
    #[serde(default)]
    pub files: Vec<String>,
    #[serde(default)]
    pub file_lists: Vec<String>,
}

/// On-disk compile metadata consumed by the BSP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompileDb {
    pub version: u32,
    #[serde(default)]
    pub swift_units: Vec<SwiftCompileUnit>,
}

impl CompileDb {
    pub fn new(swift_units: Vec<SwiftCompileUnit>) -> Self {
        Self {
            version: 1,
            swift_units,
        }
    }

    pub fn load_json(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        serde_json::from_str(&content)
            .with_context(|| format!("failed to parse {}", path.display()))
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, serde_json::to_vec_pretty(self)?)
            .with_context(|| format!("failed to write {}", path.display()))
    }

    /// Look up compiler arguments for a source file by expanding any Swift file lists.
    pub fn query(&self, file: &Path) -> Option<(Vec<String>, String)> {
        let map = self.file_index();
        let key = normalize_for_lookup(file, None);
        let entry = map.get(&key)?;
        let flags = command_to_arguments(entry.command);
        let workdir = flags
            .windows(2)
            .find_map(|window| (window[0] == "-working-directory").then(|| window[1].clone()))
            .unwrap_or_else(|| entry.directory.to_string());
        Some((flags, workdir))
    }

    fn file_index(&self) -> HashMap<String, FileCommand<'_>> {
        let mut files = HashMap::new();
        for unit in &self.swift_units {
            // Xcode may log files inline or via `.SwiftFileList`; index both into the
            // same lookup table so the server can answer per-file queries cheaply.
            for file in &unit.files {
                files.insert(
                    normalize_for_lookup(Path::new(file), None),
                    FileCommand {
                        command: &unit.command,
                        directory: &unit.directory,
                    },
                );
            }
            for list_path in &unit.file_lists {
                let list_path =
                    normalize_path(Path::new(list_path), Some(Path::new(&unit.directory)));
                if let Ok(entries) = read_args_file(&list_path) {
                    for file in entries {
                        let file_path =
                            normalize_path(Path::new(&file), Some(Path::new(&unit.directory)));
                        files.insert(
                            normalize_for_lookup(&file_path, None),
                            FileCommand {
                                command: &unit.command,
                                directory: &unit.directory,
                            },
                        );
                    }
                }
            }
        }
        files
    }
}

struct FileCommand<'a> {
    command: &'a str,
    directory: &'a str,
}

/// Convert a logged compiler command into sourcekit-lsp-friendly arguments.
pub fn command_to_arguments(command: &str) -> Vec<String> {
    let mut args = shlex::split(command).unwrap_or_default();
    if !args.is_empty() {
        args.remove(0);
    }
    filter_arguments(args)
}

fn filter_arguments(args: Vec<String>) -> Vec<String> {
    let mut filtered = Vec::new();
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        if arg == "-emit-localized-strings-path" {
            let _ = iter.next();
            continue;
        }
        // These flags are useful for a real build but break sourcekit-lsp option parsing.
        if arg == "-use-frontend-parseable-output" || arg == "-emit-localized-strings" {
            continue;
        }
        if arg == "-filelist" {
            if let Some(path) = iter.next() {
                // sourcekit-lsp expects the concrete source file arguments, not the
                // indirection used by xcodebuild.
                filtered.extend(read_args_file(Path::new(&path)).unwrap_or_default());
            }
            continue;
        }
        if let Some(path) = arg.strip_prefix('@') {
            filtered.extend(read_args_file(Path::new(path)).unwrap_or_default());
            continue;
        }
        filtered.push(arg);
    }
    filtered
}

fn read_args_file(path: &Path) -> Result<Vec<String>> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read args file {}", path.display()))?;
    Ok(shlex::split(&content).unwrap_or_else(|| {
        // `.SwiftFileList` files are one-path-per-line, while some argument files are
        // shell-split strings; support both shapes.
        content
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(ToOwned::to_owned)
            .collect()
    }))
}

/// Normalize paths eagerly so the same source file can be found across symlinks and cwd changes.
pub fn normalize_path(path: &Path, base: Option<&Path>) -> PathBuf {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else if let Some(base) = base {
        base.join(path)
    } else {
        path.to_path_buf()
    };
    fs::canonicalize(&path).unwrap_or(path)
}

fn normalize_for_lookup(path: &Path, base: Option<&Path>) -> String {
    // Lowercase keys to match the historical behavior of xcode-build-server and
    // avoid surprises on case-insensitive filesystems.
    normalize_path(path, base)
        .display()
        .to_string()
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::{CompileDb, SwiftCompileUnit};

    #[test]
    fn query_returns_filtered_swift_arguments() {
        let temp = tempdir().unwrap();
        let src = temp.path().join("Sources").join("App.swift");
        fs::create_dir_all(src.parent().unwrap()).unwrap();
        fs::write(&src, "print(\"hi\")").unwrap();

        let db = CompileDb::new(vec![SwiftCompileUnit {
            directory: temp.path().display().to_string(),
            command: format!(
                "/usr/bin/swiftc -module-name App -emit-localized-strings-path /tmp/foo -working-directory {} {}",
                temp.path().display(),
                src.display()
            ),
            files: vec![src.display().to_string()],
            file_lists: vec![],
        }]);

        let (args, workdir) = db.query(&src).unwrap();
        assert!(!args.contains(&"-emit-localized-strings-path".to_string()));
        assert_eq!(workdir, temp.path().display().to_string());
    }
}
