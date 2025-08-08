# Variables Guide

Bake provides a powerful hierarchical variable system that allows you to template and customize your project configuration. Variables can be used throughout all configuration files to make your project flexible and maintainable.

## Overview

Variables in Bake follow a hierarchical scoping system where values can be overridden at different levels:

1. **Project variables** (defined in `bake.yml`)
2. **Cookbook variables** (defined in `cookbook.yml`)
3. **Recipe variables** (defined in individual recipes)
4. **Command-line overrides** (passed via `--var` flag)

## Variable Types

### User Variables

User-defined variables that you define in your configuration files.

**Access with**: `{{var.variable_name}}`

```yaml
# bake.yml
variables:
  environment: development
  version: "1.0.0"
  build_type: debug

# In recipes, use as:
run: |
  echo "Building version {{var.version}} for {{var.environment}}"
  npm run build:{{var.build_type}}
```

### Environment Variables

Access system environment variables from your recipes.

**Access with**: `{{env.VARIABLE_NAME}}`

```yaml
# Access environment variables
variables:
  node_env: "{{env.NODE_ENV}}"
  ci_build: "{{env.CI}}"
  
recipes:
  deploy:
    environment:
      - NODE_ENV
      - API_KEY
    run: |
      echo "NODE_ENV is {{env.NODE_ENV}}"
      echo "Using API key: {{env.API_KEY}}"
```

### Built-in Constants

Bake provides several built-in constants for common paths and metadata.

**Project Constants**:
- `{{project.root}}` - Absolute path to project root directory
- `{{project.name}}` - Project name from configuration

**Cookbook Constants**:
- `{{cookbook.root}}` - Absolute path to cookbook directory
- `{{cookbook.name}}` - Name of the current cookbook

**Recipe Constants**:
- `{{recipe.name}}` - Name of the current recipe
- `{{recipe.cookbook}}` - Name of the cookbook containing this recipe

```yaml
recipes:
  build:
    run: |
      echo "Building {{recipe.cookbook}}:{{recipe.name}}"
      cd {{project.root}}
      echo "Cookbook directory: {{cookbook.root}}"
```

## Variable Definition

### Project-Level Variables

Defined in `bake.yml`, available to all cookbooks and recipes:

```yaml
# bake.yml
variables:
  # Simple string variables
  environment: development
  version: "1.0.0"
  
  # Variables referencing environment
  node_env: "{{env.NODE_ENV}}"
  debug_mode: "{{env.DEBUG}}"
  
  # Computed variables referencing other variables
  full_version: "{{var.version}}-{{var.environment}}"
  api_base_url: "https://api-{{var.environment}}.example.com"
  
  # Project metadata
  project_name: "{{project.name}}"
  build_timestamp: "{{env.BUILD_TIMESTAMP}}"
```

### Cookbook-Level Variables

Defined in `cookbook.yml`, available to all recipes in that cookbook:

```yaml
# cookbook.yml
name: frontend
variables:
  # Reference project variables
  build_env: "{{var.environment}}"
  app_version: "{{var.version}}"
  
  # Cookbook-specific variables
  package_name: "@myapp/frontend"
  output_dir: "dist-{{var.build_env}}"
  
  # Reference built-in constants
  source_path: "{{cookbook.root}}/src"
  
  # Computed values
  build_command: "npm run build:{{var.build_env}}"
```

### Recipe-Level Variables

Defined within individual recipe definitions:

```yaml
recipes:
  build:
    variables:
      # Recipe-specific variables
      webpack_mode: "{{var.build_env}}"
      output_path: "{{var.output_dir}}/bundle"
      
      # Override cookbook/project variables if needed
      build_env: production
    
    run: |
      echo "Building with webpack mode: {{var.webpack_mode}}"
      echo "Output path: {{var.output_path}}"
```

## Variable Scoping and Inheritance

Variables are resolved in hierarchical order, with later definitions overriding earlier ones:

```yaml
# bake.yml
variables:
  environment: development  # Project level
  port: 3000

# cookbook.yml  
variables:
  environment: staging      # Overrides project level
  service_name: api-server  # Cookbook level only

# recipe level
recipes:
  start:
    variables:
      port: 8080            # Overrides project level
    run: |
      # environment = staging (from cookbook)
      # port = 8080 (from recipe)  
      # service_name = api-server (from cookbook)
      echo "Starting {{var.service_name}} on port {{var.port}} for {{var.environment}}"
```

## Environment-Specific Configuration

### Using Overrides

Define environment-specific variable sets:

```yaml
# bake.yml
variables:
  # Default values
  environment: development
  api_url: "http://localhost:3001"
  debug: true
  replicas: 1

overrides:
  # Production overrides
  production:
    environment: production
    api_url: "https://api.prod.example.com"
    debug: false
    replicas: 3
    
  # Staging overrides  
  staging:
    environment: staging
    api_url: "https://api-staging.example.com" 
    replicas: 2
```

Activate overrides via command line:

```bash
# Use production overrides
bake --var environment=production

# Use staging overrides
bake --var environment=staging
```

### Environment Detection

Automatically detect environment from system variables:

```yaml
variables:
  # Detect from CI environment
  is_ci: "{{env.CI}}"
  branch_name: "{{env.GITHUB_REF_NAME}}"
  
  # Set environment based on branch
  environment: |
    {{#if (eq env.GITHUB_REF_NAME "main")}}
    production
    {{else if (eq env.GITHUB_REF_NAME "develop")}}
    staging  
    {{else}}
    development
    {{/if}}
```

## Advanced Variable Patterns

### Conditional Variables

Use Handlebars conditionals for dynamic values:

```yaml
variables:
  # Conditional based on environment
  database_url: |
    {{#if (eq var.environment "production")}}
    postgresql://prod-db:5432/myapp
    {{else if (eq var.environment "staging")}}  
    postgresql://staging-db:5432/myapp
    {{else}}
    postgresql://localhost:5432/myapp_dev
    {{/if}}
  
  # Conditional flags
  enable_debug: |
    {{#if (or (eq var.environment "development") env.DEBUG)}}
    true
    {{else}}
    false
    {{/if}}
```

### Array Variables

Define and iterate over arrays:

```yaml
variables:
  # Define array of services
  services:
    - name: frontend
      port: 3000
      path: "./frontend"
    - name: backend  
      port: 3001
      path: "./backend"
      
  # Build targets
  build_targets: ["web", "mobile", "desktop"]

recipes:
  build-all:
    run: |
      {{#each var.services}}
      echo "Building {{this.name}} on port {{this.port}}"
      cd {{this.path}} && npm run build
      {{/each}}
      
      {{#each var.build_targets}}
      echo "Creating {{this}} build"
      npm run build:{{this}}
      {{/each}}
```

### Complex Object Variables

Use nested objects for structured configuration:

```yaml
variables:
  # Database configuration
  database:
    host: "{{env.DB_HOST}}"
    port: 5432
    name: "myapp_{{var.environment}}"
    ssl: "{{#if (eq var.environment \"production\")}}require{{else}}prefer{{/if}}"
  
  # Service configuration
  services:
    api:
      image: "myapp/api:{{var.version}}"
      replicas: "{{var.api_replicas}}"
      resources:
        memory: "512Mi"
        cpu: "250m"
    
  # Feature flags
  features:
    new_ui: "{{env.ENABLE_NEW_UI}}"
    analytics: true
    debug_panel: "{{var.enable_debug}}"

recipes:
  deploy:
    run: |
      echo "Connecting to {{var.database.host}}:{{var.database.port}}"
      echo "Database: {{var.database.name}} (SSL: {{var.database.ssl}})"
      echo "API replicas: {{var.services.api.replicas}}"
      
      {{#if var.features.analytics}}
      echo "Analytics enabled"
      {{/if}}
```

## Command-Line Variable Overrides

Override any variable from the command line:

```bash
# Override single variables
bake --var environment=production
bake --var version=2.1.0  
bake --var debug=false

# Override multiple variables
bake --var environment=staging --var replicas=2 --var debug=true

# Override nested object properties  
bake --var database.host=remote-db.example.com
bake --var services.api.replicas=5

# Override for specific recipes
bake frontend:build --var environment=production
bake :test --var database.name=test_db
```

## Debugging Variables

### Render Configuration

See resolved variables in context:

```bash
# Render entire project configuration
bake --render

# Render specific cookbook  
bake frontend: --render

# Render with overrides to see changes
bake --render --var environment=production --var debug=false
```

### Variable Resolution Order

Understanding how variables are resolved:

1. **Project defaults** from `bake.yml`
2. **Cookbook overrides** from `cookbook.yml` 
3. **Recipe overrides** from recipe definitions
4. **Command-line overrides** from `--var` flags

Later values always take precedence.

### Common Issues

**Variable not found errors**:
```bash
# Check if variable is defined at the right scope
bake --render | grep -A5 -B5 "variable_name"
```

**Unexpected variable values**:
```bash
# See the resolution chain
bake --render --var debug_vars=true
```

**Template syntax errors**:
```bash
# Validate template syntax
bake --validate
```

## Best Practices

### 1. Use Descriptive Variable Names

```yaml
# Good
variables:
  api_base_url: "https://api.example.com"
  database_connection_timeout: 30
  enable_feature_x: true

# Avoid  
variables:
  url: "https://api.example.com"
  timeout: 30
  flag: true
```

### 2. Group Related Variables

```yaml
variables:
  # Database configuration
  database_host: "{{env.DB_HOST}}"
  database_port: 5432
  database_name: "myapp"
  
  # Or use nested objects
  database:
    host: "{{env.DB_HOST}}"
    port: 5432  
    name: "myapp"
```

### 3. Provide Sensible Defaults

```yaml
variables:
  # Always provide defaults for optional configuration
  debug: false
  max_parallel: 4
  cache_enabled: true
  
  # Use environment variables with fallbacks
  port: "{{env.PORT}}"
  database_url: "{{env.DATABASE_URL}}"
```

### 4. Document Complex Variables

```yaml
# Add comments explaining complex variables
variables:
  # Build configuration - affects output directory and optimization
  build_mode: "{{env.BUILD_MODE}}"  # Values: development, staging, production
  
  # Feature flags - toggle functionality without code changes
  features:
    new_dashboard: false  # Enable redesigned dashboard UI
    beta_api: true       # Use new API endpoints
    analytics: true      # Collect usage analytics
```

### 5. Use Environment-Specific Files

For complex projects, consider splitting variables:

```bash
# Project structure
├── bake.yml                    # Common variables
├── vars/
│   ├── development.yml        # Development overrides  
│   ├── staging.yml           # Staging overrides
│   └── production.yml        # Production overrides
└── cookbooks/...
```

## Integration with Recipe Templates

Variables work seamlessly with recipe templates:

```yaml
# Template definition
parameters:
  service_name:
    type: string
    required: true
  port:
    type: number
    default: 3000

template:
  run: |
    echo "Starting {{params.service_name}} on port {{params.port}}"
    echo "Environment: {{var.environment}}"
    echo "Version: {{var.version}}"
```

## Related Documentation

- [Configuration Guide](configuration.md) - Complete configuration reference
- [Recipe Templates](recipe-templates.md) - Using variables in templates
- [CLI Commands](../reference/cli-commands.md) - Command-line variable overrides
- [First Project Tutorial](../getting-started/first-project.md) - Variables in practice