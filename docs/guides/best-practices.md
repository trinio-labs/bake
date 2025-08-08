# Best Practices

This guide covers proven patterns and best practices for organizing, structuring, and maintaining Bake projects effectively.

## Project Organization

### Directory Structure

Organize your project with clear separation of concerns:

```
my-project/
├── bake.yml                    # Project configuration
├── shared/                     # Shared utilities
│   ├── cookbook.yml
│   └── src/
├── services/
│   ├── api/                    # Backend API
│   │   ├── cookbook.yml
│   │   └── src/
│   └── workers/                # Background workers  
│       ├── cookbook.yml
│       └── src/
├── apps/
│   ├── web/                    # Frontend web app
│   │   ├── cookbook.yml
│   │   └── src/
│   └── mobile/                 # Mobile app
│       ├── cookbook.yml
│       └── src/
├── deployment/                 # Infrastructure & deployment
│   ├── cookbook.yml
│   └── scripts/
└── .bake/
    ├── cache/                  # Local cache
    └── templates/              # Recipe templates
        ├── service-template.yml
        └── app-template.yml
```

### Cookbook Naming

Use descriptive, consistent names:

```yaml
# Good - Clear purpose and scope
cookbooks:
  - shared-utils
  - user-service  
  - payment-api
  - admin-dashboard
  - mobile-app
  - infrastructure

# Avoid - Vague or inconsistent
cookbooks:
  - shared
  - api
  - frontend  
  - stuff
  - misc
```

### Recipe Organization

Group related recipes logically:

```yaml
# cookbook.yml
recipes:
  # Setup and dependencies
  install:
    description: "Install project dependencies"
    
  # Development lifecycle  
  build:
    description: "Build application for target environment"
  test:
    description: "Run unit tests"
  lint:
    description: "Check code quality"
    
  # Deployment lifecycle
  package:
    description: "Create deployment package"  
  deploy-staging:
    description: "Deploy to staging environment"
  deploy-production:
    description: "Deploy to production environment"
    
  # Utilities
  clean:
    description: "Clean build artifacts"
  docs:
    description: "Generate documentation"
```

## Recipe Design

### Single Responsibility

Each recipe should have one clear purpose:

```yaml
# Good - Single, focused responsibility
recipes:
  compile:
    description: "Compile TypeScript to JavaScript"
    run: tsc
    
  bundle:
    description: "Bundle JavaScript modules"  
    dependencies: [compile]
    run: webpack --mode production
    
  optimize:
    description: "Optimize bundle size"
    dependencies: [bundle]
    run: webpack-bundle-analyzer dist/

# Avoid - Multiple responsibilities in one recipe
recipes:
  build:
    description: "Compile, bundle, optimize, and test"
    run: |
      tsc
      webpack --mode production  
      webpack-bundle-analyzer dist/
      npm test
```

### Meaningful Dependencies

Express logical dependencies clearly:

```yaml
recipes:
  # Logical dependency chain
  test:
    dependencies: [compile]      # Tests need compiled code
    
  package: 
    dependencies: [build, test]  # Package after build and test
    
  deploy:
    dependencies: [package]      # Deploy the package
    
  # Parallel execution where possible
  lint:
    # No dependencies - can run in parallel
    
  type-check:
    # No dependencies - can run in parallel
```

### Descriptive Names and Documentation

Use clear, descriptive recipe names:

```yaml
# Good - Self-explanatory names
recipes:
  install-dependencies:
    description: "Install npm dependencies using package-lock.json"
    
  build-production:
    description: "Build optimized production bundle with minification"
    
  run-unit-tests:
    description: "Execute Jest unit tests with coverage reporting"
    
  deploy-to-staging:
    description: "Deploy application to staging environment with smoke tests"

# Avoid - Vague or abbreviated names
recipes:
  install:
    description: "Install stuff"
    
  build:
    description: "Build"
    
  test:
    description: "Test"
```

## Variable Management

### Hierarchical Organization

Use the variable hierarchy effectively:

```yaml
# bake.yml - Global defaults
variables:
  node_version: "18"
  environment: development
  version: "1.0.0"
  
# cookbook.yml - Cookbook-specific
variables:
  service_name: "user-service"
  port: 3001
  build_env: "{{var.environment}}"
  
# recipes - Recipe-specific overrides
recipes:
  deploy-production:
    variables:
      environment: production  # Override for this recipe
      replicas: 3
```

### Environment-Specific Configuration

Structure environment variables clearly:

```yaml
# bake.yml
variables:
  # Common defaults
  environment: development
  debug: true
  log_level: info
  
  # Environment-specific URLs
  api_url: |
    {{#if (eq var.environment "production")}}
    https://api.prod.example.com
    {{else if (eq var.environment "staging")}}  
    https://api-staging.example.com
    {{else}}
    http://localhost:3001
    {{/if}}

overrides:
  production:
    environment: production
    debug: false
    log_level: warn
    replicas: 5
    
  staging:
    environment: staging  
    debug: false
    log_level: info
    replicas: 2
```

### Variable Naming Conventions

Use consistent naming patterns:

```yaml
variables:
  # Use snake_case for consistency
  service_name: "my-service"
  api_base_url: "https://api.example.com"
  database_connection_timeout: 30
  
  # Group related variables with prefixes
  docker_image: "myapp/service:{{var.version}}"
  docker_registry: "docker.io"
  docker_tag: "{{var.version}}-{{var.environment}}"
  
  # Boolean flags with clear naming
  enable_caching: true
  enable_debug_mode: false
  enable_feature_x: "{{env.FEATURE_X_ENABLED}}"
```

## Caching Strategy

### Precise Input Specifications

Be specific about inputs to maximize cache hits:

```yaml
# Good - Specific inputs
recipes:
  build:
    cache:
      inputs:
        - "src/**/*.{ts,tsx,js,jsx}"      # Source files
        - "package.json"                   # Dependencies
        - "tsconfig.json"                  # Build config
        - "webpack.config.js"              # Build tool config
        - "!src/**/*.{test,spec}.*"        # Exclude tests

# Avoid - Too broad  
recipes:
  build:
    cache:
      inputs:
        - "**/*"        # Invalidates cache on any file change
```

### Layered Caching

Structure caches hierarchically:

```yaml
recipes:
  # Fast cache - just dependency info
  install:
    inputs: ["package.json", "package-lock.json"]
    outputs: ["node_modules/**/*"]
    
  # Medium cache - compiled code
  compile:
    inputs: ["src/**/*.ts", "tsconfig.json"] 
    outputs: ["lib/**/*"]
    dependencies: [install]
    
  # Slower cache - bundled assets
  bundle:
    inputs: ["lib/**/*", "webpack.config.js"]
    outputs: ["dist/**/*"]  
    dependencies: [compile]
```

### Environment-Aware Caching

Separate caches by environment when outputs differ:

```yaml
variables:
  cache_key: "{{var.environment}}-{{var.node_version}}"
  
recipes:
  build:
    cache:
      inputs:
        - "src/**/*"
        - "package.json"
      outputs:
        - "dist-{{var.environment}}/**/*"
    environment:
      - NODE_ENV         # Affects build output
      - TARGET_ENV       # Affects bundling
    variables:
      NODE_ENV: "{{var.environment}}"
```

## Template Usage

### Create Reusable Patterns

Extract common recipe patterns into templates:

```yaml
# .bake/templates/node-service.yml
name: "node-service"
description: "Standard Node.js service template"

parameters:
  service_name:
    type: string
    required: true
  port:
    type: number  
    default: 3000
  database_required:
    type: boolean
    default: false

template:
  description: "{{params.service_name}} service on port {{params.port}}"
  dependencies: ["install"]
  environment:
    - NODE_ENV
    - PORT
    {{#if params.database_required}}- DATABASE_URL{{/if}}
  variables:
    PORT: "{{params.port}}"
  run: |
    echo "Starting {{params.service_name}}"
    {{#if params.database_required}}
    echo "Waiting for database..."
    ./wait-for-db.sh
    {{/if}}
    npm start
```

Use consistently across services:

```yaml
# user-service/cookbook.yml
recipes:
  start:
    template: node-service
    params:
      service_name: "User Service"
      port: 3001
      database_required: true
      
# notification-service/cookbook.yml  
recipes:
  start:
    template: node-service
    params:
      service_name: "Notification Service"  
      port: 3002
      database_required: false
```

### Template Inheritance

Use inheritance for specialization:

```yaml
# .bake/templates/base-service.yml
name: "base-service"
parameters:
  service_name:
    type: string
    required: true
    
template:
  description: "{{params.service_name}} service"
  dependencies: ["install"]
  
# .bake/templates/web-service.yml  
name: "web-service"
extends: "base-service"
parameters:
  port:
    type: number
    default: 3000
    
template:
  environment: ["PORT"]
  variables:
    PORT: "{{params.port}}"
  run: |
    echo "Starting web service {{params.service_name}} on port {{params.port}}"
    npm start
```

## Performance Optimization

### Parallel Execution

Structure dependencies to maximize parallelism:

```yaml
# Good - Allows parallel execution
recipes:
  # These can run in parallel
  lint:
    inputs: ["src/**/*.ts"]
    run: eslint src/
    
  type-check:
    inputs: ["src/**/*.ts", "tsconfig.json"]
    run: tsc --noEmit
    
  unit-test:
    inputs: ["src/**/*.ts", "test/**/*.ts"]  
    run: jest
    
  # This depends on all parallel tasks
  build:
    dependencies: [lint, type-check, unit-test]
    run: webpack --mode production

# Avoid - Unnecessary serialization
recipes:
  lint:
    run: eslint src/
    
  type-check:  
    dependencies: [lint]        # Unnecessary dependency
    run: tsc --noEmit
    
  unit-test:
    dependencies: [type-check]  # Unnecessary dependency  
    run: jest
```

### Resource Management

Configure resource limits appropriately:

```yaml
# bake.yml
config:
  # Tune for your system and workload
  max_parallel: 6              # Balance CPU usage vs speed
  
  cache:
    local:
      enabled: true
      max_size: "5GB"          # Limit cache disk usage
      retention_days: 7        # Automatic cleanup
      
    remotes:
      s3:
        parallel_operations: 3  # Limit concurrent uploads
```

## Error Handling

### Fail Fast vs Resilient

Choose the right failure strategy:

```yaml
# Development - fail fast for quick feedback
config:
  fast_fail: true
  
overrides:
  ci:
    # CI - run all tests even if some fail  
    fast_fail: false
```

### Meaningful Error Messages

Provide helpful error context:

```yaml
recipes:
  deploy:
    run: |
      # Check prerequisites
      if [ -z "$DEPLOY_KEY" ]; then
        echo "ERROR: DEPLOY_KEY environment variable is required"
        echo "Set it with: export DEPLOY_KEY=your-key"
        exit 1
      fi
      
      if [ ! -f "dist/index.html" ]; then
        echo "ERROR: Built application not found at dist/index.html"
        echo "Run 'bake build' first"
        exit 1  
      fi
      
      echo "Deploying to {{var.environment}}..."
      ./deploy.sh
```

### Graceful Degradation

Handle optional dependencies gracefully:

```yaml
recipes:
  build:
    run: |
      # Required build step
      npm run build
      
      # Optional optimization (don't fail if missing)
      if command -v webpack-bundle-analyzer >/dev/null 2>&1; then
        echo "Analyzing bundle size..."
        webpack-bundle-analyzer dist/ --report bundle-report.html
      else
        echo "webpack-bundle-analyzer not found, skipping analysis"
      fi
```

## Security Considerations

### Secrets Management

Never commit secrets to configuration:

```yaml
# Good - Reference environment variables
variables:
  database_url: "{{env.DATABASE_URL}}"
  api_key: "{{env.API_KEY}}"
  
recipes:
  deploy:
    environment:
      - DATABASE_URL
      - API_KEY  
      - DEPLOY_TOKEN
    run: ./deploy.sh

# Avoid - Hard-coded secrets
variables:
  database_url: "postgresql://user:password@host/db"  # Don't do this
  api_key: "sk-1234567890abcdef"                     # Don't do this
```

### Input Validation

Validate external inputs:

```yaml
recipes:
  deploy:
    run: |
      # Validate environment
      case "{{var.environment}}" in
        development|staging|production)
          echo "Deploying to {{var.environment}}"
          ;;
        *)
          echo "ERROR: Invalid environment: {{var.environment}}"
          echo "Must be one of: development, staging, production"
          exit 1
          ;;
      esac
      
      # Validate version format
      if ! echo "{{var.version}}" | grep -E '^[0-9]+\.[0-9]+\.[0-9]+$'; then
        echo "ERROR: Invalid version format: {{var.version}}"
        echo "Must be semantic version (e.g., 1.2.3)"
        exit 1
      fi
      
      ./deploy.sh
```

### File Permissions

Be explicit about file permissions:

```yaml
recipes:
  setup-ssl:
    run: |
      # Create certificates with restricted permissions
      openssl req -x509 -newkey rsa:4096 -keyout key.pem -out cert.pem
      chmod 600 key.pem          # Private key - owner read/write only  
      chmod 644 cert.pem         # Certificate - owner read/write, others read
```

## Monitoring and Debugging

### Verbose Logging

Provide helpful output for debugging:

```yaml
recipes:
  build:
    run: |
      echo "=== Build Configuration ==="
      echo "Environment: {{var.environment}}"
      echo "Version: {{var.version}}"
      echo "Node version: $(node --version)"
      echo "Build mode: {{var.build_mode}}"
      echo ""
      
      echo "=== Starting Build ==="
      npm run build:{{var.build_mode}}
      
      echo "=== Build Complete ==="
      ls -la dist/
      echo "Bundle size: $(du -sh dist/)"
```

### Health Checks

Add validation to critical recipes:

```yaml
recipes:
  deploy:
    run: |
      ./deploy.sh
      
      # Validate deployment
      echo "Waiting for deployment to be ready..."
      for i in {1..30}; do
        if curl -f "{{var.app_url}}/health" >/dev/null 2>&1; then
          echo "Deployment successful!"
          exit 0
        fi
        echo "Attempt $i/30: Service not ready yet..."
        sleep 10
      done
      
      echo "ERROR: Deployment failed health check"
      exit 1
```

## Testing Strategies

### Recipe Testing

Test recipes in isolation:

```yaml
recipes:
  test-build:
    description: "Test build process in isolated environment"  
    run: |
      # Create clean test environment
      rm -rf test-build/
      mkdir test-build/
      cp -r src/ test-build/
      cd test-build/
      
      # Run build
      npm ci --quiet
      npm run build
      
      # Verify outputs
      if [ ! -f "dist/index.html" ]; then
        echo "ERROR: Build did not create expected output"
        exit 1
      fi
      
      echo "Build test passed"
      cd ..
      rm -rf test-build/
```

### Integration Testing

Test recipe interactions:

```yaml
recipes:
  integration-test:
    dependencies: [build, start-test-db]
    run: |
      # Wait for dependencies to be ready
      ./wait-for-service.sh "{{var.api_url}}"
      ./wait-for-service.sh "{{var.db_url}}"
      
      # Run integration tests
      npm run test:integration
      
      # Cleanup
      bake stop-test-db
```

## Documentation Standards

### Recipe Documentation

Document purpose and usage:

```yaml
recipes:
  migrate-database:
    description: |
      Run database migrations to update schema to match current code.
      
      Prerequisites:
      - Database must be running and accessible
      - DATABASE_URL environment variable must be set
      
      Effects:  
      - Applies all pending migrations in migrations/
      - Updates schema_version table
      - Creates backup before destructive changes
      
    environment:
      - DATABASE_URL
      - BACKUP_ENABLED
    run: |
      if [ "${BACKUP_ENABLED:-true}" = "true" ]; then
        echo "Creating database backup..."
        ./backup-db.sh
      fi
      
      echo "Running migrations..."
      ./migrate.sh
```

### Variable Documentation

Document variable purposes and valid values:

```yaml
variables:
  # Build configuration
  node_version: "18"              # Node.js version (14, 16, 18, 20)
  build_mode: "production"        # Build optimization (development, production)
  
  # Feature flags  
  enable_analytics: true          # Send usage analytics to service
  enable_debug_panel: false       # Show debug UI in development
  
  # Environment-specific settings
  api_timeout: 30                 # API request timeout in seconds
  retry_attempts: 3               # Number of retry attempts for failed requests
```

This comprehensive approach to Bake project organization will help you build maintainable, efficient, and reliable build systems that scale with your project's complexity.

## Related Documentation

- [Configuration Guide](configuration.md) - Complete configuration reference
- [Variables Guide](variables.md) - Variable system best practices
- [Caching Guide](caching.md) - Cache optimization strategies
- [Recipe Templates](recipe-templates.md) - Template design patterns
- [Troubleshooting](troubleshooting.md) - Common issues and solutions