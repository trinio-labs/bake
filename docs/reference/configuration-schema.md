# Configuration Schema Reference

Complete reference for all configuration options in Bake YAML files.

## Project Configuration (`bake.yml`)

The root project configuration file that defines global settings and project structure.

### Schema Overview

```yaml
# Optional project metadata
name: string                    # Project name
description: string             # Project description

# Required cookbook list
cookbooks: [string]             # List of cookbook directories

# Optional project variables
variables:
  key: value                    # Variable definitions

# Optional environment overrides
overrides:
  environment_name:             # Override set name
    key: value                  # Variable overrides

# Optional global configuration
config:
  max_parallel: number          # Maximum parallel recipes
  fast_fail: boolean           # Stop on first failure
  verbose: boolean             # Enable verbose output
  clean_environment: boolean   # Use clean environment
  minVersion: string           # Minimum Bake version
  
  cache:                       # Cache configuration
    local:                     # Local cache settings
      enabled: boolean
      path: string
      max_size: string
      retention_days: number
    remotes:                   # Remote cache providers
      s3:                      # AWS S3 cache
        bucket: string
        region: string
        prefix: string
        endpoint: string
        encryption: string
      gcs:                     # Google Cloud Storage
        bucket: string
        prefix: string
        endpoint: string
    order: [string]            # Cache priority order
    
  update:                      # Update configuration
    enabled: boolean
    check_interval_days: number
    auto_update: boolean
    prerelease: boolean
```

### Detailed Field Reference

#### Project Metadata

**`name`** (optional, string)
- Human-readable project name
- Used in built-in variables as `{{project.name}}`
- Default: directory name

**`description`** (optional, string)  
- Project description for documentation
- No functional impact

#### Cookbook Discovery

**`cookbooks`** (required, array of strings)
- List of directories containing `cookbook.yml` files
- Paths relative to project root
- Supports glob patterns

```yaml
cookbooks:
  - frontend                   # Simple directory
  - backend                   # Simple directory
  - services/auth             # Nested directory
  - "libs/*"                  # Glob pattern
```

#### Variables

**`variables`** (optional, object)
- Project-wide variable definitions
- Available to all cookbooks and recipes
- Can reference environment variables and other variables

```yaml
variables:
  # Simple values
  environment: development
  version: "1.0.0"
  debug: true
  port: 3000
  
  # Environment variable references
  node_env: "{{env.NODE_ENV}}"
  api_key: "{{env.API_KEY}}"
  
  # Variable references
  full_version: "{{var.version}}-{{var.environment}}"
  api_url: "https://api-{{var.environment}}.example.com"
  
  # Complex objects
  database:
    host: "{{env.DB_HOST}}"
    port: 5432
    name: "myapp_{{var.environment}}"
    
  # Arrays
  build_targets: ["web", "mobile", "desktop"]
  
  # Conditional values (using Handlebars)
  log_level: |
    {{#if (eq var.environment "production")}}
    warn
    {{else}}
    debug
    {{/if}}
```

#### Environment Overrides

**`overrides`** (optional, object)
- Environment-specific variable overrides
- Keys are override set names
- Activated via CLI or environment detection

```yaml
variables:
  environment: development
  debug: true
  replicas: 1

overrides:
  production:
    environment: production
    debug: false
    replicas: 3
    api_url: "https://api.prod.example.com"
    
  staging:
    environment: staging
    debug: true
    replicas: 2
    api_url: "https://api-staging.example.com"
```

#### Tool Configuration

**`config.max_parallel`** (optional, integer, default: CPU cores - 1)
- Maximum number of recipes to execute simultaneously
- Range: 1 to system CPU cores

**`config.fast_fail`** (optional, boolean, default: true)
- Stop execution on first recipe failure
- When false, continues running independent recipes

**`config.verbose`** (optional, boolean, default: false)
- Enable verbose output by default
- Can be overridden by CLI flags

**`config.clean_environment`** (optional, boolean, default: false)
- Run recipes in clean environment (no inherited env vars)
- When true, only explicitly listed environment variables are available

**`config.minVersion`** (optional, string)
- Minimum required Bake version
- Uses semantic versioning format
- Prevents running with incompatible versions

```yaml
config:
  max_parallel: 6
  fast_fail: false
  verbose: true
  clean_environment: true
  minVersion: "0.11.0"
```

#### Cache Configuration

**`config.cache.local`** (optional, object)
- Local filesystem cache settings

```yaml
config:
  cache:
    local:
      enabled: true                    # Enable local cache
      path: ".bake/cache"             # Cache directory (default)
      max_size: "5GB"                 # Maximum cache size (optional)
      retention_days: 7               # Auto-cleanup policy (optional)
```

**`config.cache.remotes.s3`** (optional, object)
- AWS S3 remote cache configuration

```yaml
config:
  cache:
    remotes:
      s3:
        bucket: "my-build-cache"      # S3 bucket name (required)
        region: "us-west-2"           # AWS region (required)
        prefix: "team/{{var.env}}"    # Key prefix (optional)
        endpoint: "https://s3.com"    # Custom endpoint (optional)
        encryption: "AES256"          # Server-side encryption (optional)
```

**`config.cache.remotes.gcs`** (optional, object)
- Google Cloud Storage remote cache configuration

```yaml
config:
  cache:
    remotes:
      gcs:
        bucket: "my-build-cache"      # GCS bucket name (required)
        prefix: "builds/{{var.ver}}"  # Key prefix (optional)  
        endpoint: "https://storage.googleapis.com"  # Custom endpoint (optional)
```

**`config.cache.order`** (optional, array of strings)
- Cache lookup and storage priority
- First match for reads, all configured for writes

```yaml
config:
  cache:
    order: ["local", "s3", "gcs"]     # Check local first, then S3, then GCS
```

#### Update Configuration

**`config.update`** (optional, object)
- Auto-update behavior settings

```yaml
config:
  update:
    enabled: true                     # Enable update checks
    check_interval_days: 7            # Days between checks
    auto_update: false                # Automatically install updates
    prerelease: false                 # Include prerelease versions
```

## Cookbook Configuration (`cookbook.yml`)

Cookbook-specific configuration defining recipes and local settings.

### Schema Overview

```yaml
# Required cookbook identifier
name: string                    # Cookbook name

# Optional metadata
description: string             # Cookbook description

# Optional cookbook variables
variables:
  key: value                    # Variable definitions

# Optional environment variables
environment: [string]           # Environment variables to expose

# Required recipe definitions
recipes:
  recipe_name:                  # Recipe identifier
    # Recipe properties
    description: string         # Recipe description
    run: string                # Command to execute
    cache:                     # Cache configuration
      inputs: [string]         # Input file patterns
      outputs: [string]        # Output file patterns
    dependencies: [string]     # Recipe dependencies
    environment: [string]      # Environment variables
    variables:                 # Recipe variables
      key: value
    template: string           # Template name
    params:                    # Template parameters
      key: value
```

### Detailed Field Reference

#### Cookbook Metadata

**`name`** (required, string)
- Unique cookbook identifier
- Used in cross-cookbook dependencies (`cookbook:recipe`)
- Must match directory name by convention

**`description`** (optional, string)
- Human-readable cookbook description
- Used for documentation

#### Cookbook Variables

**`variables`** (optional, object)
- Cookbook-level variable definitions
- Inherit from project variables
- Available to all recipes in cookbook

```yaml
variables:
  # Reference project variables
  build_env: "{{var.environment}}"
  app_version: "{{var.version}}"
  
  # Cookbook-specific variables
  service_name: "user-service"
  port: 3001
  
  # Built-in references
  cookbook_path: "{{cookbook.root}}"
  
  # Computed values
  service_url: "http://localhost:{{var.port}}"
  build_command: "npm run build:{{var.build_env}}"
```

#### Environment Variables

**`environment`** (optional, array of strings)
- Environment variables accessible to all recipes
- Must be explicitly listed for security

```yaml
environment:
  - NODE_ENV
  - DATABASE_URL
  - API_KEY
  - DEBUG
```

#### Recipe Definitions

**`recipes`** (required, object)
- Map of recipe name to recipe configuration
- Each recipe represents an executable task

### Recipe Schema

#### Required Fields

**`run`** (required, string) OR **`template`** (required, string)
- Shell command to execute OR template to instantiate
- Multi-line strings supported for complex scripts
- Cannot specify both `run` and `template`

```yaml
recipes:
  simple:
    run: "npm build"
    
  complex:
    run: |
      echo "Starting build..."
      npm ci
      npm run build
      echo "Build complete"
      
  templated:
    template: "build-template"
    params:
      language: "node"
```

#### Optional Fields

**`description`** (optional, string)
- Human-readable recipe description
- Used in help output and documentation

**`cache.inputs`** (optional, array of strings)
- Glob patterns for input files
- Relative to cookbook directory
- Affects caching behavior

```yaml
cache:
  inputs:
    # File types
    - "src/**/*.{ts,tsx,js,jsx}"
    - "test/**/*.{ts,js}"
    
    # Configuration files
    - "package.json"
    - "tsconfig.json"
    - ".env"
    
    # Relative paths
    - "../shared/dist/**/*"
    
    # Variables in patterns
    - "{{var.source_dir}}/**/*"
    
    # Exclusion patterns
    - "src/**/*"
    - "!src/**/*.test.ts"
```

**`cache.outputs`** (optional, array of strings)
- Glob patterns for output files
- Used for caching and validation
- Relative to cookbook directory

```yaml
cache:
  outputs:
    - "dist/**/*"
    - "build/**/*" 
    - "{{var.output_dir}}/**/*"
    - "coverage/lcov.info"
```

**`dependencies`** (optional, array of strings)
- Recipes that must complete before this recipe
- Same cookbook: use recipe name only
- Other cookbooks: use `cookbook:recipe` format

```yaml
dependencies:
  # Same cookbook
  - install
  - compile
  
  # Other cookbooks  
  - shared:build
  - backend:migrate
  
  # Multiple dependencies
  - [install, shared:build]
```

**`environment`** (optional, array of strings)
- Environment variables for this specific recipe
- Must also be listed in cookbook environment

```yaml
environment:
  - NODE_ENV
  - BUILD_TARGET
  - API_URL
```

**`variables`** (optional, object)
- Recipe-specific variable definitions
- Override cookbook and project variables

```yaml
variables:
  # Recipe-specific values
  build_mode: "production"
  output_path: "{{var.dist_dir}}/bundle"
  
  # Override parent variables
  environment: "staging"
```

**`template`** (optional, string)
- Name of recipe template to use
- Cannot be combined with `run` field
- Requires `.bake/templates/[template].yml` file

**`params`** (optional, object)
- Parameters for template instantiation
- Only used with `template` field
- Parameter validation defined in template

```yaml
template: "service-template" 
params:
  service_name: "user-api"
  port: 3001
  database_required: true
```

## Recipe Template Schema (`.bake/templates/*.yml`)

Reusable recipe templates with typed parameters.

### Schema Overview

```yaml
# Required template metadata
name: string                    # Template name
description: string             # Template description

# Optional template inheritance
extends: string                 # Parent template name

# Optional parameter definitions
parameters:
  param_name:                   # Parameter name
    type: string               # Parameter type
    required: boolean          # Required flag
    default: any               # Default value
    description: string        # Parameter description
    # Type-specific validation
    pattern: string            # Regex pattern (string)
    min: number               # Minimum value (number)
    max: number               # Maximum value (number)
    items:                    # Array item schema (array)
      type: string

# Required template definition
template:
  # Recipe properties (same as regular recipe)
  description: string
  run: string
  cache:
    inputs: [string]
    outputs: [string]
  dependencies: [string]
  environment: [string]
  variables:
    key: value
```

### Parameter Types

#### String Parameters

```yaml
parameters:
  service_name:
    type: string
    required: true
    pattern: "^[a-z][a-z0-9-]+$"        # Regex validation
    description: "Service name"
    
  api_url:
    type: string  
    default: "http://localhost:3000"     # Optional default
    description: "API base URL"
```

#### Number Parameters

```yaml
parameters:
  port:
    type: number
    required: true
    min: 1024                           # Minimum value
    max: 65535                          # Maximum value
    default: 3000
    description: "Service port"
    
  timeout:
    type: number
    default: 30
    min: 1
    description: "Timeout in seconds"
```

#### Boolean Parameters

```yaml
parameters:
  debug:
    type: boolean
    default: false
    description: "Enable debug mode"
    
  ssl_enabled:
    type: boolean
    required: true
    description: "Enable SSL/TLS"
```

#### Array Parameters

```yaml
parameters:
  build_targets:
    type: array
    default: ["web"]
    items:
      type: string                      # Array item type
    description: "List of build targets"
    
  ports:
    type: array
    items:
      type: number
      min: 1000
      max: 9999
    description: "List of service ports"
```

#### Object Parameters

```yaml
parameters:
  database:
    type: object
    default:
      host: "localhost"
      port: 5432
    description: "Database configuration"
    
  service_config:
    type: object
    required: true
    description: "Service configuration object"
```

### Template Definition

The `template` section defines the recipe structure using parameters:

```yaml
template:
  description: "{{params.service_name}} service"
  dependencies: ["install"]
  environment: 
    - NODE_ENV
    - PORT
  variables:
    PORT: "{{params.port}}"
    DEBUG: "{{params.debug}}"
  run: |
    echo "Starting {{params.service_name}} on port {{params.port}}"
    {{#if params.debug}}
    export DEBUG=1
    echo "Debug mode enabled"
    {{/if}}
    npm start
```

### Template Inheritance

Templates can extend other templates:

```yaml
# base-service.yml
name: "base-service"
parameters:
  service_name:
    type: string
    required: true
template:
  description: "{{params.service_name}} base service"
  dependencies: ["install"]
  
# web-service.yml  
name: "web-service"
extends: "base-service"            # Inherit from base-service
parameters:
  port:                           # Add additional parameters
    type: number
    default: 3000
template:
  environment: ["PORT"]           # Extend template definition
  variables:
    PORT: "{{params.port}}"
  run: |
    echo "Starting web service {{params.service_name}}"
    npm start
```

## Built-in Variables

### Project Constants

- **`{{project.root}}`** - Absolute path to project root directory
- **`{{project.name}}`** - Project name from configuration

### Cookbook Constants

- **`{{cookbook.root}}`** - Absolute path to cookbook directory  
- **`{{cookbook.name}}`** - Name of current cookbook

### Recipe Constants

- **`{{recipe.name}}`** - Name of current recipe
- **`{{recipe.cookbook}}`** - Name of cookbook containing recipe

### Variable References

- **`{{var.name}}`** - User-defined variables
- **`{{env.NAME}}`** - Environment variables
- **`{{params.name}}`** - Template parameters (templates only)

## Validation Rules

### File Structure

```
project/
├── bake.yml                    # Required project config
├── cookbook1/
│   └── cookbook.yml            # Required cookbook config
└── .bake/
    └── templates/
        └── template.yml        # Optional templates
```

### Required Fields

- **Project**: `cookbooks` field required
- **Cookbook**: `name` and `recipes` fields required  
- **Recipe**: Either `run` or `template` field required
- **Template**: `name` and `template` fields required

### Naming Conventions

- Cookbook names: alphanumeric, hyphens, underscores
- Recipe names: alphanumeric, hyphens, underscores
- Template names: alphanumeric, hyphens, underscores
- Variable names: alphanumeric, underscores

### Cross-References

- Cookbook names must match directory names
- Dependencies must reference existing recipes
- Template references must exist in `.bake/templates/`
- Variables must be defined before use

## JSON Schema Support

Bake provides JSON schemas for IDE validation and autocompletion:

- **Project**: `https://schemas.bake.sh/bake-project.schema.json`
- **Cookbook**: `https://schemas.bake.sh/cookbook.schema.json`  
- **Template**: `https://schemas.bake.sh/recipe-template.schema.json`

See [Schema Documentation](../development/schemas.md) for IDE setup instructions.

## Related Documentation

- [Configuration Guide](../guides/configuration.md) - Configuration usage patterns
- [Variables Guide](../guides/variables.md) - Variable system details
- [Recipe Templates](../guides/recipe-templates.md) - Template system usage
- [Schema Documentation](../development/schemas.md) - JSON schema setup