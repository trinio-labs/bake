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
- **🌳 Execution Planning**: Clean tree-style visualization of recipe execution order
- **🔍 Configuration Debugging**: Render resolved configuration with variable substitution
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
  minVersion: "0.8.1"
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

## Variable System

Bake provides a powerful variable system that supports:

- **Environment Variables**: `{{env.NODE_ENV}}`
- **User Variables**: `{{var.version}}`
- **Built-in Constants**: `{{project.root}}`, `{{cookbook.root}}`
- **Variable References**: `{{var.base_url}}/v{{var.version}}`

Variables are scoped hierarchically: project → cookbook → recipe → command line overrides.

## Recipe Templates

Bake supports reusable recipe templates to eliminate duplication and standardize common patterns. Templates are defined in `.bake/templates/` with typed parameters and can be instantiated across multiple cookbooks.

```yaml
# .bake/templates/build-template.yml
name: "Build Template"
description: "Standard build process with configurable parameters"

parameters:
  language:
    type: string
    required: true
    description: "Programming language (node, rust, go)"

  build_command:
    type: string
    default: "npm run build"
    description: "Command to run for building"

recipe:
  description: "Build {{params.language}} application"
  inputs:
    - "src/**/*"
    - "package.json"
  run: |
    echo "Building {{params.language}} project..."
    {{params.build_command}}
```

Use templates in cookbooks by referencing them:

```yaml
# app/cookbook.yml
recipes:
  build:
    template: build-template
    params:
      language: "node"
      build_command: "npm run build:prod"
```

Templates support validation, defaults, inheritance, and can be combined with regular recipe definitions.

## Execution Planning & Debugging

Bake provides powerful tools to understand and debug your project configuration:

### Execution Plan Visualization

Use the `--show-plan` flag to see a clean tree-style visualization of how recipes will be executed:

```bash
# Show execution plan for all recipes
bake --show-plan

# Show execution plan for specific recipes
bake app:build --show-plan
```

This displays a tree structure showing the execution order and dependencies between recipes.

### Configuration Debugging

Use the `--render` flag to see your complete resolved configuration with all variables substituted:

```bash
# Render complete project configuration
bake --render

# Render specific cookbook/recipe with dependencies
bake app:build --render

# Render with variable overrides to see how they affect the output
bake app:build --render --var environment=production
```

This outputs clean YAML showing exactly how bake interprets your configuration, making it perfect for debugging template variables and understanding complex configurations.

## Auto-Updates

Bake includes an auto-update feature that keeps your installation up to date automatically. The tool checks for updates
periodically based on your configuration and stores the last check time to avoid excessive API calls.

You can configure update behavior in your `bake.yml` file or use CLI commands:

```bash
# Check for updates (bypasses interval)
bake --check-updates

# Perform self-update
bake --self-update

# Update to latest version (including prereleases)
bake --self-update --prerelease

# Update project configuration to current bake version
bake --update-version

# Force run with version mismatch
bake --force-version-override
```

For detailed configuration options, see [Auto-Update Documentation](./docs/auto-update.md).

## Version Management

Bake tracks the minimum version required by project configurations:

```yaml
config:
  minVersion: "0.8.1"
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
in the form `<cookbook>:<recipe>`. By default, cookbook and recipe names are matched exactly, but you can use the `--regex` flag to enable regex pattern matching.

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

You can also use regex patterns with the `--regex` flag:

```sh
# Run all recipes in cookbooks starting with "app"
bake --regex '^app.*:'

# Run all recipes ending with "test" across all cookbooks
bake --regex ':.*test$'

# Run build recipes in cookbooks matching a pattern
bake --regex '^(frontend|backend):build$'
```

## Caching

By default, bake caches runs locally in a directory called `.bake/cache`. Bake will use the combined hash of all inputs of
a recipe, the hash of its dependencies and its run command to create a cache key. This allows for recipes to be cached
and only run again if either a dependency or the recipe itself changes. Bake can also be configured to use a remote storage
to cache recipes such as S3 or GCS.

For more information on how to configure caching, please see [Caching](./docs/configuration.md#caching).

## Documentation

- [Configuration Guide](./docs/configuration.md) - Complete configuration reference
- [Recipe Templates](./docs/recipe-templates.md) - Reusable recipe definitions and DRY patterns
- [Auto-Update Documentation](./docs/auto-update.md) - Update system configuration
