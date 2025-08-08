# Contributing to Bake

Thank you for considering contributing to Bake! This guide explains how to contribute code, documentation, and feedback to help make Bake better.

## Ways to Contribute

- **Bug Reports** - Report issues or unexpected behavior
- **Feature Requests** - Suggest new features or improvements
- **Documentation** - Improve or add to documentation
- **Code Contributions** - Fix bugs or implement new features
- **Testing** - Help test new features or edge cases

## Getting Started

### Prerequisites

- Rust 1.75 or later
- Git for version control
- Basic understanding of Rust and async programming

### Development Setup

1. **Fork and clone the repository**:
   ```bash
   git clone https://github.com/your-username/bake.git
   cd bake
   ```

2. **Set up development environment**:
   ```bash
   # Install Rust if not already installed
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   
   # Install development dependencies
   cargo install cargo-watch
   cargo install cargo-nextest  # Optional, for faster testing
   ```

3. **Run initial tests**:
   ```bash
   cargo test
   cargo clippy --all-targets --all-features
   cargo fmt --check
   ```

4. **Test the tool locally**:
   ```bash
   cargo run -- --help
   cargo run -- -p ./resources/tests/valid/
   ```

### Development Workflow

1. **Create a feature branch**:
   ```bash
   git checkout -b feature/your-feature-name
   # or
   git checkout -b bugfix/issue-description
   ```

2. **Make your changes** following the coding guidelines below

3. **Test your changes**:
   ```bash
   # Run all tests
   cargo test
   
   # Run tests with coverage
   cargo test --all-features
   
   # Check formatting and linting
   cargo fmt
   cargo clippy --all-targets --all-features --fix
   
   # Test with real projects
   cargo run -- -p ./resources/tests/valid/
   ```

4. **Commit your changes**:
   ```bash
   git add .
   git commit -m "feat: add feature description" # Use conventional commits
   ```

5. **Push and create a pull request**:
   ```bash
   git push origin feature/your-feature-name
   ```

## Coding Guidelines

### Code Style

Bake follows Rust standard formatting and linting practices:

```bash
# Format code
cargo fmt

# Check and fix linting issues
cargo clippy --all-targets --all-features --fix

# Check for unused dependencies
cargo machete --with-metadata
```

### Code Principles

Follow these principles when contributing:

1. **KISS (Keep It Simple, Stupid)** - Prefer simple, clear solutions
2. **DRY (Don't Repeat Yourself)** - Extract common functionality
3. **SOLID Principles** - Write maintainable, extensible code
4. **YAGNI (You Aren't Gonna Need It)** - Don't over-engineer

### Error Handling

- Use `Result<T, E>` for recoverable errors
- Use `anyhow::Result` for application errors
- Provide meaningful error messages with context
- Never use `panic!` or `unwrap()` in production code paths

```rust
// Good - Proper error handling
fn load_config(path: &Path) -> anyhow::Result<Config> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read config file: {}", path.display()))?;
    
    let config: Config = serde_yaml::from_str(&content)
        .with_context(|| format!("Invalid YAML in config file: {}", path.display()))?;
    
    Ok(config)
}

// Avoid - Unwrapping and panicking
fn load_config_bad(path: &Path) -> Config {
    let content = std::fs::read_to_string(path).unwrap(); // Don't do this!
    serde_yaml::from_str(&content).expect("Invalid config") // Or this!
}
```

### Async/Await

- Use async/await for I/O operations
- Avoid blocking operations in async contexts
- Use `tokio::spawn` for concurrent tasks
- Handle cancellation gracefully

```rust
// Good - Async I/O operations
async fn execute_recipe(&self, recipe: &Recipe) -> anyhow::Result<()> {
    let output = tokio::process::Command::new("sh")
        .arg("-c")
        .arg(&recipe.run)
        .output()
        .await
        .context("Failed to execute recipe command")?;
    
    if !output.status.success() {
        anyhow::bail!("Recipe failed with exit code: {:?}", output.status.code());
    }
    
    Ok(())
}
```

### Testing

Write comprehensive tests for new functionality:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_recipe_execution() {
        let recipe = Recipe {
            name: "test".to_string(),
            run: "echo 'hello'".to_string(),
            ..Default::default()
        };
        
        let result = execute_recipe(&recipe).await;
        assert!(result.is_ok());
    }
    
    #[test]
    fn test_config_parsing() {
        let yaml = r#"
            name: test-project
            cookbooks: [frontend, backend]
        "#;
        
        let config: ProjectConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.name, Some("test-project".to_string()));
        assert_eq!(config.cookbooks.len(), 2);
    }
}
```

## Project Structure

Understanding Bake's architecture will help you contribute effectively:

```
src/
├── lib.rs              # Library entry point, CLI argument parsing
├── main.rs             # Binary entry point (thin wrapper)
├── baker.rs            # Main execution engine
├── template.rs         # Variable substitution system
├── update.rs           # Self-update functionality
├── cache/              # Multi-tier caching system
│   ├── mod.rs         # Cache trait and builder
│   ├── local.rs       # Local filesystem cache
│   ├── s3.rs          # AWS S3 cache
│   ├── gcs.rs         # Google Cloud Storage cache
│   └── builder.rs     # Cache strategy composition
└── project/            # Project configuration and planning
    ├── mod.rs         # Project loading and execution planning
    ├── config.rs      # Tool configuration
    ├── cookbook.rs    # Cookbook management
    ├── recipe.rs      # Recipe definitions
    ├── graph.rs       # Dependency graph
    ├── hashing.rs     # Input/output fingerprinting
    └── recipe_template.rs # Recipe template system
```

### Key Components

- **lib.rs** - Public API, CLI parsing, command handlers
- **baker.rs** - Recipe execution with parallelism and caching
- **project/** - Configuration loading and dependency resolution
- **cache/** - Local and remote caching implementations
- **template.rs** - Handlebars template processing

## Testing Strategy

### Test Types

1. **Unit Tests** - Test individual functions and modules
2. **Integration Tests** - Test complete workflows (in `tests/` directory)
3. **Property Tests** - Test with generated inputs (use `proptest`)
4. **Performance Tests** - Benchmark critical paths

### Test Organization

```bash
# Run all tests
cargo test

# Run specific test
cargo test test_recipe_execution

# Run integration tests only
cargo test --test '*'

# Run with output
cargo test -- --nocapture

# Run tests in parallel
cargo nextest run  # If installed
```

### Test Helpers

Use the existing test infrastructure:

```rust
use crate::tests::TestProjectBuilder;

#[tokio::test]
async fn test_complex_project() {
    let project = TestProjectBuilder::new()
        .with_cookbook("frontend", |cb| {
            cb.with_recipe("build", "npm run build")
              .with_inputs(&["src/**/*.ts", "package.json"])
        })
        .with_cookbook("backend", |cb| {
            cb.with_recipe("test", "cargo test")
              .with_dependency("frontend:build")
        })
        .build()
        .await;
    
    let result = project.run_recipe("backend:test").await;
    assert!(result.is_ok());
}
```

## Documentation

### Code Documentation

- Add rustdoc comments for public APIs
- Include examples in doc comments
- Document error conditions and panics

```rust
/// Execute a recipe with proper error handling and logging.
///
/// This function runs the recipe's command in a shell environment,
/// captures output, and handles both success and failure cases.
///
/// # Arguments
/// * `recipe` - The recipe to execute
/// 
/// # Returns
/// * `Ok(())` - Recipe executed successfully
/// * `Err(anyhow::Error)` - Recipe failed or system error occurred
///
/// # Examples
/// ```
/// let recipe = Recipe {
///     name: "build".to_string(), 
///     run: "npm run build".to_string(),
///     ..Default::default()
/// };
/// 
/// execute_recipe(&recipe).await?;
/// ```
pub async fn execute_recipe(recipe: &Recipe) -> anyhow::Result<()> {
    // Implementation
}
```

### User Documentation

When adding features that affect users:

1. Update relevant documentation in `docs/`
2. Add examples to the appropriate guides
3. Update the CLI help text if needed
4. Consider updating the `README.md` if it's a major feature

## Pull Request Process

### Before Submitting

- [ ] Tests pass locally (`cargo test`)
- [ ] Code is formatted (`cargo fmt`)
- [ ] No clippy warnings (`cargo clippy`)
- [ ] Documentation is updated
- [ ] Changelog is updated (for significant changes)

### Pull Request Guidelines

1. **Title**: Use conventional commit format
   - `feat: add new feature`
   - `fix: resolve bug in X`
   - `docs: update configuration guide`
   - `refactor: reorganize cache module`

2. **Description**: Include:
   - What changes were made
   - Why the changes were needed  
   - Any breaking changes
   - Testing done

3. **Size**: Keep PRs focused and reasonably sized
   - Large features should be split into multiple PRs
   - Include tests with the feature PR

### Review Process

1. Automated checks must pass (CI/CD)
2. At least one maintainer review required
3. All conversations must be resolved
4. Squash commits when merging (unless preserving history is important)

## Release Process

For maintainers releasing new versions:

1. **Update Version**: Bump version in `Cargo.toml`
2. **Update Changelog**: Add release notes to `CHANGELOG.md`
3. **Run Tests**: Ensure all tests pass
4. **Create Tag**: `git tag v1.2.3`
5. **Push**: `git push && git push --tags`
6. **Publish**: `cargo publish`

### Semantic Versioning

Bake follows semantic versioning:
- **Major** (1.0.0): Breaking changes
- **Minor** (0.1.0): New features, backwards compatible
- **Patch** (0.0.1): Bug fixes, backwards compatible

## Getting Help

### Communication Channels

- **GitHub Issues** - Bug reports and feature requests
- **GitHub Discussions** - General questions and design discussions
- **Discord** - Real-time chat (if available)

### Asking for Help

When asking for help:

1. **Be specific** - Include error messages, code snippets, versions
2. **Provide context** - What were you trying to do?
3. **Share environment** - OS, Rust version, Bake version
4. **Minimal reproduction** - Smallest example that shows the issue

### Reporting Bugs

Good bug reports include:

1. **Summary** - Brief description of the issue
2. **Environment** - OS, Rust version, Bake version
3. **Steps to reproduce** - Exact steps to trigger the bug
4. **Expected behavior** - What should happen
5. **Actual behavior** - What actually happened
6. **Additional context** - Logs, configurations, screenshots

### Feature Requests

Good feature requests include:

1. **Problem statement** - What problem does this solve?
2. **Proposed solution** - How should it work?
3. **Use cases** - Real-world examples
4. **Alternatives considered** - Other approaches explored

## Code of Conduct

We expect all contributors to follow our code of conduct:

- **Be respectful** - Treat everyone with respect and kindness
- **Be inclusive** - Welcome contributions from everyone
- **Be collaborative** - Work together to build something great
- **Be patient** - Remember that everyone is learning
- **Be constructive** - Provide helpful feedback and suggestions

## License

By contributing to Bake, you agree that your contributions will be licensed under the same license as the project.

## Recognition

Contributors are recognized in:
- Release notes for significant contributions
- `CONTRIBUTORS.md` file (if maintained)
- GitHub contributors page

Thank you for contributing to Bake! Your help makes the project better for everyone.

## Related Documentation

- [Architecture Documentation](architecture.md) - Internal system design
- [Schema Documentation](schemas.md) - JSON schema development
- [Troubleshooting Guide](../guides/troubleshooting.md) - Common development issues