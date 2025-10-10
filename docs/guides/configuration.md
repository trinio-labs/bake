# Configuration Guide

Bake uses YAML configuration files to define projects, cookbooks, and recipes. This guide covers the structure and options for each configuration file type.

## Configuration File Types

Bake uses three types of configuration files:

- **`bake.yml`** - Project-level configuration in the project root
- **`cookbook.yml`** - Cookbook configuration in each cookbook directory
- **`.bake/templates/*.yml`** - Recipe templates for reusable patterns
- **`.bake/helpers/*.yml`** - Custom Handlebars helpers for template functionality

## Project Configuration (`bake.yml`)

The root `bake.yml` file defines project-wide settings and variables for your project. Cookbooks are automatically discovered by finding `cookbook.yml` files throughout your directory tree.

### Basic Structure

```yaml
# bake.yml
name: "My Application"
description: "Example Bake project"

# Define project-wide variables  
variables:
  environment: development
  version: "1.0.0"
  node_version: "18"

# Global configuration
config:
  max_parallel: 4
  fast_fail: true
  verbose: false
  clean_environment: false
  minVersion: "0.11.0"
  
  # Cache configuration
  cache:
    local:
      enabled: true
      path: .bake/cache
      
  # Update configuration  
  update:
    enabled: true
    check_interval_days: 7
    auto_update: false
```

### Configuration Options

#### Project Metadata

- **`name`** (optional) - Human-readable project name
- **`description`** (optional) - Project description

#### Cookbook Discovery

Cookbooks are automatically discovered by recursively scanning your project directory for files named `cookbook.yml` or `cookbook.yaml`. No manual configuration is required.

```bash
# Example project structure - cookbooks are discovered automatically:
my-project/
├── bake.yml                    # Project configuration
├── frontend/
│   ├── cookbook.yml           # Frontend cookbook (auto-discovered)
│   └── src/
├── backend/
│   ├── cookbook.yml           # Backend cookbook (auto-discovered)
│   └── src/
├── services/
│   └── auth/
│       └── cookbook.yml       # Auth service cookbook (auto-discovered)
└── libs/
    ├── shared/
    │   └── cookbook.yml       # Shared library cookbook (auto-discovered)
    └── utils/
        └── cookbook.yml       # Utils cookbook (auto-discovered)
```

#### Tool Configuration

- **`max_parallel`** - Maximum recipes to run simultaneously (default: CPU cores - 1)
- **`fast_fail`** - Stop all execution on first failure (default: `true`)  
- **`verbose`** - Enable detailed output (default: `false`)
- **`clean_environment`** - Run recipes with clean environment variables (default: `false`)
- **`minVersion`** - Minimum Bake version required (default: none)

```yaml
config:
  max_parallel: 8           # Run up to 8 recipes in parallel
  fast_fail: false         # Continue execution despite failures
  verbose: true            # Show detailed output
  clean_environment: true  # Clean env vars for all recipes
  minVersion: "0.11.0"     # Require Bake v0.11.0 or later
```

#### Variable System

Define project-wide variables available to all cookbooks and recipes.

**See the [Variables Guide](variables.md) for complete documentation.**

#### Cache Configuration

Configure local and remote caching for better performance.

**See the [Caching Guide](caching.md) for complete documentation.**

#### Update Configuration

Control Bake's auto-update behavior.

**See the [Auto-Update Reference](../reference/auto-update.md) for complete documentation.**

## Cookbook Configuration (`cookbook.yml`)

Each cookbook directory contains a `cookbook.yml` file that defines the cookbook's recipes and configuration.

### Basic Structure

```yaml  
# cookbook.yml
name: frontend
description: "React frontend application"

# Cookbook-level variables
variables:
  app_name: "frontend-app"  
  build_env: "{{var.environment}}"
  output_dir: "dist-{{var.build_env}}"

# Environment variables to expose to recipes
environment:
  - NODE_ENV
  - API_URL
  - BUILD_ID

# Recipe definitions
recipes:
  install:
    description: "Install dependencies"
    cache:
      inputs:
        - "package.json"
        - "package-lock.json"  
    run: npm ci

  build:
    description: "Build the application"
    cache:
      inputs:
        - "src/**/*"
        - "package.json"
        - "tsconfig.json"
      outputs:
        - "{{var.output_dir}}/**/*"
    dependencies:
      - install
      - shared:build
    environment:
      - NODE_ENV
    variables:
      NODE_ENV: "{{var.build_env}}"
    run: |
      echo "Building {{var.app_name}} for {{var.build_env}}"
      npm run build
```

### Cookbook Properties

#### Metadata

- **`name`** (required) - Cookbook identifier used in dependencies (`cookbook:recipe`)
- **`description`** (optional) - Human-readable description

#### Variables

Cookbook-level variables inherit from project variables and can be overridden at the recipe level.

```yaml
variables:
  # Reference project variables
  environment: "{{var.environment}}"
  
  # Define new variables  
  service_name: "my-service"
  port: 3000
  
  # Computed variables
  service_url: "http://localhost:{{var.port}}"
```

#### Environment Variables

List environment variables that recipes in this cookbook may access:

```yaml
environment:
  - NODE_ENV          # Available to all recipes
  - DATABASE_URL      
  - API_KEY
```

#### Recipes

Define the tasks that can be executed in this cookbook.

## Recipe Configuration

Recipes are individual tasks defined within cookbooks. Each recipe specifies its inputs, outputs, dependencies, and execution command.

### Basic Recipe Structure

```yaml
recipes:
  recipe-name:
    description: "Human-readable description"  # Optional but recommended
    inputs:             # Files that affect this recipe
      - "src/**/*.ts"
      - "package.json"
    outputs:           # Files produced by this recipe  
      - "dist/**/*"
    dependencies:      # Other recipes that must run first
      - install
      - shared:build
    environment:       # Environment variables for this recipe
      - NODE_ENV
      - BUILD_TARGET  
    variables:         # Recipe-specific variables
      BUILD_TARGET: production
    run: |             # Command to execute (required)
      echo "Building..."
      npm run build
```

### Recipe Properties

#### Required Properties

- **`run`** - Shell command to execute (string or multi-line string)

#### Optional Properties

- **`description`** - Human-readable description
- **`inputs`** - Glob patterns for input files (affects caching)
- **`outputs`** - Glob patterns for output files (affects caching)  
- **`dependencies`** - List of recipes that must run first
- **`environment`** - Environment variables to include
- **`variables`** - Recipe-specific variable definitions
- **`template`** - Use a recipe template instead of `run` command

### Dependencies

Recipes can depend on other recipes within the same cookbook or in other cookbooks:

```yaml
dependencies:
  # Same cookbook
  - install
  - test
  
  # Other cookbooks  
  - shared:build
  - backend:migrate
  
  # Multiple dependencies
  - [install, shared:build]  # Alternative syntax
```

### Input and Output Patterns

Use glob patterns to specify files that affect recipe execution and caching:

```yaml
cache:
  inputs:
    # Include specific file types
    - "src/**/*.{ts,tsx,js,jsx}"
    - "test/**/*.{ts,js}"
    
    # Include configuration files
    - "package.json" 
    - "tsconfig.json"
    - ".env"
    
    # Include files from other directories
    - "../shared/dist/**/*"              # Relative path
    - "{{project.root}}/config/**/*"     # Absolute path
    
  outputs:  
    # Generated files
    - "dist/**/*"
    - "build/**/*"
    
    # Specific files
    - "bundle-stats.json"
    - "coverage/lcov.info"
    
    # Variable-based paths
    - "{{var.output_dir}}/**/*"
```

### Environment Variables

Specify environment variables that affect recipe output:

```yaml
recipes:
  build:
    environment:
      - NODE_ENV        # Different build for dev/prod
      - API_URL         # Affects build configuration
      - FEATURE_FLAGS   # Conditional compilation
      
    run: |
      echo "NODE_ENV: $NODE_ENV"
      echo "API_URL: $API_URL" 
      npm run build
```

### Recipe Templates

Use templates instead of defining recipes from scratch:

```yaml
recipes:
  build:
    # Use a template instead of 'run' command
    template: node-build-template
    params:
      service_name: "frontend-app"
      build_command: "npm run build:prod"
      port: 3000
```

**See the [Recipe Templates Guide](recipe-templates.md) for complete documentation.**

## Custom Helpers

Extend template functionality with custom Handlebars helpers that can execute shell commands, transform strings, and process data.

### Helper Definition

Create helpers in the `.bake/helpers/` directory:

```yaml
# .bake/helpers/uppercase.yml
name: uppercase
description: Convert text to uppercase
returns: string
parameters:
  text:
    type: string
    required: true
    description: The text to convert
run: |
  echo "{{params.text}}" | tr '[:lower:]' '[:upper:]'
```

### Using Helpers in Recipes

```yaml
recipes:
  build:
    run: |
      # Built-in shell helper
      echo "Git branch: {{shell 'git rev-parse --abbrev-ref HEAD'}}"

      # Custom helper
      echo "{{uppercase text="hello world"}}"  # Outputs: HELLO WORLD

      # Helper with arrays
      {{#each (shell_lines 'ls *.txt')}}
      process-file {{this}}
      {{/each}}
```

### Helper Features

- **Typed Parameters** - string, number, boolean, array, object
- **Default Values** - Optional parameters with defaults
- **Variables** - Helper-specific variables and environment access
- **Return Types** - String or array output
- **Caching** - Results cached based on rendered script

**See the [Custom Helpers Guide](custom-helpers.md) for complete documentation.**

## Configuration Examples

### Simple Project

```yaml
# bake.yml
name: "Simple App"

config:
  max_parallel: 2
```

```yaml
# app/cookbook.yml  
name: app
recipes:
  build:
    cache:
      inputs: ["src/**/*"]
      outputs: ["dist/**/*"] 
    run: |
      mkdir -p dist
      cp src/* dist/
```

### Multi-Service Project

```yaml
# bake.yml
name: "E-commerce Platform"

variables:
  environment: development
  version: "2.1.0"
  api_base: "https://api-{{var.environment}}.shop.com"

config:
  max_parallel: 6
  fast_fail: true
  cache:
    local:
      enabled: true
    remotes:
      s3:
        bucket: team-build-cache
        prefix: "shop/{{var.environment}}"
    order: ["local", "s3"]
```

### Complex Recipe Dependencies

```yaml
# deployment/cookbook.yml
name: deployment

recipes:
  test-all:
    dependencies:
      - frontend:test
      - api:test  
      - workers:test
    run: echo "All tests passed"
    
  build-images:
    dependencies:
      - frontend:build
      - api:build
      - workers:build
    run: |
      docker build -t shop/frontend:{{var.version}} frontend/
      docker build -t shop/api:{{var.version}} api/
      docker build -t shop/workers:{{var.version}} workers/
      
  deploy-staging:
    dependencies:
      - test-all
      - build-images
    variables:
      target: staging
    run: |
      kubectl apply -f k8s/staging/
      ./scripts/deploy.sh {{var.target}}
```

## Configuration Validation

### Built-in Validation

Bake validates configuration files automatically:

```bash
# Validate all configuration
bake --validate

# Validate specific cookbook
bake frontend: --validate

# Check for common issues
bake --lint-config
```

### JSON Schema Support

Use JSON Schema for IDE validation and autocompletion:

**See the [Schema Documentation](../development/schemas.md) for setup instructions.**

## Best Practices

### 1. Use Clear Naming

```yaml
# Good
cookbooks:
  - user-service
  - payment-api
  - admin-dashboard

recipes:
  build-production:
    description: "Build optimized production bundle"
    
# Avoid  
cookbooks:
  - svc1
  - api
  - dash
```

### 2. Define Meaningful Dependencies

```yaml
# Express logical dependencies
recipes:
  deploy:
    dependencies: [build, test, security-scan]
    
  integration-test:
    dependencies: [deploy]
```

### 3. Use Specific Input Patterns

```yaml  
# Too broad
cache:
  inputs: ["**/*"]

# Better
cache:
  inputs: ["src/**/*.ts", "package.json", "tsconfig.json"]

# Best  
cache:
  inputs: 
    - "src/**/*.{ts,tsx}"      # Source files
    - "!src/**/*.{test,spec}.ts"  # Exclude tests
    - "package.json"           # Dependencies
    - "tsconfig.json"          # Build config
```

### 4. Document Complex Recipes

```yaml
recipes:
  deploy-production:
    description: |
      Deploy to production environment with:
      - Blue/green deployment strategy
      - Database migrations
      - Cache warmup
      - Smoke tests
    dependencies: [build, test, security-audit]
    run: |
      ./scripts/deploy-production.sh
```

### 5. Use Variables for Flexibility

```yaml
variables:
  # Configurable values
  node_version: "18"
  build_mode: "production"
  target_env: "{{var.environment}}"

recipes:
  setup:
    run: |
      nvm use {{var.node_version}}
      npm run build:{{var.build_mode}}
```

## Troubleshooting

### Common Issues

**Cookbook not found**:
- Verify cookbook directory exists and contains `cookbook.yml`
- Check that the `cookbook.yml` file is valid YAML
- Ensure the cookbook directory is within your project tree

**Recipe dependency errors**:  
- Use `cookbook:recipe` format for cross-cookbook dependencies
- Check recipe names are spelled correctly
- Verify dependent cookbooks exist

**Variable resolution errors**:
- Check variable names and scoping  
- Use `bake --render` to debug variable resolution
- Verify template syntax is correct

**Cache not working**:
- Ensure input/output patterns are correct
- Check file paths are relative to cookbook root
- Verify cache configuration is valid

## Related Documentation

- [Variables Guide](variables.md) - Complete variable system documentation
- [Caching Guide](caching.md) - Cache configuration and optimization
- [Recipe Templates](recipe-templates.md) - Reusable recipe patterns
- [Custom Helpers](custom-helpers.md) - Custom Handlebars helpers for templates
- [CLI Commands](../reference/cli-commands.md) - Command-line options
- [Configuration Schema](../reference/configuration-schema.md) - Complete schema reference