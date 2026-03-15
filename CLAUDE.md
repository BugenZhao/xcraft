# xcraft

CLI for building and running Xcode projects (`.xcworkspace`, SPM `Package.swift`, and Tuist `Project.swift`) from the terminal.

## Tech Stack

- Rust (edition 2024), binary name `xcraft`
- Main dependencies
  - clap (CLI parsing)
  - dialoguer (interactive prompts)
  - serde/serde_json (JSON parsing)
  - toml (TOML serialization for cache)
  - plist + flate2 (xcactivitylog / LogStoreManifest parsing)
  - shlex (Swift command parsing)
  - url (file URI conversion for BSP)
  - walkdir (file discovery)
  - anyhow (error handling)
  - md5 (stable BSP index database path hashing)
  - tempfile

## Project Structure

- `src/main.rs` — CLI entry point, clap subcommands (`workspaces`, `schemes`, `configs`, `destinations`, `configure`, `reset`, `bsp`, `build`, `clean`, `launch`)
- `src/cmd/` — subcommand implementations
  - `build.rs` — shared `ResolveArgs` / `BuildArgs`, `resolve_and_cache` (resolve + save to cache), `resolve_and_build` (resolve + build), `build` subcommand
  - `bsp.rs` — `bsp init`, `bsp sync`, `bsp serve`
  - `clean.rs` — `clean` subcommand (`CleanArgs` reuses `ResolveArgs`, calls `xcodebuild clean`)
  - `configure.rs` — `configure` subcommand (interactive re-prompt with cached defaults, no build)
  - `reset.rs` — `reset` subcommand (clear cached selections)
  - `launch.rs` — `launch` subcommand (flattens `BuildArgs`, adds launch-specific options)
  - `workspaces.rs`, `schemes.rs`, `configs.rs`, `destinations.rs` — listing subcommands
- `src/bsp/` — BSP / SourceKit-LSP integration
  - `config.rs` — `[bsp]` state under `.xcraft/state[.profile].toml`, build root derivation, path layout
  - `connection.rs` — `buildServer.json` writer
  - `xcactivitylog.rs` — LogStoreManifest lookup + `.xcactivitylog` string extraction
  - `parser.rs` — Swift compile command extraction from Xcode build log sections
  - `compile_db.rs` — persistent Swift compile database + query/filter logic
  - `server.rs` — minimal BSP stdio server for `sourcekit-lsp`
- `src/workspace.rs` — workspace detection and resolution (depth-4 scan for `.xcworkspace` / `Package.swift` / `Project.swift`); Tuist support via `ensure_generated()` which runs `tuist generate --no-open` and returns the generated `.xcworkspace`
- `src/scheme.rs` — scheme and configuration listing/resolution (SPM via `swift package dump-package`, Xcode via `xcodebuild -list`)
- `src/destination.rs` — destination listing (simulators via `simctl`, physical devices via `devicectl`, macOS)
- `src/build.rs` — `xcodebuild build` / `xcodebuild clean` execution + build settings extraction + optional xcbeautify pipe
- `src/launch.rs` — app launch by destination type (macOS direct exec, simulator simctl install/launch, device devicectl install/launch)
- `src/cache.rs` — persistent cache (`CachedState`) for last-used workspace/scheme/configuration/destination, stored in `.xcraft/state[.profile].toml`; BSP-specific state lives in the same file under `[bsp]`, and compile metadata is stored in `.xcraft/bsp/compile-db[.profile].json`
- `src/util.rs` — command execution helpers + fault-tolerant JSON parsing (handles non-JSON prefixes in xcodebuild output)
- `docs/build-run-launch-flow.md` — detailed flow documentation (Chinese)

## Build

```sh
cargo build
cargo run -- launch
```

## Development Workflow

After finishing a task:

- Run `cargo fmt` and `cargo clippy` to ensure formatting and lint compliance
- Optinally test with real projects like `~/Developer/MNGA`.
- Update `CLAUDE.md` to reflect changes on project structure
