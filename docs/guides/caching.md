# Caching Guide

Bake's intelligent caching system dramatically improves build performance by avoiding unnecessary work. It supports both local and remote caching with content-based invalidation.

## How Caching Works

Bake creates cache keys based on:
1. **Recipe inputs** - Files and their content hashes
2. **Recipe dependencies** - Results from dependent recipes  
3. **Run command** - The exact command being executed
4. **Environment variables** - Values of specified environment variables

If none of these change, the cached result is used instead of re-executing the recipe.

## Cache Configuration

### Project-Level Cache Settings

Configure caching in your `bake.yml`:

```yaml
# bake.yml
config:
  cache:
    # Local filesystem cache
    local:
      enabled: true
      path: .bake/cache         # Cache directory (default)
      
    # Remote cache providers
    remotes:
      # AWS S3 cache
      s3:
        bucket: my-bake-cache
        region: us-west-2
        prefix: "project/{{var.environment}}"  # Optional key prefix
        
      # Google Cloud Storage cache  
      gcs:
        bucket: my-bake-cache
        prefix: "builds/{{var.version}}"
        
    # Cache strategy order (first hit wins)
    order: ["local", "s3", "gcs"]
```

### Recipe-Level Cache Configuration

Define what should be cached for each recipe:

```yaml
# cookbook.yml
recipes:
  build:
    description: "Build the application"
    
    # Cache configuration
    cache:
      # Cache inputs - files that affect the output
      inputs:
        - "src/**/*.ts"           # All TypeScript source files
        - "src/**/*.css"          # All CSS files  
        - "package.json"          # Dependencies
        - "tsconfig.json"         # Build configuration
        - "webpack.config.js"     # Build tool config
        
      # Cache outputs - files produced by the recipe
      outputs:
        - "dist/**/*"             # All built files
        - "build-stats.json"      # Build metadata
      
    # Environment variables that affect output
    environment:
      - NODE_ENV
      - BUILD_TARGET
      - API_URL
      
    run: |
      npm run build
```

## Cache Input Patterns

### File Patterns

Use glob patterns to specify which files affect your recipe:

```yaml
recipes:
  test:
    cache:
      inputs:
        # Include specific file types
        - "src/**/*.{js,ts,jsx,tsx}"
        - "test/**/*.{js,ts}"
        
        # Include configuration files
        - "package.json"
        - "jest.config.js"  
        - "tsconfig.json"
        
        # Include environment files
        - ".env"
        - ".env.local"
        
        # Exclude patterns (using glob negation)
        - "src/**/*"
        - "!src/**/*.test.ts"     # Exclude test files
        - "!src/**/*.spec.ts"     # Exclude spec files
```

### Relative Paths

Input patterns are relative to the cookbook directory:

```yaml
# In frontend/cookbook.yml
recipes:
  build:
    cache:
      inputs:
        - "src/**/*"              # frontend/src/**/*
        - "../shared/dist/**/*"   # shared/dist/**/* (sibling cookbook)
        - "{{project.root}}/config/**/*"  # Absolute project paths
```

### Dynamic Inputs

Use variables in input patterns:

```yaml
variables:
  source_dirs: ["src", "lib", "components"]
  config_file: "config-{{var.environment}}.json"

recipes:
  build:
    cache:
      inputs:
        # Variable-based patterns
        - "{{var.config_file}}"
        
        # Loop over directories
        - "{{#each var.source_dirs}}{{this}}/**/*.ts {{/each}}"
        
        # Conditional includes
        - "{{#if var.include_tests}}test/**/*.ts{{/if}}"
```

## Cache Outputs

### Output Specifications

Define what files your recipe produces:

```yaml
recipes:
  build:
    cache:
      outputs:
        # Build artifacts
        - "dist/**/*"
        - "build/**/*"
        
        # Generated documentation
        - "docs/api/**/*.html"
        
        # Metadata files
        - "build-manifest.json"
        - "bundle-stats.json"
      
    run: |
      npm run build
      npm run docs:generate
```

### Output Validation

Bake can validate that expected outputs are produced:

```yaml
recipes:
  compile:
    cache:
      inputs:
        - "src/**/*.rs"
      outputs:
        - "target/release/myapp"    # Must exist after build
        - "target/release/*.so"     # Shared libraries
    run: |
      cargo build --release
```

## Local Caching

### Configuration

```yaml
config:
  cache:
    local:
      enabled: true
      path: .bake/cache           # Default cache directory
      max_size: "10GB"           # Optional size limit
      retention_days: 30         # Optional cleanup policy
```

### Cache Directory Structure

```
.bake/cache/
├── recipes/
│   ├── frontend:build/
│   │   ├── abc123.tar.zst     # Cached outputs
│   │   └── def456.tar.zst
│   └── backend:test/
├── metadata/
│   ├── frontend:build.json    # Cache metadata
│   └── backend:test.json  
└── logs/
    └── cache.log              # Cache operations log
```

### Manual Cache Management

```bash
# Clear all caches
rm -rf .bake/cache

# Clear specific recipe cache
rm -rf .bake/cache/recipes/frontend:build

# View cache sizes
du -sh .bake/cache/recipes/*

# List cached recipes
find .bake/cache/recipes -name "*.tar.zst" | head -10
```

## Remote Caching

### AWS S3 Cache

Configure S3 caching:

```yaml
config:
  cache:
    remotes:
      s3:
        bucket: my-build-cache
        region: us-west-2
        prefix: "{{var.project}}/{{var.branch}}"
        
        # Optional: Custom endpoint (for S3-compatible services)
        endpoint: "https://s3.example.com"
        
        # Optional: Server-side encryption
        encryption: AES256
```

**Authentication**: Uses AWS credentials from:
- Environment variables (`AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`)
- AWS credentials file (`~/.aws/credentials`)
- IAM roles (on EC2)
- AWS SSO

**Required S3 permissions**:
```json
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Action": [
        "s3:GetObject",
        "s3:PutObject", 
        "s3:DeleteObject"
      ],
      "Resource": "arn:aws:s3:::my-build-cache/*"
    },
    {
      "Effect": "Allow",
      "Action": "s3:ListBucket",
      "Resource": "arn:aws:s3:::my-build-cache"
    }
  ]
}
```

### Google Cloud Storage Cache

Configure GCS caching:

```yaml
config:
  cache:
    remotes:
      gcs:
        bucket: my-build-cache
        prefix: "{{var.team}}/{{var.environment}}"
        
        # Optional: Custom endpoint
        endpoint: "https://storage.googleapis.com"
```

**Authentication**: Uses Google Cloud credentials from:
- Environment variable (`GOOGLE_APPLICATION_CREDENTIALS`)
- Service account key file
- Default application credentials (gcloud auth)
- Workload Identity (on GKE)

**Required GCS permissions**:
```yaml
# IAM role bindings
bindings:
- members:
  - serviceAccount:build-cache@project.iam.gserviceaccount.com
  role: roles/storage.objectAdmin
```

### Cache Strategy Order

Control cache lookup order:

```yaml
config:
  cache:
    # Check local first, then S3, then GCS
    order: ["local", "s3", "gcs"]
    
    # Local stores successful cache retrievals from remotes
    # Subsequent runs will hit local cache first
```

## Cache Performance Optimization

### Minimize Input Scope

Be specific about inputs to improve cache hit rates:

```yaml
# Too broad - rebuilds on any file change
cache:
  inputs:
    - "**/*"
  
# Better - only relevant files
cache:
  inputs:
    - "src/**/*.{ts,js,json}"
    - "package.json"
    - "tsconfig.json"

# Best - exclude irrelevant files  
cache:
  inputs:
    - "src/**/*.{ts,js,json}"
    - "!src/**/*.test.ts"       # Exclude tests
    - "!src/**/*.spec.ts"       # Exclude specs
    - "package.json"
    - "tsconfig.json"
```

### Parallelize Cache Operations

Enable parallel cache uploads/downloads:

```yaml
config:
  max_parallel: 8              # Allow parallel recipe execution
  cache:
    parallel_operations: 4     # Parallel cache transfers
```

### Use Cache Hierarchies

Organize caches for better reuse:

```yaml
variables:
  cache_prefix: "{{var.project}}/{{var.git_branch}}/{{var.git_commit}}"
  
config:  
  cache:
    remotes:
      s3:
        bucket: shared-build-cache
        prefix: "{{var.cache_prefix}}"
        
      # Fallback to branch-level cache
      s3_branch:
        bucket: shared-build-cache  
        prefix: "{{var.project}}/{{var.git_branch}}/latest"
```

## Cache Debugging

### Cache Hit Analysis

See cache behavior for recipes:

```bash
# Show cache status during build
bake --verbose

# Show detailed cache information
bake --debug

# Check specific recipe cache
bake frontend:build --dry-run --verbose
```

### Common Cache Issues

**Cache never hits**:
- Check that input patterns are correct
- Verify file paths are relative to cookbook root
- Ensure no environment variables are changing unexpectedly

**Cache hits but outputs are wrong**:
- Verify output patterns include all generated files
- Check for non-deterministic build processes
- Ensure no absolute paths in outputs

**Remote cache failures**:
- Check authentication credentials
- Verify bucket permissions
- Test network connectivity to cache provider

## Environment Variables and Caching

### Including Environment Variables

Specify which environment variables affect recipe output:

```yaml
recipes:
  build:
    environment:
      - NODE_ENV              # Different output for dev/prod
      - API_URL               # Affects bundled config
      - FEATURE_FLAGS         # Conditional compilation
      - BUILD_NUMBER          # Build metadata
      
    cache:
      inputs:
        - "src/**/*.ts"
      
    run: |
      echo "Building for NODE_ENV=$NODE_ENV"
      npm run build
```

### Variable-Based Cache Keys

Use variables to create cache segments:

```yaml
variables:
  cache_key: "{{var.environment}}-{{var.version}}-{{env.BUILD_ID}}"
  
recipes:
  deploy:
    # Cache key includes environment and version
    variables:
      deploy_env: "{{var.environment}}"
      app_version: "{{var.version}}"
      
    run: |
      echo "Cache key: {{var.cache_key}}"
      ./deploy.sh
```

## CI/CD Integration

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
        
      - name: Configure cache
        run: |
          # Use commit SHA for unique cache keys
          echo "BUILD_ID=${GITHUB_SHA}" >> $GITHUB_ENV
          echo "BRANCH_NAME=${GITHUB_REF_NAME}" >> $GITHUB_ENV
          
      - name: Build with cache
        env:
          AWS_ACCESS_KEY_ID: ${{ secrets.AWS_ACCESS_KEY_ID }}
          AWS_SECRET_ACCESS_KEY: ${{ secrets.AWS_SECRET_ACCESS_KEY }}
        run: |
          bake --var environment=ci --var branch=$BRANCH_NAME
```

### GitLab CI

```yaml
# .gitlab-ci.yml
build:
  stage: build
  variables:
    BUILD_ID: ${CI_COMMIT_SHA}
    BRANCH_NAME: ${CI_COMMIT_REF_NAME}
  before_script:
    - echo "Configuring cache for ${BRANCH_NAME}:${BUILD_ID}"
  script:
    - bake --var environment=ci --var commit=${BUILD_ID}
  cache:
    key: "${CI_COMMIT_REF_NAME}"
    paths:
      - .bake/cache/
```

## Best Practices

### 1. Start with Local Caching

Begin with local caching, then add remote caches:

```yaml
# Start simple
config:
  cache:
    local:
      enabled: true
      
# Add remote caching later
config:
  cache:
    local:
      enabled: true
    remotes:
      s3:
        bucket: team-build-cache
    order: ["local", "s3"]
```

### 2. Use Descriptive Cache Keys

Include meaningful information in cache prefixes:

```yaml
config:
  cache:
    remotes:
      s3:
        # Good: includes project, environment, version
        prefix: "myapp/{{var.environment}}/{{var.version}}"
        
        # Better: includes git information
        prefix: "{{var.project}}/{{var.branch}}/{{env.GIT_SHA}}"
```

### 3. Monitor Cache Performance

Track cache hit rates and sizes:

```bash
# Check cache effectiveness
bake --stats

# Monitor cache sizes
du -sh .bake/cache/*

# Track remote cache usage
aws s3 ls s3://my-build-cache/ --recursive --human-readable
```

### 4. Clean Up Old Caches

Implement cache cleanup policies:

```yaml
config:
  cache:
    local:
      retention_days: 7        # Keep caches for 7 days
      max_size: "5GB"         # Limit cache size
```

### 5. Separate Fast and Slow Operations

Cache expensive operations separately:

```yaml
recipes:
  # Fast operations - smaller cache
  lint:
    inputs: ["src/**/*.ts", ".eslintrc.js"]  
    outputs: ["lint-results.json"]
    
  # Slow operations - comprehensive cache
  build:
    inputs: ["src/**/*", "package.json", "webpack.config.js"]
    outputs: ["dist/**/*", "build-stats.json"]
    dependencies: [lint]
```

## Security Considerations

### Cache Isolation

Isolate caches between environments:

```yaml
config:
  cache:
    remotes:
      s3:
        # Separate buckets or prefixes per environment
        bucket: "builds-{{var.environment}}"
        # Or use prefixes: prefix: "{{var.environment}}/{{var.team}}"
```

### Sensitive Data

Avoid caching sensitive information:

```yaml
recipes:
  deploy:
    cache:
      inputs:
        - "deploy/**/*.yml"
        # Don't include secrets or credentials
        - "!deploy/secrets/**/*"
        - "!**/*.key"
        - "!**/*.pem"
    run: |
      # Secrets passed via environment, not cached files
      ./deploy.sh
```

## Related Documentation

- [Configuration Guide](configuration.md) - Complete configuration reference
- [Variables Guide](variables.md) - Using variables in cache configuration
- [CLI Commands](../reference/cli-commands.md) - Cache management commands
- [First Project Tutorial](../getting-started/first-project.md) - Caching in practice