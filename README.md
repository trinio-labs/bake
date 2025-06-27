# 🍪 bake 🍪

Yet another task runner. This time it's tasty.

Bake is a task runner built to be simpler than Make and to satisfy some of the needs of managing dependent build, test
and deploy tasks in complex projects. It's capable of running tasks in parallel as well as caching outputs based on each
recipe's inputs such as files or environment variables.

## Features

- **🔄 Parallel Execution**: Run multiple tasks simultaneously with dependency resolution
- **💾 Smart Caching**: Local and remote caching (S3, GCS) with content-based invalidation
- **📝 Template Variables**: Comprehensive variable system with environment, user, and constant variables
- **🏗️ Project Organization**: Organize tasks into cookbooks and recipes
- **🔄 Self-Updates**: Automatic update checking and installation
- **📊 Version Management**: Track project compatibility with bake versions
- **⚡ Fast & Reliable**: Built in Rust for performance and reliability

## Installation

### Homebrew

```sh
brew install trinio-labs/tap/bake
```

### Cargo

```sh
cargo install bake-cli
```

## Quick Start

1. **Create a project configuration** (`bake.yml`):

```yaml
cookbooks:
  - app

variables:
  environment: development
  version: "1.0.0"

config:
  max_parallel: 4
  cache:
    local:
      enabled: true
```

2. **Create a cookbook** (`app/cookbook.yml`):

```yaml
name: app

variables:
  build_env: "{{var.environment}}"
  output_dir: "dist-{{var.build_env}}"

recipes:
  build:
    description: "Build the application"
    inputs:
      - "src/**/*"
      - "package.json"
    outputs:
      - "{{var.output_dir}}/**/*"
    run: |
      echo "Building for {{var.build_env}}..."
      npm install
      npm run build
    dependencies:
      - test
  
  test:
    description: "Run tests"
    inputs:
      - "src/**/*"
      - "test/**/*"
    run: |
      npm test
```

3. **Run your tasks**:

```bash
# Run all recipes
bake

# Run specific recipe
bake app:build

# Run with variable override
bake app:build --var environment=production
```

## Auto-Updates

Bake includes an auto-update feature that keeps your installation up to date automatically. The tool checks for updates periodically based on your configuration and stores the last check time to avoid excessive API calls.

You can configure update behavior in your `bake.yml` file or use CLI commands:

```bash
# Check for updates (bypasses interval)
bake --check-updates

# Perform self-update
bake --self-update

# Update to latest version (including prereleases)
bake --self-update --prerelease
```

For detailed configuration options, see [Auto-Update Documentation](./docs/auto-update.md).

## Variable System

Bake provides a powerful variable system that supports:

- **Environment Variables**: `{{env.NODE_ENV}}`
- **User Variables**: `{{var.version}}`
- **Built-in Constants**: `{{project.root}}`, `{{cookbook.root}}`
- **Variable References**: `{{var.base_url}}/v{{var.version}}`

Variables are scoped hierarchically: project → cookbook → recipe → command line overrides.

## Version Management

Bake tracks the version used to create project configurations:

```yaml
bake_version: "0.6.0"
```

This helps detect compatibility issues when using different bake versions:

```bash
# Update project to current bake version
bake --update-version

# Force run with version mismatch
bake --force-version-override
```

## A bake project

A bake project consists of a root `bake.yml` configuration file, [Cookbooks](#cookbooks) and [Recipes](#recipes).
A Cookbook is a collection of Recipes that share some context while each Recipe is a distinct task that can be run
and cached by `bake`.

A typical project looks like this:

```sh

├── foo
│   ├── src
│   │   └── main.rs
│   ├── cargo.toml
│   └── cookbook.yml
├── bar
│   ├── src
│   │   └── index.js
│   ├── package.json
│   └── cookbook.yml
└── bake.yml
```

Bake is able to quickly scan a directory for `cookbook.yml` files to find cookbooks in the project. It then builds a
dependency graph for all recipes and runs them accordingly.

### Cookbooks

Cookbooks contain recipes that usually share the same context. Typically, a cookbook is a package of a monorepo but it
is not restricted to that logical separation.

A cookbook can be configured by a `cookbook.yml` file such as the example below:

```yml
name: foo
variables:
  build_type: release
  output_dir: "dist-{{var.build_type}}"

recipes:
  build:
    inputs:
      - "./src/**/*.rs"
    outputs:
      - "./target/foo"
    run: |
      echo "Building foo for {{var.build_type}}"
      ./build.sh
    dependencies:
      - "test"
      - "bar:build"
  test:
    run: |
      cargo test
    inputs:
      - "./src/**/*.rs"
    outputs:
      - lcov.info
```

A cookbook can contain any number of recipes and in the future will be able to hold common recipe configurations.

### Recipes

As seen above, every recipe, at a minimum, must have a `run` property that defines how to bake it. It can also state which
recipes it depends on by using the recipe's full name or partial if they both belong to the same cookbook. A recipe can also
specify which files should be considered for caching in the property `inputs`. Inputs are configured as glob patterns
relative to the root of the cookbook.

For a more detailed explanation of the configuration files, please see [Configuration](./docs/configuration.md#recipes).

## Baking recipes

By default, bake will run all recipes in all cookbooks if called without any arguments.

If you want to be more granular, you can run `bake` passing a pattern to filter the recipes to run. The pattern is always
in the form `<cookbook>:<recipe>`.

For example, to run the `build` recipe from the `foo` cookbook, run:

```sh
bake foo:build
```

You can also run all recipes in the `foo` cookbook:

```sh
bake foo:
```

Or all recipes named `build` in any cookbook:

```sh
bake :build
```

## Caching

By default, bake caches runs locally in a directory called `.bake/cache`. Bake will use the combined hash of all inputs of
a recipe, the hash of its dependencies and its run command to create a cache key. This allows for recipes to be cached
and only run again if either a dependency or the recipe itself changes. Bake can also be configured to use a remote storage
to cache recipes such as S3 or GCS.

For more information on how to configure caching, please see [Caching](./docs/configuration.md#caching).

## Documentation

- [Configuration Guide](./docs/configuration.md) - Complete configuration reference
- [Auto-Update Documentation](./docs/auto-update.md) - Update system configuration
