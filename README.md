# 🍪 bake 🍪

Yet another task runner. This time it's tasty.

Bake is a fast, reliable task runner built in Rust for managing dependent build, test, and deploy tasks in complex projects. It features intelligent caching, parallel execution, and a powerful template system.

## Key Features

- **🔄 Parallel Execution** - Run multiple tasks simultaneously with dependency resolution
- **💾 Smart Caching** - Local and remote caching (S3, GCS) with content-based invalidation  
- **📝 Template System** - Reusable recipe templates with typed parameters
- **🌳 Visual Planning** - Clean tree-style execution plan visualization
- **⚡ Built in Rust** - Fast, reliable, and memory-safe

## Installation

### Homebrew (macOS/Linux)

```bash
brew install trinio-labs/tap/bake
```

### Cargo

```bash
cargo install bake-cli
```

[More installation options →](docs/getting-started/installation.md)

## Quick Start

Create a `bake.yml` and `cookbook.yml`, then run your tasks:

```bash
# Run all recipes
bake

# Run specific recipe  
bake app:build

# Override variables
bake --var environment=production
```

[5-minute tutorial →](docs/getting-started/quick-start.md)

## Project Structure

A Bake project organizes tasks into **cookbooks** and **recipes**:

```
my-project/
├── bake.yml              # Project configuration
├── frontend/
│   ├── cookbook.yml      # Cookbook with recipes
│   └── src/
└── backend/
    ├── cookbook.yml
    └── src/
```

- **Projects** contain cookbooks and global settings
- **Cookbooks** group related recipes (typically per package/service)  
- **Recipes** are individual tasks that can run and cache

## Core Concepts

### Variables & Templates

Use variables throughout your configuration:

```yaml
# bake.yml
variables:
  environment: development
  version: "1.0.0"

# cookbook.yml  
recipes:
  build:
    run: |
      echo "Building v{{var.version}} for {{var.environment}}"
      npm run build:{{var.environment}}
```

### Smart Caching

Recipes cache automatically based on input files and dependencies:

```yaml
recipes:
  build:
    cache:
      inputs:
        - "src/**/*"
        - "package.json"
      outputs: 
        - "dist/**/*"
    run: npm run build
```

### Dependencies

Recipes can depend on other recipes:

```yaml
recipes:
  deploy:
    dependencies: [build, test]
    run: ./deploy.sh
```

## Documentation

### Getting Started
- [Installation](docs/getting-started/installation.md)
- [Quick Start](docs/getting-started/quick-start.md) 
- [First Project Tutorial](docs/getting-started/first-project.md)

### Guides
- [Configuration](docs/guides/configuration.md)
- [Variables](docs/guides/variables.md)
- [Caching](docs/guides/caching.md)
- [Recipe Templates](docs/guides/recipe-templates.md)
- [Best Practices](docs/guides/best-practices.md)
- [Troubleshooting](docs/guides/troubleshooting.md)

### Reference  
- [CLI Commands](docs/reference/cli-commands.md)
- [Configuration Schema](docs/reference/configuration-schema.md)
- [Auto-Update](docs/reference/auto-update.md)

## Community

- [Contributing](docs/development/contributing.md)
- [GitHub Issues](https://github.com/trinio-labs/bake/issues)
- [Changelog](CHANGELOG.md)

## License

[View License](LICENSE)
