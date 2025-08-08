# Quick Start

Get up and running with Bake in 5 minutes.

## Prerequisites

- Bake installed ([Installation Guide](installation.md))
- Basic understanding of YAML and command-line tools

## 1. Create a New Project

Create a new directory and initialize your Bake project:

```bash
mkdir my-bake-project
cd my-bake-project
```

## 2. Create Project Configuration

Create a `bake.yml` file in your project root:

```yaml
# bake.yml
name: "My Bake Project"

variables:
  environment: development
  version: "1.0.0"

config:
  max_parallel: 4
  cache:
    local:
      enabled: true
```

## 3. Create Your First Cookbook

Create an `app` directory with a `cookbook.yml` file:

```bash
mkdir app
```

```yaml
# app/cookbook.yml
name: app

variables:
  build_env: "{{var.environment}}"
  output_dir: "dist-{{var.build_env}}"

recipes:
  build:
    description: "Build the application"
    cache:
      inputs:
        - "src/**/*"
        - "package.json"
      outputs:
        - "{{var.output_dir}}/**/*"
    run: |
      echo "Building for {{var.build_env}}..."
      mkdir -p {{var.output_dir}}
      echo "Build complete" > {{var.output_dir}}/build.txt

  test:
    description: "Run tests"
    cache:
      inputs:
        - "src/**/*"
        - "test/**/*"
    run: |
      echo "Running tests..."
      echo "All tests passed!"
    dependencies:
      - build
```

## 4. Create Sample Source Files

Create some sample files for the recipes to process:

```bash
mkdir -p app/src app/test
echo "console.log('Hello World');" > app/src/main.js
echo '{"name": "my-app", "version": "1.0.0"}' > app/package.json
echo "describe('tests', () => { it('passes', () => {}); });" > app/test/main.test.js
```

## 5. Run Your First Bake

Execute all recipes:

```bash
bake
```

Or run specific recipes:

```bash
# Run all recipes in the app cookbook
bake app:

# Run just the build recipe
bake app:build

# Run with different variables
bake app:build --var environment=production
```

## 6. Explore the Results

- Check the generated output in `dist-development/`
- Notice how Bake resolved dependencies (test ran after build)
- Try running again - recipes are cached and won't re-execute unless inputs change

## What You've Learned

- **Project Structure**: Projects contain cookbooks, cookbooks contain recipes
- **Variables**: Use `{{var.name}}` for template variables
- **Dependencies**: Recipes can depend on other recipes
- **Caching**: Bake automatically caches recipe results based on inputs
- **Parallel Execution**: Multiple recipes can run simultaneously

## Next Steps

- [First Project Tutorial](first-project.md) - More detailed walkthrough
- [Configuration Guide](../guides/configuration.md) - Learn all configuration options
- [Variables Guide](../guides/variables.md) - Master the variable system
- [Recipe Templates](../guides/recipe-templates.md) - Create reusable recipe patterns

## Common Commands Reference

```bash
# Run all recipes
bake

# Run specific cookbook
bake app:

# Run specific recipe
bake app:build

# Show execution plan
bake --show-plan

# Debug configuration
bake --render

# Override variables
bake --var environment=staging --var version=2.0.0
```