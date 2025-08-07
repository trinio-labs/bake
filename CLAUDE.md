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

1. **lib.rs** - Library entry point and public API
   
   - Provides public functions for CLI argument parsing and handling
   - Exposes main application logic for integration testing
   - Contains Args struct and command handlers (list-templates, validate, render)
   - Main run() function that orchestrates the entire application

2. **main.rs** - Binary entry point
   
   - Thin wrapper that calls bake::run().await
   - Minimal CLI executable that delegates to library

3. **baker.rs** - Main execution engine

   - Manages parallel recipe execution using tokio and semaphores
   - Handles dependency resolution and execution order
   - Implements fast-fail and cancellation logic
   - Manages progress reporting and output handling

4. **project/** - Project configuration and execution planning

   - `mod.rs` - Main project module with project loading and execution planning
   - `config.rs` - Tool configuration (parallelism, caching, updates)
   - `cookbook.rs` - Cookbook (collection of recipes) management
   - `recipe.rs` - Individual recipe definitions and execution context
   - `graph.rs` - Recipe dependency graph using petgraph
   - `hashing.rs` - Input/output fingerprinting for cache keys

5. **cache/** - Multi-tier caching system

   - `local.rs` - Local filesystem cache
   - `s3.rs` - AWS S3 remote cache
   - `gcs.rs` - Google Cloud Storage cache
   - `builder.rs` - Cache strategy composition and configuration

6. **template.rs** - Variable substitution system

   - Handlebars-based template engine
   - Hierarchical variable scoping (project → cookbook → recipe → CLI)
   - Built-in constants ({{project.root}}, {{cookbook.root}}, etc.)

5. **project/recipe_template.rs** - Recipe template system

   - Template definitions with typed parameters (string, number, boolean, array, object)
   - Parameter validation with defaults, required fields, and constraints
   - Template discovery from `.bake/templates/` directories
   - Template instantiation with parameter substitution using Handlebars
   - Template inheritance support with `extends` field

7. **update.rs** - Self-update functionality
   - GitHub release checking and binary updates
   - Configurable update intervals and auto-update behavior

### Key Concepts

- **Recipes**: Individual tasks that can be executed, cached, and have dependencies
- **Cookbooks**: Collections of related recipes, typically per project/package
- **Dependency Graph**: Recipes are executed in dependency order using topological sorting
- **Parallel Execution**: Multiple recipes can run concurrently with configurable limits
- **Smart Caching**: Recipes are cached based on input file hashes, dependencies, and command content
- **Variable System**: Hierarchical template variables with environment, user, and built-in variables
- **Recipe Templates**: Reusable recipe definitions with typed parameters to eliminate duplication and standardize patterns

## Testing

### Test Structure

- **Unit Tests**: Comprehensive unit tests in `src/` modules with `#[cfg(test)]` blocks
- **Integration Tests**: Located in `tests/` directory root (Rust convention)
  - `baker_tests.rs` - Recipe execution and baking logic tests
  - `cache_tests.rs` - Cache strategy and builder tests  
  - `project_tests.rs` - Project loading and configuration tests
  - `s3_cache_tests.rs` - S3 cache implementation tests
  - `template_tests.rs` - Variable loading and template system tests
- **Test Helpers**: `TestProjectBuilder` in `tests/common/mod.rs` for creating test projects  
- **Library+Binary Pattern**: Tests import from library crate (`bake::`) for better testability
- Use the macro test_case whenever possible to make code dryer
- Always run the tool with a valid project configuration to ensure correct behavior

### Running Tests

- All tests: `cargo test`
- Specific test: `cargo test <test_name>`
- Verbose output: `cargo test -- --nocapture`
- Parallel test execution: `cargo test -- --test-threads=1` (if needed)
- Run the tool on a valid project: `cargo run -- -p ./resources/tests/valid/`

### Test Patterns

- Tests create temporary projects with cookbooks and recipes
- Use `TestCacheStrategy` for predictable cache behavior in tests
- Verify recipe execution order, error handling, and cache operations

## Error Handling

- Fast-fail behavior: stops execution on first error (configurable)
- Graceful cancellation: Ctrl+C handling with cleanup
- Detailed error reporting with recipe context
- Log files preserved for debugging failed recipes

## Publishing a new version of Bake

To publish a new version of Bake, follow these steps in order:

- Run tests and ensure they pass: `cargo test`
- Run clippy and fix any issues: `cargo clippy --all-targets --fix --all-features`
- Bump the version number in `Cargo.toml` according to semantic versioning
  - Major version: Breaking changes
  - Minor version: New features, no breaking changes
  - Patch version: Bug fixes, no new features or breaking changes
- Update all documentation, including this file
- Update CHANGELOG.md with all changes since the last release
- Commit all changes with a clear message using conventional commit format
- Tag the commit with the version number (e.g., `v1.2.3`)
- Push the changes to the main branch

## Code Implementation Guidelines - **CRITICAL**

- **Principles**: Adhere closely to KISS, DRY, SOLID, YAGNI and the Zen of Python.
- **Clean up after yourself**: Always ensure that temporary files, directories, and resources are cleaned up after use.
- **Never create stubs**: Always implement complete, functional code rather than placeholder stubs
- **No TODO comments**: Avoid leaving TODO markers or incomplete code sections
- **Follow style guidelines**: Adhere to established coding standards and best practices
- **Ask questions**: If in doubt, seek clarification to ensure understanding and correctness.
- **Library context**: Use context7 to get the documentation for complex dependencies
