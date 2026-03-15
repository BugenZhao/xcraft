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

# Initialize BSP / SourceKit-LSP integration for an Xcode or Tuist project
xcraft bsp init

# Refresh compile metadata from the latest Xcode build log
xcraft bsp sync
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
# Create .bsp/xcraft.json and .xcraft/bsp.toml
xcraft bsp init --workspace MyApp.xcworkspace --scheme MyApp

# Or initialize from a Tuist project
xcraft bsp init --workspace Project.swift --scheme MyApp

# Refresh compile metadata after building in Xcode
xcraft bsp sync
```

`xcraft build` and `xcraft launch` automatically refresh `.xcraft/bsp/compile-db.json` after a
successful build when BSP has already been initialized.

## Acknowledgments

Inspired by [SweetPad](https://github.com/sweetpad-dev/sweetpad), a VSCode extension for Xcode development.

## License

[MIT](LICENSE)
