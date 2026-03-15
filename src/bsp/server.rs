use std::collections::HashSet;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::{Value, json};
use url::Url;

use super::compile_db::CompileDb;
use super::config::{BspConfig, index_db_dir};

const TARGET_URI: &str = "dummy://xcraft";

/// Run the minimal BSP server used by sourcekit-lsp.
pub fn serve(profile: Option<&str>) -> Result<()> {
    let stdin = io::stdin();
    let mut reader = io::BufReader::new(stdin.lock());
    let stdout = io::stdout();
    let mut writer = stdout.lock();
    let mut state: Option<State> = None;

    loop {
        let Some(message) = read_message(&mut reader)? else {
            break;
        };
        let method = message
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();

        let response = match method.as_str() {
            "build/initialize" => Some(handle_initialize(&message, &mut state, profile)?),
            "build/initialized" => None,
            "workspace/buildTargets" => Some(handle_build_targets(&message)),
            "buildTarget/sources" => Some(handle_build_target_sources(&message, state.as_ref())?),
            "workspace/waitForBuildSystemUpdates" => Some(ok_response(&message, json!({}))),
            "textDocument/registerForChanges" => {
                Some(handle_register_for_changes(&message, state.as_mut())?)
            }
            "textDocument/sourceKitOptions" => {
                Some(handle_sourcekit_options(&message, state.as_mut())?)
            }
            "build/shutdown" => Some(ok_response(&message, Value::Null)),
            "exit" => break,
            _ => unknown_method_response(&message),
        };

        if let Some(response) = response {
            write_message(&mut writer, &response)?;
        }
    }

    Ok(())
}

/// Per-process server state. The server intentionally keeps this lightweight and
/// reloads compile metadata from disk when needed instead of trying to mirror the
/// build system in memory.
struct State {
    config: BspConfig,
    compile_db: Option<CompileDb>,
    compile_db_mtime: Option<std::time::SystemTime>,
    observed_uris: HashSet<String>,
}

impl State {
    fn new(_root: PathBuf, config: BspConfig) -> Result<Self> {
        let (compile_db, compile_db_mtime) = load_compile_db(Path::new(&config.compile_db_path))?;
        Ok(Self {
            config,
            compile_db,
            compile_db_mtime,
            observed_uris: HashSet::new(),
        })
    }

    fn maybe_reload_compile_db(&mut self) -> Result<()> {
        let path = Path::new(&self.config.compile_db_path);
        let current_mtime = fs::metadata(path).and_then(|m| m.modified()).ok();
        if current_mtime == self.compile_db_mtime {
            return Ok(());
        }
        // The server is intentionally stateless across requests: reload when the file changes.
        let (compile_db, compile_db_mtime) = load_compile_db(path)?;
        self.compile_db = compile_db;
        self.compile_db_mtime = compile_db_mtime;
        Ok(())
    }
}

fn handle_initialize(
    message: &Value,
    state: &mut Option<State>,
    profile: Option<&str>,
) -> Result<Value> {
    let root_uri = message
        .get("params")
        .and_then(|params| params.get("rootUri"))
        .and_then(Value::as_str)
        .context("missing rootUri in build/initialize")?;
    let root = uri_to_path(root_uri)?;
    let config = BspConfig::load(&root, profile)?;
    let index_store_path = config.index_store_path();
    let hash = md5::compute(index_store_path.display().to_string().as_bytes());
    // Match sourcekit-lsp's expectation that one index store maps to one persistent DB path.
    let index_database_path = index_db_dir(&root).join(format!("index-db-{hash:x}"));

    *state = Some(State::new(root.clone(), config)?);

    Ok(ok_response(
        message,
        json!({
            "displayName": "xcraft",
            "version": env!("CARGO_PKG_VERSION"),
            "bspVersion": BspConfig::bsp_version(),
            "rootUri": root_uri,
            "capabilities": {
                "languageIds": ["swift"]
            },
            "data": {
                "indexDatabasePath": index_database_path.display().to_string(),
                "indexStorePath": index_store_path.display().to_string(),
                "sourceKitOptionsProvider": true
            },
            "dataKind": "sourceKit"
        }),
    ))
}

fn handle_build_targets(message: &Value) -> Value {
    // v1 exposes a single synthetic target because sourcekit-lsp only needs a place
    // to hang source roots and sourceKitOptions requests from.
    ok_response(
        message,
        json!({
            "targets": [{
                "id": { "uri": TARGET_URI },
                "displayName": "xcraft",
                "capabilities": {},
                "languageIds": ["swift"],
                "dependencies": []
            }]
        }),
    )
}

fn handle_build_target_sources(message: &Value, state: Option<&State>) -> Result<Value> {
    let state = state.context("server not initialized")?;
    let targets = message
        .get("params")
        .and_then(|params| params.get("targets"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut items = Vec::new();
    for target in targets {
        if target.get("uri").and_then(Value::as_str) != Some(TARGET_URI) {
            continue;
        }
        let mut sources = vec![json!({
            "uri": path_to_directory_uri(state.config.effective_workspace())?,
            "kind": 2,
            "generated": false
        })];
        // Tuist sources live next to `Project.swift`, while the effective workspace is generated.
        if state.config.uses_generated_workspace() {
            sources.push(json!({
                "uri": path_to_directory_uri(tuist_input_source_dir(&state.config))?,
                "kind": 2,
                "generated": false
            }));
        }
        if let Some(source_packages_dir) = source_packages_checkouts_dir(&state.config) {
            sources.push(json!({
                "uri": path_to_directory_uri(&source_packages_dir)?,
                "kind": 2,
                "generated": false
            }));
        }
        items.push(json!({
            "target": target,
            "sources": sources
        }));
    }
    Ok(ok_response(message, json!({ "items": items })))
}

fn handle_register_for_changes(message: &Value, state: Option<&mut State>) -> Result<Value> {
    let state = state.context("server not initialized")?;
    let params = message
        .get("params")
        .and_then(Value::as_object)
        .context("missing params")?;
    let action = params
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let uri = params
        .get("uri")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    match action {
        "register" => {
            state.observed_uris.insert(uri);
        }
        "unregister" => {
            state.observed_uris.remove(&uri);
        }
        _ => {}
    }
    // v1 keeps the registration set only for protocol compatibility. It does not emit
    // `build/sourceKitOptionsChanged` notifications yet.
    Ok(ok_response(message, Value::Null))
}

fn handle_sourcekit_options(message: &Value, state: Option<&mut State>) -> Result<Value> {
    let state = state.context("server not initialized")?;
    state.maybe_reload_compile_db()?;
    let uri = message
        .get("params")
        .and_then(|params| params.get("textDocument"))
        .and_then(|doc| doc.get("uri"))
        .and_then(Value::as_str)
        .context("missing text document uri")?;
    let path = uri_to_path(uri)?;
    let result = state
        .compile_db
        .as_ref()
        .and_then(|db| db.query(&path))
        .map(|(flags, workdir)| {
            json!({
                "compilerArguments": flags,
                "workingDirectory": workdir
            })
        })
        .unwrap_or(Value::Null);
    Ok(ok_response(message, result))
}

fn ok_response(message: &Value, result: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": message.get("id").cloned().unwrap_or(Value::Null),
        "result": result
    })
}

fn unknown_method_response(message: &Value) -> Option<Value> {
    message.get("id").map(|id| {
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {
                "code": -32601,
                "message": format!("unhandled method {}", message.get("method").and_then(Value::as_str).unwrap_or_default())
            }
        })
    })
}

/// Read one JSON-RPC message framed with `Content-Length`.
fn read_message(reader: &mut impl BufRead) -> Result<Option<Value>> {
    let mut content_length = None;
    loop {
        let mut line = String::new();
        let read = reader.read_line(&mut line)?;
        if read == 0 {
            return Ok(None);
        }
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }
        if let Some(value) = line.strip_prefix("Content-Length:") {
            content_length = Some(value.trim().parse::<usize>()?);
        }
    }

    let length = content_length.context("missing Content-Length header")?;
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    Ok(Some(serde_json::from_slice(&body)?))
}

/// Write one JSON-RPC response framed with `Content-Length`.
fn write_message(writer: &mut impl Write, response: &Value) -> Result<()> {
    let body = serde_json::to_vec(response)?;
    write!(writer, "Content-Length: {}\r\n\r\n", body.len())?;
    writer.write_all(&body)?;
    writer.flush()?;
    Ok(())
}

fn load_compile_db(path: &Path) -> Result<(Option<CompileDb>, Option<std::time::SystemTime>)> {
    if !path.exists() {
        return Ok((None, None));
    }
    let mtime = fs::metadata(path).and_then(|m| m.modified()).ok();
    let db = CompileDb::load_json(path)?;
    Ok((Some(db), mtime))
}

fn uri_to_path(uri: &str) -> Result<PathBuf> {
    Url::parse(uri)
        .with_context(|| format!("invalid uri {uri}"))?
        .to_file_path()
        .map_err(|_| anyhow::anyhow!("unsupported file uri {uri}"))
}

fn path_to_directory_uri(path: &Path) -> Result<String> {
    Url::from_directory_path(path)
        .map_err(|_| anyhow::anyhow!("failed to convert {} to directory uri", path.display()))
        .map(|url| url.to_string())
}

fn tuist_input_source_dir(config: &BspConfig) -> &Path {
    // When the input was `Project.swift`, expose its parent directory as a source root.
    let input = Path::new(&config.workspace_input);
    if input.is_dir() {
        input
    } else {
        input.parent().unwrap_or(input)
    }
}

fn source_packages_checkouts_dir(config: &BspConfig) -> Option<PathBuf> {
    let path = Path::new(&config.build_root)
        .join("SourcePackages")
        .join("checkouts");
    path.is_dir().then_some(path)
}
