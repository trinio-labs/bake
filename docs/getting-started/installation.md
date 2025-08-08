# Installation

Bake can be installed through several methods depending on your system and preferences.

## Homebrew (macOS/Linux)

The easiest way to install Bake on macOS and Linux:

```bash
brew install trinio-labs/tap/bake
```

## Cargo (Rust)

If you have Rust installed, you can install Bake from crates.io:

```bash
cargo install bake-cli
```

## Pre-built Binaries

Download pre-built binaries from the [GitHub Releases](https://github.com/trinio-labs/bake/releases) page.

### Linux/macOS

```bash
# Download and install (replace with latest version)
curl -L https://github.com/trinio-labs/bake/releases/latest/download/bake-x86_64-unknown-linux-gnu.tar.gz | tar xz
sudo mv bake /usr/local/bin/
```

### Windows

Download the Windows executable from the releases page and add it to your PATH.

## Development Build

To install from source:

```bash
git clone https://github.com/trinio-labs/bake.git
cd bake
cargo install --path .
```

## Verification

Verify your installation:

```bash
bake --version
```

## Auto-Updates

Bake includes auto-update functionality. See the [Auto-Update Guide](../reference/auto-update.md) for configuration options.

## Next Steps

- [Quick Start Guide](quick-start.md) - Get started with your first Bake project
- [First Project Tutorial](first-project.md) - Detailed walkthrough of creating a project