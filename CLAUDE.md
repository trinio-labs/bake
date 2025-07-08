# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Development Commands

### Build and Test

- **Build**: `cargo build` (debug) or `cargo build --release` (optimized)
- **Test**: `cargo test` (run all tests) or `cargo test --no-fail-fast` (run all tests even if some fail)
- **Single test**: `cargo test <test_name>` or `cargo test <module>::<test_name>`
- **Linting**: `cargo clippy` (check for common mistakes and style issues)
- **Format**: `cargo fmt` (auto-format code according to Rust style guidelines)

### Running the Tool

- **Run locally**: `cargo run` or `cargo run -- <args>`
- **Install locally**: `cargo install --path .`

## Architecture Overview

Bake is a parallel task runner with smart caching, built in Rust. The architecture consists of several key components:

### Core Components

1. **baker.rs** - Main execution engine

   - Manages parallel recipe execution using tokio and semaphores
   - Handles dependency resolution and execution order
   - Implements fast-fail and cancellation logic
   - Manages progress reporting and output handling

2. **project/** - Project configuration and management

   - `config.rs` - Tool configuration (parallelism, caching, updates)
   - `cookbook.rs` - Cookbook (collection of recipes) management
   - `recipe.rs` - Individual recipe definitions and execution context
   - `graph.rs` - Recipe dependency graph using petgraph
   - `hashing.rs` - Input/output fingerprinting for cache keys

3. **cache/** - Multi-tier caching system

   - `local.rs` - Local filesystem cache
   - `s3.rs` - AWS S3 remote cache
   - `gcs.rs` - Google Cloud Storage cache
   - `builder.rs` - Cache strategy composition and configuration

4. **template.rs** - Variable substitution system

   - Handlebars-based template engine
   - Hierarchical variable scoping (project → cookbook → recipe → CLI)
   - Built-in constants ({{project.root}}, {{cookbook.root}}, etc.)

5. **update.rs** - Self-update functionality
   - GitHub release checking and binary updates
   - Configurable update intervals and auto-update behavior

### Key Concepts

- **Recipes**: Individual tasks that can be executed, cached, and have dependencies
- **Cookbooks**: Collections of related recipes, typically per project/package
- **Dependency Graph**: Recipes are executed in dependency order using topological sorting
- **Parallel Execution**: Multiple recipes can run concurrently with configurable limits
- **Smart Caching**: Recipes are cached based on input file hashes, dependencies, and command content
- **Variable System**: Hierarchical template variables with environment, user, and built-in variables

## Project Structure

### Configuration Files

- `bake.yml` - Project root configuration defining cookbooks, global variables, and tool settings
- `cookbook.yml` - Per-cookbook configuration with recipes and local variables

### Recipe Execution Model

- Recipes specify `inputs` (files that affect caching), `outputs` (files produced), and `dependencies`
- Execution happens in parallel within dependency levels
- Each recipe runs in its cookbook directory with environment variables
- Output is captured to log files in `.bake/logs/`

### Cache Strategy

- Cache keys are computed from input file hashes, dependency hashes, and recipe content
- Multiple cache tiers (local, S3, GCS) with configurable priority order
- Cache hits skip recipe execution; misses trigger execution and cache storage

### Variable Scoping

Variables are resolved in order of precedence:

1. CLI overrides (`--var key=value`)
2. Recipe-level variables
3. Cookbook-level variables
4. Project-level variables
5. Environment variables (`{{env.VAR}}`)
6. Built-in constants (`{{project.root}}`, `{{cookbook.root}}`)

## Testing

### Test Structure

- Unit tests use `TestProjectBuilder` helper for creating test projects
- Integration tests verify recipe execution, caching, and error handling
- Mock cache strategies for testing cache behavior

### Running Tests

- All tests: `cargo test`
- Specific test: `cargo test <test_name>`
- Verbose output: `cargo test -- --nocapture`
- Parallel test execution: `cargo test -- --test-threads=1` (if needed)

### Test Patterns

- Tests create temporary projects with cookbooks and recipes
- Use `TestCacheStrategy` for predictable cache behavior in tests
- Verify recipe execution order, error handling, and cache operations

## Error Handling

- Fast-fail behavior: stops execution on first error (configurable)
- Graceful cancellation: Ctrl+C handling with cleanup
- Detailed error reporting with recipe context
- Log files preserved for debugging failed recipes

## Development Tips

- Recipe execution uses shell commands with `set -e` for error propagation
- Progress bars and colored output for better UX
- Verbose mode available for debugging recipe execution
- Cache operations are async and use structured concurrency

## Code Implementation Guidelines

- **Never create stubs**: Always implement complete, functional code rather than placeholder stubs
- **Complete implementations only**: All functions, methods, and modules should be fully implemented
- **No TODO comments**: Avoid leaving TODO markers or incomplete code sections

