# CLI Commands Reference

Complete reference for all Bake command-line options and usage patterns.

## Basic Usage

```bash
bake [OPTIONS] [RECIPE]
```

## Recipe Patterns

### Recipe Selection

Bake uses a flexible pattern system to select which recipes to run:

```bash
# Run all recipes in all cookbooks
bake

# Run specific recipe from specific cookbook
bake frontend:build

# Run all recipes in a cookbook
bake frontend:

# Run all recipes with specific name across all cookbooks  
bake :test

# Run multiple specific recipes
bake frontend:build backend:test
```

### Pattern Matching Modes

#### Exact Matching (Default)

By default, cookbook and recipe names are matched exactly:

```bash
# Matches exactly "frontend" cookbook and "build" recipe
bake frontend:build

# Matches exactly "web" cookbook, all recipes
bake web:

# Matches exactly "test" recipe in all cookbooks
bake :test
```

#### Regex Pattern Matching

Enable regex patterns with the `--regex` flag:

```bash
# Run all recipes in cookbooks starting with "app"
bake --regex '^app.*:'

# Run all recipes ending with "test" across all cookbooks
bake --regex ':.*test$'

# Run build recipes in frontend or backend cookbooks
bake --regex '^(frontend|backend):build$'

# Complex pattern: test recipes in service cookbooks
bake --regex '^.*-service:.*test.*$'
```

## Global Options

### Project Path

**`-p, --path <PATH>`**

Specify path to project directory or configuration file:

```bash
# Use different project directory
bake -p /path/to/project

# Use specific config file
bake -p /path/to/custom-bake.yml

# Use relative path
bake -p ./subproject

# Use with recipe pattern
bake -p ./frontend frontend:build
```

### Execution Control

**`-n, --dry-run`**

Show what would be executed without actually running recipes:

```bash
# See what would run
bake --dry-run

# See what specific recipe would do
bake --dry-run frontend:build

# Combine with verbose for detailed output
bake --dry-run --verbose frontend:build
```

**`-f, --fail-fast`**

Stop execution on first error (overrides project configuration):

```bash
# Stop on first failure
bake --fail-fast

# Continue despite failures (opposite)
bake --no-fail-fast
```

**`-j, --jobs <JOBS>`**

Set maximum number of concurrent recipe executions:

```bash
# Limit to 2 parallel jobs
bake -j 2

# Use all available CPU cores
bake -j $(nproc)

# Single-threaded execution
bake -j 1
```

### Output and Debugging

**`-v, --verbose`**

Enable verbose output (can be used multiple times for increased verbosity):

```bash
# Basic verbose output
bake -v

# More verbose output
bake -vv  

# Maximum verbose output
bake -vvv

# Verbose with specific recipe
bake -v frontend:build
```

**`-e, --show-plan`**

Display execution plan without running recipes:

```bash
# Show plan for all recipes
bake --show-plan

# Show plan for specific recipes
bake --show-plan frontend:build backend:test

# Show plan with verbose output
bake --show-plan -v
```

**`-t, --tree`**

Display execution plan in tree format:

```bash
# Tree view of execution plan
bake --tree

# Combine with show-plan for detailed tree
bake --show-plan --tree

# Tree view for specific recipes
bake --tree frontend:
```

## Variable Override

**`-D, --define <VARS>`**

Override variables from command line (can be used multiple times):

```bash
# Override single variable
bake --define environment=production

# Override multiple variables
bake --define environment=staging --define version=2.1.0

# Short form
bake -D debug=false -D replicas=3

# Override nested object properties
bake -D database.host=remote-db.com -D database.port=5432

# Use with specific recipes
bake frontend:build -D environment=production -D optimize=true
```

## Configuration and Validation

**`--render`**

Print resolved configuration with all variables and templates processed:

```bash
# Render entire project configuration
bake --render

# Render with variable overrides to see changes
bake --render -D environment=production

# Render specific cookbook/recipe
bake --render frontend:build

# Save rendered config to file
bake --render > rendered-config.yml
```

**`--list-templates`**

List all available recipe templates in the project:

```bash
# List all templates
bake --list-templates

# Example output:
# Available templates:
# - build-template: Generic build template for various languages
# - test-template: Generic test template with coverage options
# - deploy-template: Deployment template with environment support
```

**`--validate-templates`**

Validate all recipe templates for syntax and parameter errors:

```bash
# Validate all templates
bake --validate-templates

# Returns exit code 0 for success, non-zero for validation errors
```

## Cache Management

**`--cache <STRATEGY>`**

Override cache strategy at runtime. Controls which caches are used and in what order:

```bash
# Use only local cache (disable remote)
bake --cache local-only frontend:build

# Use only remote cache (disable local)
bake --cache remote-only frontend:build

# Check local cache first, then remote (typical default)
bake --cache local-first frontend:build

# Check remote cache first, then local
bake --cache remote-first frontend:build

# Disable all caching
bake --cache disabled frontend:build
```

**Available strategies:**
- `local-only` - Use only local filesystem cache
- `remote-only` - Use only remote caches (S3, GCS)
- `local-first` - Check local first, then remote
- `remote-first` - Check remote first, then local
- `disabled` - Disable all caching

**Use cases:**
```bash
# Development: fast local-only builds
bake --cache local-only

# CI/CD: use shared team cache
bake --cache remote-only --var environment=ci

# Debugging: force clean build
bake --cache disabled --verbose

# Fresh from team: prioritize remote cache
bake --cache remote-first
```

See [Caching Guide](../guides/caching.md#cli-cache-overrides) for detailed usage examples.

**`--skip-cache`**

Disable all caching (legacy flag, equivalent to `--cache disabled`):

```bash
# Skip all caches
bake --skip-cache

# Equivalent to:
bake --cache disabled
```

**`-c, --clean`**

Clean outputs and caches for selected recipes:

```bash
# Clean cache for specific recipe
bake --clean frontend:build

# Clean all caches
bake --clean

# Clean cookbook cache
bake --clean frontend:

# Clean specific recipe type across all cookbooks
bake --clean :test
```

## Update Management

**`--check-updates`**

Check for available Bake updates (bypasses interval checking):

```bash
# Check for updates
bake --check-updates

# Example output:
# Current version: 0.12.0
# Latest version:  0.12.1  
# Update available: bake --self-update
```

**`--self-update`**

Update Bake to the latest version:

```bash
# Update to latest stable version
bake --self-update

# Update to latest version including prereleases
bake --self-update --prerelease

# Check what would be updated (dry run)
bake --self-update --dry-run
```

**`--prerelease`**

Include prerelease versions when checking or updating:

```bash
# Check for prerelease updates
bake --check-updates --prerelease

# Update to latest prerelease  
bake --self-update --prerelease
```

## Information Commands

**`-h, --help`**

Display help information:

```bash
# Show help
bake --help

# Short help  
bake -h
```

**`-V, --version`**

Display version information:

```bash
# Show version
bake --version

# Example output:
# bake 0.12.0
```

## Advanced Usage Patterns

### Complex Recipe Selection

```bash
# Run tests in all service cookbooks
bake --regex '.*-service:.*test.*'

# Run build and test for frontend
bake frontend:build frontend:test

# Run all linting across project
bake :lint

# Run deployment pipeline
bake build test package deploy
```

### Environment-Specific Builds

```bash
# Development build
bake -D environment=development -D debug=true

# Staging deployment  
bake deploy-staging -D environment=staging -D replicas=2

# Production release
bake -D environment=production -D version=1.2.3 -D optimize=true
```

### Debugging and Analysis

```bash
# Analyze execution plan
bake --show-plan --tree -v

# Debug configuration issues
bake --render -D debug=true

# Verbose dry run
bake --dry-run -vv frontend:build

# Clean and rebuild with verbose output
bake --clean frontend:build && bake -v frontend:build
```

### Performance Tuning

```bash
# High-performance parallel build
bake -j 8 -D environment=production

# Memory-constrained build
bake -j 2 --verbose

# Single-threaded for debugging
bake -j 1 -vv frontend:build
```

### CI/CD Integration

```bash
# CI build with fail-fast
bake --fail-fast -D environment=ci -D version=$BUILD_NUMBER

# Deployment with checks
bake --check-updates && bake deploy -D environment=production

# Validate before execution
bake --validate-templates && bake --show-plan && bake
```

## Exit Codes

Bake uses standard exit codes to indicate execution status:

- **0**: Success - all recipes executed successfully
- **1**: General error - configuration error, recipe not found, etc.
- **2**: Recipe execution failure - one or more recipes failed
- **3**: Validation error - invalid configuration or templates
- **4**: Update error - failed to check for or install updates

```bash
# Check exit code in scripts
bake frontend:build
if [ $? -eq 0 ]; then
  echo "Build succeeded"
else
  echo "Build failed with exit code $?"
fi
```

## Configuration Files

Bake looks for configuration files in this order:

1. File specified with `-p/--path`
2. `bake.yml` in current directory
3. `bake.yaml` in current directory  
4. `bake.yml` in parent directories (recursive)

## Environment Variables

Bake respects these environment variables:

- **`BAKE_CONFIG_PATH`** - Default path to configuration file
- **`BAKE_CACHE_DIR`** - Override default cache directory
- **`BAKE_LOG_LEVEL`** - Set log level (error, warn, info, debug)
- **`BAKE_MAX_PARALLEL`** - Default parallel execution limit
- **`BAKE_NO_UPDATE_CHECK`** - Disable automatic update checks

```bash
# Use environment variables
export BAKE_LOG_LEVEL=debug
export BAKE_MAX_PARALLEL=4
bake frontend:build
```

## Integration Examples

### Makefile Integration

```makefile
# Makefile
.PHONY: build test deploy clean

build:
	bake :build

test:
	bake :test

deploy-staging:
	bake deploy -D environment=staging

clean:
	bake --clean
```

### GitHub Actions

```yaml
# .github/workflows/build.yml
name: Build
on: [push, pull_request]

jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      
      - name: Install Bake
        run: cargo install bake-cli
        
      - name: Build and Test
        run: |
          bake --fail-fast \
               -D environment=ci \
               -D version=${GITHUB_SHA::8} \
               build test
```

### Docker Integration

```dockerfile
# Dockerfile
FROM rust:1.75 as builder
RUN cargo install bake-cli

COPY . .
RUN bake -D environment=production build

FROM ubuntu:22.04
COPY --from=builder /app/dist /app/
```

## Related Documentation

- [Configuration Guide](../guides/configuration.md) - Configuration file reference
- [Variables Guide](../guides/variables.md) - Variable system usage
- [Recipe Templates](../guides/recipe-templates.md) - Template system
- [Troubleshooting](../guides/troubleshooting.md) - Common issues and solutions