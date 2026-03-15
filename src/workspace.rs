use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

/// The type of Xcode workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkspaceType {
    Xcode,
    Spm,
    Tuist,
}

/// A detected workspace path with its type.
#[derive(Debug, Clone)]
pub struct Workspace {
    pub path: PathBuf,
    pub ws_type: WorkspaceType,
}

impl Workspace {
    pub fn new(path: PathBuf) -> Self {
        let ws_type = detect_type(&path);
        Self { path, ws_type }
    }

    /// For SPM projects, returns the directory containing Package.swift.
    /// For Xcode projects, returns the parent directory of the .xcworkspace.
    pub fn working_dir(&self) -> &Path {
        match self.ws_type {
            WorkspaceType::Xcode => self.path.parent().unwrap_or(&self.path),
            WorkspaceType::Spm | WorkspaceType::Tuist => {
                if self.path.is_dir() {
                    &self.path
                } else {
                    self.path.parent().unwrap_or(&self.path)
                }
            }
        }
    }

    /// For Tuist workspaces, run `tuist generate` and return the generated Xcode workspace.
    /// For other types, return a clone of self.
    pub fn ensure_generated(&self) -> Result<Workspace> {
        match self.ws_type {
            WorkspaceType::Tuist => tuist_generate(self),
            _ => Ok(self.clone()),
        }
    }
}

impl std::fmt::Display for WorkspaceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WorkspaceType::Xcode => write!(f, "Xcode"),
            WorkspaceType::Spm => write!(f, "SPM"),
            WorkspaceType::Tuist => write!(f, "Tuist"),
        }
    }
}

impl std::fmt::Display for Workspace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.ws_type, self.path.display())
    }
}

fn detect_type(path: &Path) -> WorkspaceType {
    if path.is_dir() && path.join("Project.swift").exists()
        || path.file_name().is_some_and(|n| n == "Project.swift")
    {
        WorkspaceType::Tuist
    } else if path.is_dir() && path.join("Package.swift").exists()
        || path.file_name().is_some_and(|n| n == "Package.swift")
    {
        WorkspaceType::Spm
    } else {
        WorkspaceType::Xcode
    }
}

/// Detect all workspace candidates under `root` (depth <= 4).
pub fn detect_workspaces(root: &Path) -> Vec<Workspace> {
    let mut results = Vec::new();
    for entry in WalkDir::new(root)
        .max_depth(4)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n,
            None => continue,
        };
        if name == "Package.swift"
            || name == "Project.swift"
            || (name.ends_with(".xcworkspace") && !path.starts_with("."))
        {
            results.push(Workspace::new(path.to_path_buf()));
        }
    }
    results
}

/// Resolve workspace: use explicit path, or auto-detect from current dir.
/// When `default` is provided, it pre-selects the matching workspace in the prompt.
pub fn resolve_workspace(explicit: Option<&Path>, default: Option<&Path>) -> Result<Workspace> {
    if let Some(p) = explicit {
        return Ok(Workspace::new(p.to_path_buf()));
    }

    let cwd = std::env::current_dir()?;
    let candidates = detect_workspaces(&cwd);

    match candidates.len() {
        0 => bail!("no .xcworkspace, Package.swift, or Project.swift found (searched depth 4)"),
        1 => Ok(candidates.into_iter().next().unwrap()),
        _ => {
            let labels: Vec<String> = candidates.iter().map(|w| w.to_string()).collect();
            let default_idx = default
                .and_then(|d| candidates.iter().position(|w| w.path == d))
                .unwrap_or(0);
            let sel = dialoguer::Select::new()
                .with_prompt("Multiple workspaces found, select one")
                .items(&labels)
                .default(default_idx)
                .interact()?;
            Ok(candidates.into_iter().nth(sel).unwrap())
        }
    }
}

/// Run `tuist generate --no-open` and return the generated `.xcworkspace`.
fn tuist_generate(ws: &Workspace) -> Result<Workspace> {
    let dir = ws.working_dir();
    eprintln!("Running tuist generate...");
    crate::util::run_cmd_inherit(
        Command::new("tuist")
            .args(["generate", "--no-open", "--path"])
            .arg(dir),
    )?;

    // Find the generated .xcworkspace in the project directory.
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().is_some_and(|ext| ext == "xcworkspace") {
            return Ok(Workspace {
                path,
                ws_type: WorkspaceType::Xcode,
            });
        }
    }
    bail!(
        "no .xcworkspace found after tuist generate in {}",
        dir.display()
    )
}
