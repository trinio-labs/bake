# Bake JSON Schemas

This directory contains JSON schemas for Bake configuration files. These schemas enable IDE validation, autocompletion, and documentation for Bake projects.

## Available Schemas

### 1. Project Configuration Schema (`bake-project.schema.json`)
- **File**: `bake.yml` or `bake.yaml`
- **Description**: Main project configuration file
- **Schema URL**: `https://schemas.bake.sh/bake-project.schema.json`

**Validates**:
- Project name and description
- Global variables and environment settings
- Tool configuration (parallelism, fast-fail, verbose)
- Cache configuration (local, S3, GCS)
- Update settings
- Minimum bake version requirements

### 2. Cookbook Configuration Schema (`cookbook.schema.json`)
- **File**: `cookbook.yml` or `cookbook.yaml`
- **Description**: Cookbook files that define collections of recipes
- **Schema URL**: `https://schemas.bake.sh/cookbook.schema.json`

**Validates**:
- Cookbook name and metadata
- Recipe definitions with run commands or templates
- Recipe dependencies and cache configurations
- Variables and environment inheritance
- Template usage with parameters

### 3. Recipe Template Schema (`recipe-template.schema.json`)
- **File**: `.bake/templates/*.yml` or `.bake/templates/*.yaml`
- **Description**: Reusable recipe templates with parameters
- **Schema URL**: `https://schemas.bake.sh/recipe-template.schema.json`

**Validates**:
- Template name and description
- Parameter definitions with types and validation
- Template inheritance (`extends`)
- Template body with Handlebars variables
- Cache configuration with template variables

## IDE Integration

### VS Code
Add to your VS Code settings (`.vscode/settings.json`):

```json
{
  "yaml.schemas": {
    "./schemas/bake-project.schema.json": ["bake.yml", "bake.yaml"],
    "./schemas/cookbook.schema.json": ["**/cookbook.yml", "**/cookbook.yaml"],
    "./schemas/recipe-template.schema.json": [".bake/templates/*.yml", ".bake/templates/*.yaml"]
  }
}
```

### JetBrains IDEs (IntelliJ, WebStorm, etc.)
1. Go to **Settings** → **Languages & Frameworks** → **Schemas and DTDs** → **JSON Schema Mappings**
2. Add each schema with appropriate file patterns

### Neovim with LSP
For `yaml-language-server`, add to your configuration:

```lua
require('lspconfig').yamlls.setup({
  settings = {
    yaml = {
      schemas = {
        ["./schemas/bake-project.schema.json"] = {"bake.yml", "bake.yaml"},
        ["./schemas/cookbook.schema.json"] = {"**/cookbook.yml", "**/cookbook.yaml"},
        ["./schemas/recipe-template.schema.json"] = {".bake/templates/*.yml", ".bake/templates/*.yaml"}
      }
    }
  }
})
```

## Command Line Validation

### Using Python (recommended)
```bash
# Install dependencies
pip install jsonschema PyYAML

# Run validation tests
python3 schemas/test_schemas.py

# Validate individual files
python3 -c "
import json, yaml
from jsonschema import validate

# Load schema and file
with open('schemas/bake-project.schema.json') as f:
    schema = json.load(f)
with open('bake.yml') as f:
    data = yaml.safe_load(f)

# Validate
validate(instance=data, schema=schema)
print('✅ Valid!')
"
```

### Using AJV (Node.js)
```bash
# Install ajv-cli
npm install -g ajv-cli

# Convert YAML to JSON and validate
python3 -c "import yaml, json; print(json.dumps(yaml.safe_load(open('bake.yml'))))" | ajv validate -s schemas/bake-project.schema.json
```

## Schema Features

### Type Safety
- Strict typing for all configuration options
- Required field validation
- Format validation (e.g., version strings, file paths)

### Documentation
- Comprehensive descriptions for all fields
- Examples showing proper usage
- Default value documentation

### Template Support
- Handlebars template variable validation
- Parameter type checking with constraints
- Template inheritance validation

### Cache Configuration
- File pattern validation for inputs/outputs
- Support for template variables in cache patterns
- Validation of cache strategy ordering

## Testing

The schemas are tested against real Bake project files:

```bash
# Run all validation tests
python3 schemas/test_schemas.py

# Expected output:
# ✅ bake.yml is valid against bake-project.schema.json
# ✅ cookbook.yml is valid against cookbook.schema.json
# ✅ build-template.yml is valid against recipe-template.schema.json
# ✅ test-template.yml is valid against recipe-template.schema.json
# 🎉 All schema validations passed!
```

## Schema Evolution

These schemas follow semantic versioning and are designed to be:

- **Backward Compatible**: New schema versions won't break existing configurations
- **Forward Compatible**: Older configurations will work with newer schemas
- **Extensible**: Additional properties are generally allowed for future features

## Publishing to Schema Stores

These schemas are designed to be published to public schema stores:

### JSON Schema Store
- Submit to: https://github.com/SchemaStore/schemastore
- Enables automatic schema detection in many IDEs

### Custom Schema URLs
- Host at: `https://schemas.bake.sh/`
- CDN distribution for fast global access
- Versioned URLs for stability

## Examples

### Basic Project Configuration
```yaml
# bake.yml
name: "my-project"
description: "Example project"
variables:
  NODE_ENV: "production"
config:
  maxParallel: 4
  cache:
    local:
      enabled: true
    remotes:
      s3:
        bucket: "my-cache-bucket"
        region: "us-east-1"
```

### Cookbook with Templates
```yaml
# cookbook.yml
name: "frontend"
recipes:
  build:
    template: "build-template"
    parameters:
      language: "typescript"
      build_command: "npm run build:prod"
      cache_inputs: ["src/**/*.ts", "package.json"]
  
  test:
    description: "Run unit tests"
    run: "npm test"
    dependencies: ["build"]
```

### Recipe Template
```yaml
# .bake/templates/build-template.yml
name: "build-template"
description: "Generic build template"
parameters:
  language:
    type: string
    required: true
    pattern: "^(node|rust|go|python)$"
  build_command:
    type: string
    default: "npm run build"
template:
  description: "Build {{ params.language }} application"
  run: |
    echo "Building {{ params.language }}..."
    {{ params.build_command }}
```

## Contributing

When modifying schemas:

1. Update the schema file
2. Run validation tests: `python3 schemas/test_schemas.py`
3. Update examples and documentation
4. Test with real project files
5. Consider backward compatibility

## License

These schemas are part of the Bake project and follow the same license terms.