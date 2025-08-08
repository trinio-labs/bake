# Architecture Documentation

This document explains Bake's internal architecture, design decisions, and codebase organization for contributors and maintainers.

## Overview

Bake is a parallel task runner built in Rust with smart caching capabilities. The architecture is designed around these core principles:

- **Async-first design** using Tokio for concurrent recipe execution
- **Modular architecture** with clear separation of concerns
- **Pluggable caching system** supporting local and remote storage
- **Type-safe configuration** with comprehensive validation
- **Library + binary pattern** for better testability

## High-Level Architecture

```
┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
│   CLI (main.rs) │───▶│ Library (lib.rs)│───▶│ Baker (baker.rs)│
└─────────────────┘    └─────────────────┘    └─────────────────┘
                                │                       │
                                ▼                       ▼
                       ┌─────────────────┐    ┌─────────────────┐
                       │ Project Loading │    │Recipe Execution │
                       └─────────────────┘    └─────────────────┘
                                │                       │
                                ▼                       ▼
                       ┌─────────────────┐    ┌─────────────────┐
                       │Template System  │    │  Cache System   │
                       └─────────────────┘    └─────────────────┘
```

## Core Components

### 1. Binary Entry Point (`main.rs`)

**Purpose**: Minimal CLI executable that delegates to the library.

```rust
// Thin wrapper that calls the library
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    bake::run().await
}
```

**Design**: Kept intentionally minimal to enable library usage and integration testing.

### 2. Library Entry Point (`lib.rs`)

**Purpose**: Public API, CLI argument parsing, and command orchestration.

**Key Components**:
- `Args` struct with clap-based CLI parsing
- Command handlers (`list-templates`, `validate`, `render`, etc.)
- Main `run()` function that orchestrates the application
- Public API for external library usage

**Design Decisions**:
- Uses `clap` for robust CLI argument parsing
- Exposes public functions for integration testing
- Contains the main application logic flow

### 3. Execution Engine (`baker.rs`)

**Purpose**: Core recipe execution with parallelism and dependency resolution.

**Key Features**:
- Parallel recipe execution using Tokio and semaphores
- Dependency resolution with topological sorting
- Fast-fail and cancellation logic
- Progress reporting and output handling
- Recipe lifecycle management

**Architecture**:
```rust
pub struct Baker {
    project: Project,
    cache: Box<dyn Cache>,
    semaphore: Arc<Semaphore>,
    config: ExecutionConfig,
}
```

**Concurrency Model**:
- Uses `tokio::sync::Semaphore` to limit concurrent executions
- Spawns async tasks for independent recipe execution
- Implements graceful shutdown with cancellation tokens

### 4. Project System (`project/`)

The project system handles configuration loading, validation, and execution planning.

#### Project Module (`project/mod.rs`)

**Purpose**: Main project loading and execution planning coordination.

**Key Functions**:
- Project discovery and loading
- Cookbook enumeration and validation
- Execution plan generation
- Template resolution integration

#### Configuration (`project/config.rs`)

**Purpose**: Tool configuration management (parallelism, caching, updates).

```rust
pub struct Config {
    pub max_parallel: usize,
    pub fast_fail: bool,
    pub verbose: bool,
    pub clean_environment: bool,
    pub cache: CacheConfig,
    pub update: UpdateConfig,
}
```

#### Cookbook Management (`project/cookbook.rs`)

**Purpose**: Cookbook loading, validation, and recipe management.

**Key Features**:
- YAML parsing with `serde`
- Variable inheritance and scoping
- Recipe validation and normalization
- Cross-cookbook dependency resolution

#### Recipe Definitions (`project/recipe.rs`)

**Purpose**: Individual recipe definitions and execution context.

```rust
pub struct Recipe {
    pub name: String,
    pub description: Option<String>,
    pub run: Option<String>,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub dependencies: Vec<String>,
    pub environment: Vec<String>,
    pub variables: HashMap<String, Value>,
    pub template: Option<String>,
}
```

#### Dependency Graph (`project/graph.rs`)

**Purpose**: Recipe dependency graph using `petgraph`.

**Features**:
- Cycle detection for circular dependencies
- Topological sorting for execution order
- Parallel execution planning
- Dependency validation

#### Input/Output Fingerprinting (`project/hashing.rs`)

**Purpose**: Cache key generation based on inputs and dependencies.

**Algorithm**:
1. Hash all input file contents
2. Include dependency recipe hashes
3. Hash the run command
4. Include relevant environment variables
5. Generate composite cache key

#### Recipe Templates (`project/recipe_template.rs`)

**Purpose**: Reusable recipe definitions with typed parameters.

**Key Features**:
- Template discovery from `.bake/templates/`
- Parameter validation with types and constraints
- Template inheritance with `extends` support
- Handlebars integration for parameter substitution

## Template System (`template.rs`)

**Purpose**: Variable substitution system using Handlebars.

**Architecture**:
```rust
pub struct TemplateEngine {
    handlebars: Handlebars<'static>,
    variables: VariableContext,
}
```

**Features**:
- Hierarchical variable scoping (project → cookbook → recipe → CLI)
- Built-in constants (`{{project.root}}`, `{{cookbook.root}}`, etc.)
- Environment variable access (`{{env.VAR_NAME}}`)
- Handlebars helpers for control flow and formatting

**Variable Resolution Order**:
1. Project variables from `bake.yml`
2. Cookbook variables from `cookbook.yml`
3. Recipe variables from recipe definitions
4. Command-line overrides via `--var` flag

## Caching System (`cache/`)

Multi-tier caching system with pluggable storage backends.

### Cache Trait (`cache/mod.rs`)

**Design**: Plugin architecture using Rust traits.

```rust
#[async_trait]
pub trait Cache: Send + Sync {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>>;
    async fn put(&self, key: &str, data: &[u8]) -> Result<()>;
    async fn delete(&self, key: &str) -> Result<()>;
}
```

### Cache Builder (`cache/builder.rs`)

**Purpose**: Cache strategy composition and configuration.

**Features**:
- Multi-tier cache hierarchy
- Fallback strategies (local → S3 → GCS)
- Parallel cache operations
- Strategy priority configuration

### Local Cache (`cache/local.rs`)

**Purpose**: High-speed local filesystem cache.

**Implementation**:
- Uses `tar + zstd` compression for space efficiency
- Atomic operations with temporary files
- Configurable retention policies
- Safe concurrent access

### S3 Cache (`cache/s3.rs`)

**Purpose**: AWS S3 remote cache implementation.

**Features**:
- AWS SDK v2 integration
- Configurable regions and endpoints
- Server-side encryption support
- Credential chain authentication

### GCS Cache (`cache/gcs.rs`)

**Purpose**: Google Cloud Storage remote cache implementation.

**Features**:
- Google Cloud SDK integration
- Workload Identity Federation support
- Custom endpoint configuration
- Service account authentication

## Update System (`update.rs`)

**Purpose**: Self-update functionality with GitHub integration.

**Features**:
- GitHub releases API integration
- Binary replacement with backup
- Configurable update intervals
- Package manager detection (prevents conflicts)
- Prerelease version support

## Data Flow

### Recipe Execution Flow

```
1. CLI Input → Argument Parsing (lib.rs)
2. Project Loading → Configuration Validation (project/)  
3. Dependency Resolution → Execution Planning (project/graph.rs)
4. Template Resolution → Variable Substitution (template.rs)
5. Cache Lookup → Restore Outputs (cache/)
6. Recipe Execution → Parallel Tasks (baker.rs)
7. Cache Storage → Save Outputs (cache/)
8. Progress Reporting → User Feedback (baker.rs)
```

### Configuration Loading Flow

```
1. Project Discovery → Find bake.yml
2. YAML Parsing → Deserialize Configuration  
3. Cookbook Discovery → Find cookbook.yml files
4. Template Discovery → Load .bake/templates/
5. Variable Resolution → Apply Hierarchy
6. Validation → Check Consistency
7. Graph Building → Create Dependency Graph
```

## Error Handling Strategy

### Error Types

- **Configuration Errors** - Invalid YAML, missing files, validation failures
- **Execution Errors** - Recipe failures, command not found, permission issues
- **Cache Errors** - Network failures, authentication issues, disk space
- **System Errors** - I/O errors, resource exhaustion, signal handling

### Error Handling Patterns

```rust
// Use anyhow for application errors with context
use anyhow::{Context, Result};

fn load_config(path: &Path) -> Result<Config> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read config: {}", path.display()))?;
    
    serde_yaml::from_str(&content)
        .with_context(|| format!("Invalid YAML in: {}", path.display()))
}

// Use custom error types for domain-specific errors
#[derive(thiserror::Error, Debug)]
pub enum RecipeError {
    #[error("Recipe '{name}' not found in cookbook '{cookbook}'")]
    RecipeNotFound { name: String, cookbook: String },
    
    #[error("Circular dependency detected: {chain}")]
    CircularDependency { chain: String },
}
```

## Testing Architecture

### Test Organization

```
tests/                           # Integration tests (Rust convention)
├── baker_tests.rs              # Recipe execution tests
├── cache_tests.rs              # Cache strategy tests
├── project_tests.rs            # Project loading tests
├── s3_cache_tests.rs           # S3-specific tests
├── template_tests.rs           # Template system tests
└── common/                     # Test utilities
    └── mod.rs                  # TestProjectBuilder helper

src/                            # Unit tests (embedded)
├── lib.rs                      # #[cfg(test)] blocks
├── project/hashing.rs          # Unit tests for hashing
└── ...                         # Tests alongside implementation
```

### Test Helpers

**TestProjectBuilder**: Creates isolated test projects with configurable setups.

```rust
let project = TestProjectBuilder::new()
    .with_cookbook("frontend", |cb| {
        cb.with_recipe("build", "npm run build")
          .with_inputs(&["src/**/*.ts"])
          .with_outputs(&["dist/**/*"])
    })
    .build()
    .await;
```

## Performance Characteristics

### Parallelism

- **Recipe Execution**: Controlled by `max_parallel` setting and dependency graph
- **Cache Operations**: Parallel uploads/downloads to remote caches
- **File I/O**: Async operations using Tokio for non-blocking I/O

### Memory Usage

- **Configuration**: Loaded entirely into memory (typically < 1MB)
- **Cache Data**: Streamed to/from disk and network (not held in memory)
- **Recipe Output**: Captured and processed incrementally

### Disk Usage

- **Local Cache**: Configurable size limits with LRU eviction
- **Temporary Files**: Cleaned up after recipe execution
- **Log Files**: Rotated and size-limited

## Security Considerations

### Input Validation

- All configuration files are validated against schemas
- User input is sanitized before shell execution
- File paths are validated to prevent directory traversal

### Execution Security

- Recipes run in controlled shell environments
- Environment variable access is explicit and controlled
- File permissions are preserved and validated

### Cache Security

- Remote cache data is encrypted in transit
- Access credentials use secure storage mechanisms
- Cache keys are derived from content hashes (no secrets)

## Deployment and Distribution

### Release Process

1. **Version Bump** - Update `Cargo.toml` version
2. **Changelog** - Update `CHANGELOG.md` with changes
3. **Testing** - Run full test suite including integration tests
4. **Tagging** - Create Git tag with version
5. **Publishing** - Automatic publishing to crates.io via CI/CD
6. **Binaries** - Cross-platform binary builds via `cargo-dist`

### Distribution Channels

- **crates.io** - Primary Rust package registry
- **Homebrew** - macOS and Linux package manager
- **GitHub Releases** - Direct binary downloads
- **Docker** - Container images (if applicable)

## Monitoring and Observability

### Logging

- **Structured Logging** - Using `tracing` crate for structured logs
- **Log Levels** - Configurable verbosity (error, warn, info, debug, trace)
- **Context** - Rich context information in log messages

### Metrics

- **Execution Times** - Recipe execution duration tracking
- **Cache Hit Rates** - Cache effectiveness monitoring
- **Error Rates** - Failure tracking and analysis

### Debugging

- **Verbose Output** - Multiple levels of verbosity
- **Configuration Rendering** - `--render` flag for config debugging
- **Execution Planning** - `--show-plan` for dependency visualization

## Future Architecture Considerations

### Scalability

- **Distributed Execution** - Potential for multi-machine recipe execution
- **Cache Partitioning** - Horizontal cache scaling strategies
- **Resource Management** - Better resource utilization and limits

### Extensibility

- **Plugin System** - Runtime plugin loading for custom functionality
- **Custom Cache Backends** - User-defined cache implementations
- **Recipe Providers** - External recipe sources and registries

### Performance

- **Incremental Builds** - Fine-grained change detection
- **Parallel Parsing** - Configuration loading optimization
- **Memory Optimization** - Reduce memory footprint for large projects

This architecture supports Bake's core goals of simplicity, performance, and reliability while providing a solid foundation for future enhancements and contributions.

## Related Documentation

- [Contributing Guide](contributing.md) - How to contribute to Bake
- [Schema Documentation](schemas.md) - JSON schema development
- [Best Practices](../guides/best-practices.md) - Usage patterns and recommendations