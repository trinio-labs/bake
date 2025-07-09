# Core Module - CLAUDE.md

This file provides guidance for working with the core source files in the bake project.

## Key Files

- **main.rs** - CLI entry point and argument parsing using clap
- **baker.rs** - Main execution engine for parallel recipe execution with tokio
- **template.rs** - Variable substitution system using Handlebars with type preservation
- **update.rs** - Self-update functionality via GitHub releases API
- **test_utils.rs** - Shared testing utilities and TestProjectBuilder
- **project/** - Project management module (uses mod.rs pattern)
- **cache/** - Multi-tier caching system (uses mod.rs pattern)

## Module Structure

The bake project uses the modern Rust `mod.rs` pattern for organizing modules:

- **project/mod.rs** - Main project module (handles project loading and coordination)
- **cache/mod.rs** - Main cache module (cache traits and common functionality)

This structure provides cleaner separation between module interface and implementation, making it easier to navigate and maintain the codebase.

## Template System (`template.rs`)

### Variable Context Structure

- `VariableContext` manages hierarchical variable resolution
- `process_template_in_value()` processes YAML values while preserving types
- `parse_template()` processes individual template strings
- Uses Handlebars engine with custom helpers

### Built-in Template Variables

- `{{project.root}}` - Project root directory
- `{{cookbook.root}}` - Cookbook directory
- `{{env.VAR}}` - Environment variables
- `{{var.NAME}}` - User-defined variables

### Type Preservation

Templates maintain YAML types (bool, number, string) after processing:

```yaml
variables:
  debug: true
  port: 8080
recipes:
  build:
    debug_mode: "{{var.debug}}" # Becomes boolean true
    server_port: "{{var.port}}" # Becomes number 8080
```

## Baker (`baker.rs`)

### Execution Flow

1. Build dependency graph from recipes
2. Create semaphore for parallelism control
3. Execute recipes in topological order
4. Handle cancellation and cleanup

### Key Features

- Parallel execution within dependency levels
- Fast-fail behavior (configurable)
- Progress reporting with indicatif
- Graceful Ctrl+C handling

## Update System (`update.rs`)

### GitHub Integration

- Checks releases via GitHub API
- Downloads binary updates
- Configurable update intervals
- Auto-update support

### Configuration

```yaml
update:
  check: true
  interval: 86400 # seconds
  auto: false
```

## Testing (`test_utils.rs`)

### TestProjectBuilder Pattern

```rust
TestProjectBuilder::new()
    .cookbook("test", |cookbook| {
        cookbook.recipe("build", |recipe| {
            recipe.run("echo 'building'")
                  .cache_inputs(vec!["src/**/*.rs"])
        })
    })
    .build()?
```

### Test Helpers

- `TestCacheStrategy` for predictable cache behavior
- Temporary project creation with cleanup
- Recipe execution verification
