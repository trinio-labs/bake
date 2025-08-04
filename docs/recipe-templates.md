# Recipe Templates

Recipe templates provide a powerful way to eliminate duplication and standardize common patterns across your Bake projects. Instead of repeating similar recipe definitions in multiple cookbooks, you can create reusable templates with parameters.

## Overview

Recipe templates allow you to:
- **Eliminate duplication** by defining common patterns once
- **Standardize workflows** across teams and projects
- **Parameterize recipes** for different use cases
- **Maintain consistency** while allowing customization
- **Type-safe parameters** with validation and defaults

## Template Location

Templates are stored in the `.bake/templates/` directory within your project:

```
project-root/
├── bake.yml
├── .bake/
│   └── templates/
│       ├── build-template.yml
│       ├── test-template.yml
│       └── deploy-template.yml
└── frontend/
    └── cookbook.yml
```

## Template Structure

A recipe template consists of three main sections:

### 1. Template Metadata

```yaml
name: "build-template"
description: "Generic build template for various languages"
extends: "base-template"  # Optional: inherit from another template
```

### 2. Parameters Definition

```yaml
parameters:
  language:
    type: string
    required: true
    description: "Programming language (node, rust, go, etc.)"
  
  build_command:
    type: string
    default: "npm run build"
    description: "Command to run for building"
    
  port:
    type: number
    default: 3000
    min: 1024
    max: 65535
    description: "Server port number"
    
  features:
    type: array
    default: []
    items:
      type: string
    description: "List of features to enable"
    
  debug:
    type: boolean
    default: false
    description: "Enable debug mode"
```

### 3. Template Definition

```yaml
template:
  description: "Build {{ params.language }} application"
  cache:
    inputs: ["src/**/*", "package.json"]
    outputs: ["dist/**/*", "build/**/*"]
  dependencies: ["install"]
  environment:
    - "NODE_ENV"
  variables:
    DEBUG: "{{ params.debug }}"
  run: |
    echo "Building {{ params.language }} application..."
    {{ params.build_command }}
    {% if params.debug %}echo "Debug mode enabled"{% endif %}
```

## Parameter Types

### String Parameters

```yaml
service_name:
  type: string
  required: true
  pattern: "^[a-z][a-z0-9-]+$"  # Optional regex validation
  description: "Service name (lowercase, alphanumeric with hyphens)"
```

### Number Parameters

```yaml
port:
  type: number
  default: 3000
  min: 1024
  max: 65535
  description: "Port number for the service"
```

### Boolean Parameters

```yaml
enable_ssl:
  type: boolean
  default: false
  description: "Enable SSL/TLS encryption"
```

### Array Parameters

```yaml
build_targets:
  type: array
  default: ["production"]
  items:
    type: string
  description: "List of build targets"
```

### Object Parameters

```yaml
database_config:
  type: object
  default:
    host: "localhost"
    port: 5432
  description: "Database configuration object"
```

## Using Templates in Recipes

To use a template in your cookbook, reference it in a recipe:

```yaml
# cookbook.yml
name: "frontend"
recipes:
  build:
    template: "build-template"
    parameters:
      language: "typescript"
      build_command: "npm run build:prod"
      debug: true
    # Additional recipe properties can still be specified
    variables:
      EXTRA_VAR: "value"
    environment:
      - "CUSTOM_ENV"
```

## Template Parameter Access

Within templates, access parameters using the `{{ params.name }}` syntax:

```yaml
template:
  description: "Deploy {{ params.service_name }} to {{ params.environment }}"
  run: |
    echo "Deploying {{ params.service_name }}"
    {% if params.environment == "production" %}
    echo "Production deployment - extra validation"
    ./validate-production.sh
    {% endif %}
    ./deploy.sh --service={{ params.service_name }} --env={{ params.environment }}
```

## Template Inheritance

Templates can extend other templates using the `extends` field:

```yaml
# .bake/templates/base-service.yml
name: "base-service"
parameters:
  service_name:
    type: string
    required: true
template:
  description: "{{ params.service_name }} service"
  run: |
    echo "Starting {{ params.service_name }}"

# .bake/templates/web-service.yml
name: "web-service"
extends: "base-service"
parameters:
  port:
    type: number
    default: 3000
template:
  run: |
    echo "Starting {{ params.service_name }} on port {{ params.port }}"
    npm start
```

## Complete Template Examples

### Build Template

```yaml
# .bake/templates/build-template.yml
name: "build-template"
description: "Generic build template for various languages"
parameters:
  language:
    type: string
    required: true
    description: "Programming language (node, rust, go, etc.)"
  build_command:
    type: string
    default: "npm run build"
    description: "Command to run for building"
  cache_inputs:
    type: array
    default: ["src/**/*", "package.json"]
    description: "Input files to cache"
template:
  description: "Build {{ params.language }} application"
  cache:
    inputs: "{{ params.cache_inputs }}"
    outputs: ["dist/**/*", "build/**/*"]
  run: |
    echo "Building {{ params.language }} application..."
    {{ params.build_command }}
```

### Test Template

```yaml
# .bake/templates/test-template.yml
name: "test-template"
description: "Generic test template"
parameters:
  test_command:
    type: string
    default: "npm test"
    description: "Command to run tests"
  coverage:
    type: boolean
    default: false
    description: "Enable coverage reporting"
template:
  description: "Run tests with {{ params.test_command }}"
  dependencies: ["build"]
  run: |
    echo "Running tests..."
    {% if params.coverage %}echo "Coverage enabled"{% endif %}
    {{ params.test_command }}
```

## Using Templates in Cookbooks

```yaml
# cookbook.yml
name: "my-project"
recipes:
  build:
    template: "build-template"
    parameters:
      language: "typescript"
      build_command: "npm run build:prod"
      cache_inputs: ["src/**/*.ts", "package.json", "tsconfig.json"]
  
  test:
    template: "test-template"
    parameters:
      test_command: "npm run test:ci"
      coverage: true
  
  # Regular recipe without template
  deploy:
    description: "Deploy the application"
    run: |
      echo "Deploying application..."
      ./deploy.sh
    dependencies: ["build", "test"]
```

## Variable Integration

Templates work seamlessly with Bake's existing variable system. You can use:

- **Template parameters**: `{{ params.name }}`
- **Project variables**: `{{ var.name }}`
- **Environment variables**: `{{ env.NAME }}`
- **Built-in constants**: `{{ project.root }}`, `{{ cookbook.root }}`

```yaml
template:
  run: |
    cd {{ project.root }}
    export SERVICE_NAME={{ params.service_name }}
    export DEBUG={{ var.debug_mode }}
    ./scripts/deploy.sh
```

## Error Handling

The template system provides comprehensive validation and error reporting:

### Parameter Validation Errors
- Missing required parameters
- Type mismatches (string vs number)
- Values outside min/max ranges
- Arrays with wrong item types
- Pattern validation failures

### Template Resolution Errors
- Template not found
- Circular inheritance
- Invalid template syntax
- Parameter substitution failures

### Example Error Messages

```
Recipe Template Validation: Required parameter 'service_name' is missing for template 'deploy-template'

Recipe Template Validation: Parameter 'port' value 99999 is greater than maximum 65535

Template Resolution: Template 'unknown-template' used by recipe 'frontend:build' was not found. Available templates: build-template, test-template, deploy-template
```

## Best Practices

### 1. Use Descriptive Parameter Names
```yaml
# Good
parameters:
  service_name:
    type: string
    description: "Name of the service to deploy"

# Avoid
parameters:
  name:
    type: string
```

### 2. Provide Sensible Defaults
```yaml
parameters:
  build_command:
    type: string
    default: "npm run build"
    description: "Build command to execute"
```

### 3. Add Parameter Validation
```yaml
parameters:
  environment:
    type: string
    pattern: "^(dev|staging|prod)$"
    description: "Deployment environment"
```

### 4. Document Your Templates
```yaml
name: "build-template"
description: "Standard build template for Node.js applications with TypeScript support"
parameters:
  node_version:
    type: string
    default: "18"
    description: "Node.js version to use (major version number)"
```

### 5. Use Template Inheritance Wisely
```yaml
# Base template for common service patterns
name: "base-service"
# Specific templates that extend the base
name: "web-service"
extends: "base-service"
```

### 6. Organize Templates by Purpose
```
.bake/templates/
├── build/
│   ├── node-build.yml
│   ├── rust-build.yml
│   └── go-build.yml
├── test/
│   ├── unit-test.yml
│   └── integration-test.yml
└── deploy/
    ├── docker-deploy.yml
    └── k8s-deploy.yml
```

## Troubleshooting

### Template Not Found
Ensure your template files are in `.bake/templates/` and have `.yml` or `.yaml` extensions.

### Parameter Validation Failures
Check that all required parameters are provided and match the expected types.

### Template Syntax Errors
Validate your YAML syntax and ensure template expressions are properly formatted.

### Variable Resolution Issues
Remember that template parameters use `{{ params.name }}` while project variables use `{{ var.name }}`.

## Migration from Duplicated Recipes

To migrate existing duplicated recipes to templates:

1. **Identify common patterns** across your cookbooks
2. **Extract parameters** that vary between instances
3. **Create a template** with appropriate parameter definitions
4. **Update cookbook recipes** to use the template
5. **Test thoroughly** to ensure behavior is preserved

## Advanced Features

### Conditional Logic in Templates
```yaml
template:
  run: |
    {% if params.environment == "production" %}
    echo "Production build"
    npm run build:prod
    {% else %}
    echo "Development build"
    npm run build:dev
    {% endif %}
```

### Array Parameters with Loops
```yaml
parameters:
  build_targets:
    type: array
    default: ["web", "mobile"]
template:
  run: |
    {% for target in params.build_targets %}
    echo "Building {{ target }}"
    npm run build:{{ target }}
    {% endfor %}
```

### Complex Parameter Validation
```yaml
parameters:
  database_url:
    type: string
    pattern: "^(postgresql|mysql)://.*"
    description: "Database connection URL"
  replicas:
    type: number
    min: 1
    max: 10
    default: 3
```

The recipe template system provides a powerful way to eliminate duplication while maintaining the flexibility and type safety that makes Bake projects maintainable and reliable.