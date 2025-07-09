# Project Module - CLAUDE.md

This file provides guidance for working with the project module in the bake project.

## Module Overview

The project module handles project configuration, recipe definitions, cookbook management, and dependency graph
construction. It's the core of bake's project model and execution planning.

## Key Files

- **mod.rs** - Main project module with project loading
- **config.rs** - Tool configuration (parallelism, caching, updates)
- **cookbook.rs** - Cookbook (collection of recipes) management and parsing
- **recipe.rs** - Individual recipe definitions and execution context
- **graph.rs** - Recipe dependency graph construction using petgraph
- **hashing.rs** - Input/output fingerprinting for cache keys

## Architecture

### Project Configuration Hierarchy

```text
Project (bake.yml)
├── Global variables
├── Cache configuration
├── Tool settings
└── Cookbooks
    ├── Cookbook (cookbook.yml)
    │   ├── Local variables
    │   └── Recipes
    │       ├── Recipe
    │       │   ├── run command
    │       │   ├── dependencies
    │       │   ├── cache (inputs/outputs)
    │       │   └── variables
```

### Dependency Graph

- Built using `petgraph` crate for efficient graph operations
- Topological sorting determines execution order
- Parallel execution within dependency levels
- Cycle detection prevents infinite loops

## Key Concepts

### Project Loading

1. **Discovery**: Find `bake.yml` in current or parent directories
2. **Parsing**: Parse project configuration and validate
3. **Cookbook Loading**: Load and parse all referenced cookbooks
4. **Recipe Collection**: Gather all recipes from all cookbooks
5. **Execution planning**: Find out which recipes are the target based on the CLI arguments
6. **Graph Construction**: Build dependency graph for execution planning

### Recipe Execution Context

Each recipe runs with:

- Working directory set to cookbook directory
- Environment variables from multiple sources
- Template variables resolved from hierarchy
- Input/output file tracking for caching

### Variable Resolution

Variables are resolved in order of precedence:

1. CLI overrides (`--var key=value`)
2. Recipe-level variables
3. Cookbook-level variables
4. Project-level variables
5. Environment variables (`{{env.VAR}}`)
6. Built-in constants (`{{project.root}}`, `{{cookbook.root}}`)

## Implementation Guidelines

### Configuration Parsing

- Use `serde` for YAML deserialization
- Implement `Default` for optional configuration
- Validate configuration early with helpful error messages
- Support both single files and directory structures

### Error Handling

- Provide context for configuration errors (file:line)
- Validate recipe dependencies exist
- Check for circular dependencies
- Validate file paths and permissions

### Performance Considerations

- Cache parsed configurations when possible
- Use lazy loading for large projects
- Parallel cookbook loading when feasible
- Efficient graph algorithms for dependency resolution

## Configuration Examples

### Project Configuration (bake.yml)

```yaml
cookbooks:
  - path: ./frontend
  - path: ./backend
  - path: ./shared

variables:
  NODE_ENV: production
  BUILD_DIR: dist

cache:
  local: true
  s3:
    bucket: my-cache-bucket

parallelism: 4
```

### Cookbook Configuration (cookbook.yml)

```yaml
name: "my-cookbook"
variables:
  PORT: 3000

recipes:
  install:
    description: "Install dependencies"
    run: npm install
    cache:
      inputs: [package.json, package-lock.json]
      outputs: [node_modules]

  build:
    description: "Build the application"
    run: npm run build
    dependencies: [install]
    cache:
      inputs: [src/**/*.ts, tsconfig.json]
      outputs: [dist]
```

## Recipe Schema

### Required Fields

- **name**: Auto-populated from recipe key
- **run**: Shell command to execute

### Optional Fields

- **description**: Human-readable description
- **dependencies**: Array of recipe names (can be cross-cookbook with `cookbook:recipe`)
- **variables**: Recipe-specific variables
- **environment**: Environment variables to inherit
- **cache**: Cache configuration with inputs/outputs

### Cache Configuration

- **inputs**: Array of file patterns that affect this recipe
- **outputs**: Array of file patterns this recipe produces

## Development Tips

### Adding New Configuration Options

1. Add fields to the appropriate struct
2. Update `Default` implementation if needed
3. Add validation logic
4. Update documentation and examples
5. Add tests for new configuration

### Recipe Definition Best Practices

- Use glob patterns for cache inputs/outputs when appropriate
- Specify minimal but complete dependencies
- Use relative paths from cookbook root
- Include all files that affect the recipe outcome

### Testing

- Use `TestProjectBuilder` for creating test projects
- Test configuration parsing edge cases
- Verify dependency graph construction
- Test variable resolution with different scopes

## Common Patterns

### Recipe Dependencies

```yaml
recipes:
  test:
    run: npm test
    dependencies: [build]

  build:
    run: npm run build
    dependencies: [install]

  install:
    run: npm install
```

### File Patterns in Cache

```yaml
recipes:
  typescript:
    cache:
      inputs:
        - "src/**/*.ts"
        - "*.json"
      outputs:
        - "dist/**/*.js"
        - "dist/**/*.d.ts"
```

### Variable Usage

```yaml
variables:
  API_URL: "https://api.example.com"

recipes:
  deploy:
    run: ./deploy.sh {{var.API_URL}}
    cache:
      inputs: [dist/**/*]
```

### Cross-Cookbook Dependencies

```yaml
recipes:
  integration-test:
    run: npm run test:integration
    dependencies:
      - backend:build
      - frontend:build
```
