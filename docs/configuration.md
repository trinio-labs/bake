# 🚧 WIP 🚧

# Configuration

Bake uses YAML configuration files to define projects, cookbooks, and recipes. This document covers all configuration options and provides examples.

## Project Configuration (`bake.yml`)

The root `bake.yml` file defines the project structure and global settings.

### Basic Structure

```yaml
# Project-level variables (optional)
variables:
  environment: production
  version: "1.0.0"

# Cookbooks in this project
cookbooks:
  - foo
  - bar

# Global configuration
config:
  max_parallel: 4
  fast_fail: true
  verbose: false
  clean_environment: false
  
  # Cache configuration
  cache:
    local:
      enabled: true
      path: .bake/cache
    remotes:
      s3:
        bucket: my-bake-cache
        region: us-west-2
      gcs:
        bucket: my-bake-cache
    order: ["local", "s3", "gcs"]
  
  # Update configuration
  update:
    enabled: true
    check_interval_days: 7
    auto_update: false
    prerelease: false

# Version tracking (optional)
bake_version: "0.6.0"
```

### Variables

Project-level variables are available throughout all cookbooks and recipes. They can reference environment variables and other variables:

```yaml
variables:
  # Simple variables
  environment: production
  version: "1.0.0"
  
  # Variables that reference environment variables
  node_env: "{{env.NODE_ENV}}"
  
  # Variables that reference other variables
  full_version: "{{var.version}}-{{var.environment}}"
  
  # Variables that reference project constants
  project_name: "{{project.name}}"
```

### Cookbooks

The `cookbooks` list specifies which directories contain cookbook configurations:

```yaml
cookbooks:
  - frontend
  - backend
  - shared
```

### Configuration Options

#### Tool Configuration

- `max_parallel`: Maximum number of recipes to run in parallel (default: CPU cores - 1)
- `fast_fail`: Stop execution on first failure (default: true)
- `verbose`: Enable verbose output (default: false)
- `clean_environment`: Run recipes in clean environment (default: false)

#### Cache Configuration

- `local.enabled`: Enable local caching (default: true)
- `local.path`: Local cache directory (default: `.bake/cache`)
- `remotes.s3`: S3 cache configuration
- `remotes.gcs`: Google Cloud Storage cache configuration
- `order`: Cache strategy priority order

#### Update Configuration

- `enabled`: Enable update checks (default: true)
- `check_interval_days`: Days between update checks (default: 7)
- `auto_update`: Automatically install updates (default: false)
- `prerelease`: Include prerelease versions (default: false)

## Cookbook Configuration (`cookbook.yml`)

Each cookbook is defined by a `cookbook.yml` file in its directory.

### Basic Structure

```yaml
name: my-cookbook

# Cookbook-level variables
variables:
  build_type: release
  output_dir: dist

# Environment variables to include
environment:
  - NODE_ENV
  - BUILD_TYPE

# Recipes in this cookbook
recipes:
  build:
    description: "Build the application"
    inputs:
      - "src/**/*"
      - "package.json"
    outputs:
      - "dist/**/*"
    run: |
      npm install
      npm run build
    dependencies:
      - test
      - other-cookbook:build
  
  test:
    description: "Run tests"
    inputs:
      - "src/**/*"
      - "test/**/*"
    outputs:
      - "coverage/**/*"
    run: |
      npm test
    cache:
      inputs:
        - "src/**/*.js"
        - "test/**/*.js"
      outputs:
        - "coverage/lcov.info"
```

### Cookbook Variables

Cookbook variables inherit from project variables and can reference them:

```yaml
variables:
  # Reference project variables
  build_env: "{{var.environment}}"
  
  # Reference project constants
  cookbook_path: "{{cookbook.root}}"
  
  # Define cookbook-specific variables
  package_name: "my-package"
  build_command: "npm run build-{{var.build_env}}"
```

### Recipe Configuration

Each recipe defines a task that can be executed and cached.

#### Required Fields

- `run`: The command to execute (shell script)

#### Optional Fields

- `description`: Human-readable description
- `inputs`: Files that affect the recipe (glob patterns)
- `outputs`: Files produced by the recipe (glob patterns)
- `dependencies`: Other recipes that must run first
- `variables`: Recipe-specific variables
- `environment`: Environment variables to include
- `cache`: Cache configuration (inputs/outputs)

#### Dependencies

Dependencies can be specified as:

```yaml
dependencies:
  # Same cookbook recipe
  - test
  
  # Other cookbook recipe
  - other-cookbook:build
  
  # Multiple dependencies
  - test
  - other-cookbook:build
  - third-cookbook:prepare
```

#### Cache Configuration

Recipes can specify cache inputs and outputs separately:

```yaml
cache:
  inputs:
    - "src/**/*.js"
    - "package.json"
  outputs:
    - "dist/**/*"
    - "build.log"
```

#### Variables in Recipes

Recipe variables have access to project, cookbook, and recipe variables:

```yaml
variables:
  # Reference project variables
  env: "{{var.environment}}"
  
  # Reference cookbook variables
  build_type: "{{var.build_type}}"
  
  # Recipe-specific variables
  output_name: "app-{{var.env}}"
```

## Variable System

Bake provides a comprehensive variable system with multiple scopes and types.

### Variable Types

1. **Environment Variables**: `{{env.VARIABLE_NAME}}`
2. **User Variables**: `{{var.variable_name}}`
3. **Constants**: `{{project.root}}`, `{{cookbook.root}}`

### Variable Scoping

Variables are resolved in the following order (later takes precedence):

1. Project variables
2. Cookbook variables
3. Recipe variables
4. Override variables (from command line)

### Variable Context

Variables can reference other variables:

```yaml
variables:
  base_url: "https://api.example.com"
  version: "1.0.0"
  full_url: "{{var.base_url}}/v{{var.version}}"
```

### Built-in Constants

- `{{project.root}}`: Project root directory
- `{{cookbook.root}}`: Cookbook directory
- `{{recipe.name}}`: Recipe name
- `{{recipe.cookbook}}`: Cookbook name

## Examples

### Simple Project

```yaml
# bake.yml
cookbooks:
  - app

config:
  max_parallel: 2
  cache:
    local:
      enabled: true
```

```yaml
# app/cookbook.yml
name: app

recipes:
  build:
    inputs:
      - "src/**/*"
    outputs:
      - "dist/**/*"
    run: |
      echo "Building application..."
      mkdir -p dist
      cp src/* dist/
  
  test:
    inputs:
      - "src/**/*"
      - "test/**/*"
    run: |
      echo "Running tests..."
      echo "All tests passed!"
    dependencies:
      - build
```

### Complex Project with Variables

```yaml
# bake.yml
variables:
  environment: production
  version: "2.1.0"

cookbooks:
  - frontend
  - backend
  - shared

config:
  max_parallel: 4
  cache:
    local:
      enabled: true
    remotes:
      s3:
        bucket: my-bake-cache
    order: ["local", "s3"]
```

```yaml
# frontend/cookbook.yml
name: frontend

variables:
  build_env: "{{var.environment}}"
  output_dir: "dist-{{var.build_env}}"

recipes:
  build:
    inputs:
      - "src/**/*"
      - "package.json"
    outputs:
      - "{{var.output_dir}}/**/*"
    run: |
      echo "Building frontend for {{var.build_env}}..."
      npm install
      npm run build:{{var.build_env}}
    cache:
      inputs:
        - "src/**/*.js"
        - "src/**/*.css"
      outputs:
        - "{{var.output_dir}}/index.html"
```

## Best Practices

1. **Use meaningful recipe names**: `build`, `test`, `deploy` instead of `task1`, `task2`
2. **Specify inputs and outputs**: This enables proper caching and dependency detection
3. **Use variables for configuration**: Avoid hardcoding values
4. **Group related tasks in cookbooks**: Keep cookbooks focused and cohesive
5. **Use descriptive dependencies**: Make the dependency graph clear and logical
6. **Test your configuration**: Run `bake --dry-run` to validate your setup
7. **Version your projects**: Use `bake_version` to track compatibility

## Command Line Variables

You can override variables from the command line:

```bash
# Override a single variable
bake --var environment=staging

# Override multiple variables
bake --var environment=staging --var version=2.0.0

# Override variables for specific recipes
bake frontend:build --var build_type=debug
```

## Version Management

Bake tracks the version used to create project configurations:

```yaml
bake_version: "0.6.0"
```

This helps detect compatibility issues when using different bake versions. You can:

- Update the version: `bake --update-version`
- Force override: `bake --force-version-override`
