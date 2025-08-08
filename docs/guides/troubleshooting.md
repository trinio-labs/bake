# Troubleshooting Guide

This guide covers common issues you may encounter when using Bake and how to resolve them.

## General Debugging

### Enable Verbose Output

Get more detailed information about what Bake is doing:

```bash
# Show detailed execution information
bake --verbose

# Show debug-level information
bake --debug

# Show execution plan without running
bake --show-plan

# Show resolved configuration
bake --render
```

### Check Configuration

Validate your configuration files:

```bash
# Validate all configuration
bake --validate

# Check specific cookbook
bake frontend: --validate

# Render configuration to see resolved variables
bake --render
```

## Installation Issues

### Command Not Found

**Problem**: `bake: command not found`

**Solution**:
```bash
# Check if bake is installed
which bake

# If installed via Cargo, check PATH
echo $PATH | grep -o ~/.cargo/bin

# Add to PATH if missing (add to ~/.bashrc or ~/.zshrc)
export PATH="$HOME/.cargo/bin:$PATH"

# If installed via Homebrew
brew list | grep bake

# Reinstall if necessary
cargo install bake-cli --force
```

### Permission Issues

**Problem**: Permission denied when running bake

**Solution**:
```bash
# Check file permissions
ls -la $(which bake)

# Fix permissions if necessary
chmod +x $(which bake)

# On macOS, if getting "unidentified developer" error
xattr -d com.apple.quarantine $(which bake)
```

## Configuration Issues

### Cookbook Not Found

**Problem**: `Error: Cookbook 'frontend' not found`

**Diagnosis**:
```bash
# Check if cookbook directory exists
ls -la frontend/

# Check if cookbook.yml exists
ls -la frontend/cookbook.yml

# Verify cookbook is listed in bake.yml
cat bake.yml | grep -A5 cookbooks
```

**Solutions**:
- Ensure the cookbook directory exists
- Create `cookbook.yml` file in the cookbook directory
- Add cookbook to the `cookbooks` list in `bake.yml`
- Check spelling of cookbook names

### Recipe Not Found

**Problem**: `Error: Recipe 'build' not found in cookbook 'frontend'`

**Diagnosis**:
```bash
# List all recipes in cookbook
bake --list-recipes | grep frontend

# Check cookbook configuration
cat frontend/cookbook.yml
```

**Solutions**:
- Verify recipe is defined in `cookbook.yml`
- Check recipe name spelling
- Ensure proper YAML indentation

### Invalid YAML Syntax

**Problem**: `Error: Invalid YAML syntax`

**Diagnosis**:
```bash
# Validate YAML syntax
python3 -c "import yaml; yaml.safe_load(open('bake.yml'))"

# Or use online YAML validator
```

**Common Issues**:
- Inconsistent indentation (use spaces, not tabs)
- Missing colons after keys
- Incorrect string escaping
- Missing quotes around special characters

**Example Fix**:
```yaml
# Wrong - mixed indentation
recipes:
  build:
      description: Build app  # 6 spaces
    run: npm build          # 4 spaces

# Correct - consistent indentation  
recipes:
  build:
    description: Build app  # 4 spaces
    run: npm build          # 4 spaces
```

## Variable Issues

### Variable Not Found

**Problem**: `Error: Variable 'environment' not found`

**Diagnosis**:
```bash
# Check variable definition
bake --render | grep -A10 -B10 environment

# See all defined variables
bake --render | grep -A50 "variables:"
```

**Solutions**:
- Define the variable at project, cookbook, or recipe level
- Check variable name spelling
- Verify variable scoping (project → cookbook → recipe)

**Example Fix**:
```yaml
# Add missing variable
variables:
  environment: development  # Define the variable
  
recipes:
  build:
    run: echo "Building for {{var.environment}}"
```

### Variable Resolution Errors

**Problem**: Variables show as `{{var.name}}` instead of actual values

**Diagnosis**:
```bash
# Check variable syntax and resolution
bake --render --var debug=true
```

**Common Issues**:
- Wrong variable syntax (`{var.name}` instead of `{{var.name}}`)
- Circular variable references
- Undefined variables

**Example Fix**:
```yaml
# Wrong syntax
run: echo "Version: {var.version}"

# Correct syntax  
run: echo "Version: {{var.version}}"
```

## Dependency Issues

### Circular Dependencies

**Problem**: `Error: Circular dependency detected`

**Diagnosis**:
```bash
# Show execution plan to see dependency chain
bake --show-plan
```

**Solution**: Break the circular dependency by restructuring recipes:
```yaml
# Problem - Circular dependency
recipes:
  build:
    dependencies: [test]
  test:
    dependencies: [build]    # Circular!

# Solution - Remove circular reference
recipes:
  compile:
    run: tsc
  build:
    dependencies: [compile, test]
  test:
    dependencies: [compile]   # Depend on compilation, not build
```

### Dependency Not Found

**Problem**: `Error: Dependency 'shared:build' not found`

**Diagnosis**:
```bash
# Check if dependency recipe exists
bake --list-recipes | grep shared:build

# Verify cookbook exists
ls -la shared/cookbook.yml
```

**Solutions**:
- Ensure the cookbook exists and is listed in `bake.yml`
- Verify the recipe exists in the target cookbook
- Check spelling of cookbook and recipe names
- For same-cookbook dependencies, use just the recipe name

## Caching Issues

### Cache Never Hits

**Problem**: Recipes always re-run even when nothing has changed

**Diagnosis**:
```bash
# Check input patterns
bake --verbose frontend:build

# See what files are being tracked
bake --debug frontend:build | grep "Input files"
```

**Common Causes**:
- Input patterns too broad (`**/*`)
- Missing file extensions in patterns
- Including files that change frequently
- Environment variables changing between runs

**Solutions**:
```yaml
# Be more specific with inputs
recipes:
  build:
    cache:
      inputs:
        # Specific file types only
        - "src/**/*.{ts,tsx,js,jsx}"
        - "package.json"
        - "tsconfig.json"
        # Exclude frequently changing files
        - "!src/**/*.test.ts"
        - "!**/*.log"
        - "!**/node_modules/**"
```

### Cache Hits But Wrong Output

**Problem**: Cache hits but outputs are incorrect or stale

**Diagnosis**:
```bash
# Clear cache and rebuild
rm -rf .bake/cache
bake frontend:build

# Check output patterns
bake --render frontend:build | grep -A5 outputs
```

**Solutions**:
- Include all generated files in `outputs`
- Ensure build process is deterministic
- Check for absolute paths in output files

### Remote Cache Failures

**Problem**: `Error: Failed to upload/download from S3/GCS`

**Diagnosis**:
```bash
# Test credentials
aws sts get-caller-identity  # For S3
gcloud auth list            # For GCS

# Check bucket access
aws s3 ls s3://my-bucket/   # For S3
gsutil ls gs://my-bucket/   # For GCS
```

**Common Solutions**:
- Verify credentials are configured
- Check bucket permissions
- Ensure bucket exists
- Test network connectivity

## Performance Issues

### Slow Execution

**Problem**: Bake takes too long to run

**Diagnosis**:
```bash
# Profile execution time
time bake

# Check parallel execution
bake --verbose | grep "Running recipe"

# See dependency graph
bake --show-plan
```

**Solutions**:
- Increase `max_parallel` setting
- Reduce unnecessary dependencies  
- Optimize input patterns for better caching
- Use remote caching for shared builds

### High Memory Usage

**Problem**: Bake uses too much memory

**Solutions**:
- Reduce `max_parallel` setting
- Use more specific input/output patterns
- Clean up old cache files
- Break large recipes into smaller ones

## Recipe Execution Issues

### Command Not Found in Recipe

**Problem**: `Error: command not found` within recipe execution

**Diagnosis**:
```bash
# Check PATH in recipe environment
recipes:
  debug-env:
    run: |
      echo "PATH: $PATH"
      which node
      which npm
```

**Solutions**:
- Ensure required tools are installed
- Add tool directories to PATH
- Use absolute paths for commands
- Set `clean_environment: false` if you need system PATH

### Environment Variable Issues

**Problem**: Environment variables not available in recipes

**Diagnosis**:
```bash
# Check available environment variables
recipes:
  debug-env:
    run: env | sort
```

**Solutions**:
```yaml
# Explicitly list required environment variables
recipes:
  build:
    environment:
      - NODE_ENV
      - API_URL
      - DATABASE_URL
    run: |
      echo "NODE_ENV: $NODE_ENV"
      npm run build
```

### Recipe Fails Silently

**Problem**: Recipe appears to succeed but doesn't do what's expected

**Solutions**:
- Add explicit error checking:
```yaml
recipes:
  build:
    run: |
      set -e  # Exit on any error
      npm run build
      
      # Verify expected outputs exist
      if [ ! -f "dist/index.html" ]; then
        echo "ERROR: Build did not create expected output"
        exit 1
      fi
```

## Template Issues

### Template Not Found

**Problem**: `Error: Template 'build-template' not found`

**Diagnosis**:
```bash
# List available templates
bake --list-templates

# Check template file exists
ls -la .bake/templates/build-template.yml
```

**Solutions**:
- Ensure template file exists in `.bake/templates/`
- Check template file naming (`.yml` or `.yaml`)
- Verify template syntax with `bake --validate-templates`

### Template Parameter Validation Errors

**Problem**: `Error: Required parameter 'service_name' is missing`

**Diagnosis**:
```bash
# Check template parameters
bake --list-templates build-template

# Validate template usage
bake --render frontend:build
```

**Solutions**:
- Provide all required parameters:
```yaml
recipes:
  build:
    template: build-template
    params:
      service_name: "my-service"  # Add missing parameter
      port: 3000
```

## Common Error Messages

### "No recipes to run"

**Causes**:
- No cookbooks defined in `bake.yml`
- Cookbooks exist but have no recipes
- Recipe pattern doesn't match any recipes

**Solution**:
```bash
# Check what recipes exist
bake --list-recipes

# Run all recipes
bake

# Check cookbook configuration
cat */cookbook.yml
```

### "Failed to parse configuration"

**Causes**:
- Invalid YAML syntax
- Missing required fields
- Incorrect file structure

**Solution**:
```bash
# Validate YAML files
python3 -c "import yaml; [yaml.safe_load(open(f)) for f in ['bake.yml', 'frontend/cookbook.yml']]"

# Use bake validation
bake --validate
```

### "Recipe execution failed"

**Causes**:
- Command not found
- Script errors
- Missing dependencies
- Permission issues

**Solution**:
```bash
# Run with verbose output
bake --verbose recipe-name

# Test commands manually
cd cookbook-directory
# Run the exact command from the recipe
```

## Getting Help

### Debug Information

When reporting issues, include:

```bash
# Bake version
bake --version

# System information
uname -a

# Configuration (sanitized)
bake --render | head -50

# Full error output
bake --verbose 2>&1 | tee bake-debug.log
```

### Log Files

Check log files for detailed error information:

```bash
# Local cache logs
cat .bake/logs/cache.log

# Recipe execution logs  
cat .bake/logs/recipes/*/execution.log
```

### Reset Everything

If all else fails, reset your Bake environment:

```bash
# Clear all caches
rm -rf .bake/cache

# Clear logs
rm -rf .bake/logs

# Reinstall bake
cargo install bake-cli --force

# Validate configuration from scratch
bake --validate
```

## Performance Optimization

### Profiling Slow Builds

```bash
# Time individual recipes
time bake frontend:build

# Profile with verbose output
bake --verbose | ts '[%Y-%m-%d %H:%M:%.S]'

# Use system profiling tools
perf record -g bake
perf report
```

### Optimizing Cache Usage

```bash
# Check cache hit rates
bake --stats

# Analyze cache sizes
du -sh .bake/cache/recipes/*

# Clean old cache entries
find .bake/cache -name "*.tar.zst" -mtime +7 -delete
```

## Related Documentation

- [Configuration Guide](configuration.md) - Complete configuration reference
- [Variables Guide](variables.md) - Variable system documentation
- [Caching Guide](caching.md) - Cache optimization strategies
- [Best Practices](best-practices.md) - Recommended patterns
- [CLI Commands](../reference/cli-commands.md) - Command reference