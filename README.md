# xcraft

CLI for building and running Xcode projects from the terminal, aiming to simplify agentic development on Apple platforms. Supports `.xcworkspace`, SPM `Package.swift`, and Tuist `Project.swift`.

## Features

- Auto-detect `.xcworkspace`, `Package.swift`, and Tuist `Project.swift` projects
- Tuist integration — automatically runs `tuist generate` before building
- Interactive selection of workspace, scheme, configuration, and destination
- Cached selections for repeat builds — configure once, run many times
- Named profiles (`--profile`) — maintain multiple configurations side by side
- Build, clean, and launch in one command
- Launch on simulators, physical devices, and macOS
- Pipe build output through [xcbeautify](https://github.com/cpisciotta/xcbeautify) when available
- Generate `.bsp/xcraft.json` and serve SourceKit-LSP/BSP metadata for Xcode and Tuist projects
- Designed for headless / CI / agent-driven workflows

## Install

```sh
cargo install xcraft
```

Or from the Git repository:

```sh
cargo install --git https://github.com/BugenZhao/xcraft
```

## Usage

```sh
# Show available commands
xcraft help

# Build and run (interactively selects workspace, scheme, destination on first use)
xcraft launch

# Build without launching
xcraft build

# Clean build products
xcraft clean

# Other commands...

# Interactively re-select workspace, scheme, configuration, and destination
xcraft configure

# List workspaces / schemes / configurations / destinations
xcraft workspaces
xcraft schemes
xcraft configs
xcraft destinations

# Clear cached selections
xcraft reset
```

All resolve options (workspace, scheme, configuration, destination) are cached in `.xcraft/state.toml` so you only need to select them once. Use `xcraft configure` to re-select, or `xcraft reset` to clear.

### Profiles

Use `--profile <name>` to maintain multiple configurations side by side. Each profile stores its selections in a separate file (`.xcraft/state.<name>.toml`).

```sh
# Set up a simulator profile
xcraft configure --profile sim --destination "simulator:..."

# Set up a device profile
xcraft configure --profile device --destination "device:..."

# Build with a specific profile
xcraft launch --profile sim
xcraft launch --profile device

# Clear a specific profile
xcraft reset --profile sim
```

Without `--profile`, the default `.xcraft/state.toml` is used as before.

### BSP / SourceKit-LSP

For Xcode and Tuist projects, `xcraft` can generate a standard BSP connection file and serve
Swift compile metadata to `sourcekit-lsp`.

```sh
# Reuse the current cached workspace and scheme, then do an initial best-effort sync
xcraft bsp init

# Use a named profile for BSP state and compile metadata
xcraft bsp init --profile sim
xcraft bsp sync --profile sim

# Force an initial build so compile metadata is guaranteed to exist
xcraft bsp init --build

# Override the cached workspace or scheme when needed
xcraft bsp init --workspace MyApp.xcworkspace --scheme MyApp
xcraft bsp init --workspace Project.swift --scheme MyApp

# Manually refresh compile metadata from the latest Xcode build log
xcraft bsp sync
```

`xcraft bsp init` reuses the selected profile's cached `workspace` and `scheme`, writes
`.bsp/xcraft.json`, stores BSP-specific state under `[bsp]` in `.xcraft/state[.profile].toml`,
and then attempts an initial `bsp sync`. That sync is best-effort: if no usable Xcode activity
log exists yet, initialization still succeeds and a later `xcraft build`, `xcraft launch`, or
manual `xcraft bsp sync` will populate the compile database.

Use `xcraft bsp init --build` when you want initialization to immediately run a real build for
the selected profile. That build reuses the cached configuration and destination, and the normal
post-build BSP hook will refresh the matching compile database.

`xcraft build` and `xcraft launch` automatically refresh the matching compile database after a
successful build when BSP has already been initialized for that profile. The active `.bsp/xcraft.json`
always points at one profile at a time via `xcraft bsp serve [--profile ...]`.

## Acknowledgments

Inspired by [SweetPad](https://github.com/sweetpad-dev/sweetpad), a VSCode extension for Xcode development.

## License

[MIT](LICENSE)
