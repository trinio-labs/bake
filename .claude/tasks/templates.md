# Recipe Reuse Implementation Plan

## Overview
Implement a comprehensive recipe reuse system that allows recipes to be defined as reusable templates with parameters, eliminating duplication and enabling DRY project configurations.

## Task Progress Tracking

### ✅ Completed Tasks (Core Implementation)
- [x] Research current Bake architecture and recipe system
- [x] Design template system architecture
- [x] Create implementation plan
- [x] Create template definition structures and parsing logic (`src/project/recipe_template.rs`)
- [x] Add template directory creation to project setup (`.bake/templates/`)
- [x] Implement template discovery and loading system (`load_project_templates()`)
- [x] Extend variable context to support template parameters (`params` namespace)
- [x] Add template resolution to cookbook parsing (`resolve_template_recipes()`)
- [x] Update Recipe struct with template and parameters fields
- [x] Integrate template registry into BakeProject
- [x] Add template instantiation with parameter validation
- [x] Update .gitignore to allow .bake/templates/ in version control
- [x] Create test templates and cookbook for validation
- [x] Verify end-to-end template functionality

### 🚧 Next Phase Tasks
- [ ] Create template CLI commands (list, validate)
- [ ] Add comprehensive tests for template system
- [ ] Update documentation with template examples

### 🎯 Implementation Status
**Phase 1: COMPLETE** ✅  
Core template infrastructure is fully implemented and functional.

**Phase 2: PENDING** 📝  
CLI tooling and comprehensive testing.

### 🏗️ Implementation Summary
**Core Files Added/Modified:**
- `src/project/recipe_template.rs` - Template system core (350+ lines)
- `src/project/mod.rs` - Project integration with template loading/resolution
- `src/project/recipe.rs` - Extended Recipe struct with template support
- `.claude/tasks/templates.md` - This implementation plan
- `resources/tests/valid/.bake/templates/` - Test templates
- `resources/tests/valid/templates/cookbook.yml` - Test cookbook using templates

**Key Features Delivered:**
- Template definition with typed parameters (string, number, boolean, array, object)
- Parameter validation with defaults, required fields, and constraints
- Template discovery from `.bake/templates/` directories
- Template instantiation with parameter substitution using Handlebars
- Seamless integration with existing variable system (`{{ params.name }}`)
- Full backward compatibility with existing projects
- Comprehensive error handling and validation messages

**Tested Functionality:**
- Template loading from `.bake/templates/build-template.yml`
- Parameter resolution and validation
- Template instantiation into working recipes
- Recipe execution with template-generated commands
- Integration with existing dependency system

## Architecture Design

### 1. Recipe Templates System
- **Template Definitions**: Use `.bake/templates/` directory structure for storing reusable recipe templates
- **Template Parameters**: Support for typed parameters with defaults, validation, and documentation
- **Template Inheritance**: Allow templates to extend other templates
- **Backward Compatibility**: Existing cookbook.yml files work unchanged

### 2. Template Structure
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

### 3. Template Usage
```yaml
# cookbook.yml
name: "frontend"
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
      test_command: "npm test"
```

## Implementation Plan

### Phase 1: Core Infrastructure (Week 1-2)
1. **Template Discovery System**
   - Add template directory scanning to project loading (`.bake/templates/`)
   - Template file parsing and validation
   - Template metadata extraction

2. **Template Definition Format**
   - Define YAML schema for template files
   - Parameter type system (string, number, boolean, array, object)
   - Parameter validation and defaults
   - Template documentation fields

3. **Template Storage and Indexing**
   - Template registry for loaded templates
   - Template dependency resolution
   - Template inheritance chain validation

### Phase 2: Template Resolution Engine (Week 3-4)
1. **Parameter Substitution System**
   - Extend existing Handlebars system for template parameters
   - Add `{{ params.name }}` syntax alongside existing `{{ var.name }}`
   - Parameter type coercion and validation
   - Integration with existing variable hierarchy

2. **Recipe Instantiation**
   - Template resolution during cookbook parsing
   - Recipe generation from template + parameters
   - Template parameter validation
   - Error handling and reporting

3. **Template Inheritance**
   - Allow templates to extend other templates
   - Parameter inheritance and overrides
   - Template composition validation

### Phase 3: Advanced Features (Week 5-6)
1. **Template Libraries**
   - Global project templates in `.bake/templates/`
   - Cookbook-specific templates in `cookbook-name/.bake/templates/`
   - Template precedence and override rules

2. **Template Validation and Tooling**
   - `bake templates list` command
   - `bake templates validate` command
   - Template parameter documentation generation
   - Template usage analysis

3. **Enhanced Template Features**
   - Conditional template sections
   - Template loops for multiple recipe generation
   - Template composition and mixins

## File Structure Changes

```
project-root/
├── bake.yml
├── .bake/                     # Existing .bake directory
│   ├── logs/                  # Existing logs directory
│   ├── cache/                 # Existing cache directory
│   └── templates/             # New template directory
│       ├── build-template.yml
│       ├── test-template.yml
│       └── deploy-template.yml
├── frontend/
│   ├── cookbook.yml          # Uses templates
│   └── .bake/templates/      # Cookbook-specific templates
│       └── webpack-build.yml
└── backend/
    ├── cookbook.yml          # Uses templates
    └── .bake/templates/
        └── cargo-build.yml
```

## Code Changes Required

### New Files
1. `src/project/template.rs` - Template definition and loading
2. `src/project/template_engine.rs` - Template instantiation engine
3. `src/cli/template_commands.rs` - CLI commands for template management

### Modified Files
1. `src/project/cookbook.rs` - Add template resolution to recipe parsing
2. `src/project/mod.rs` - Integrate template loading into project loading
3. `src/template.rs` - Extend variable context for template parameters
4. `src/main.rs` - Add template CLI commands
5. `src/project/mod.rs` - Update `create_project_bake_dirs()` to create templates directory

### Template Parameter System

#### Parameter Types
- `string`: Text values with optional regex validation
- `number`: Integer or float values with min/max constraints
- `boolean`: True/false values
- `array`: Lists of values with type constraints for elements
- `object`: Key-value maps with schema validation

#### Parameter Validation
```yaml
parameters:
  port:
    type: number
    min: 1024
    max: 65535
    default: 3000
  environment:
    type: string
    pattern: "^(dev|staging|prod)$"
    required: true
  features:
    type: array
    items:
      type: string
    default: []
```

#### Template Inheritance
```yaml
# base-service.yml
name: "base-service"
parameters:
  service_name:
    type: string
    required: true
template:
  description: "{{ params.service_name }} service"
  run: |
    echo "Starting {{ params.service_name }}"

# web-service.yml
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

## Benefits
1. **DRY Principle**: Eliminate recipe duplication across cookbooks
2. **Consistency**: Standardized patterns for common tasks
3. **Maintainability**: Update templates to update all instances
4. **Flexibility**: Parameterize templates for different use cases
5. **Documentation**: Built-in parameter documentation
6. **Validation**: Type checking and parameter validation
7. **Integration**: Seamless integration with existing .bake directory structure

## Backward Compatibility
- Existing cookbook.yml files work unchanged
- Template system is opt-in
- No breaking changes to existing APIs
- Gradual migration path for existing projects
- Uses existing .bake directory structure

## Testing Strategy
1. Unit tests for template parsing and validation
2. Integration tests for template instantiation
3. End-to-end tests with real cookbook scenarios
4. Performance tests for template resolution overhead
5. Backward compatibility tests with existing projects

## Usage Examples

### Example 1: Build Template
```yaml
# .bake/templates/node-build.yml
name: "node-build"
description: "Standard Node.js build template"
parameters:
  build_script:
    type: string
    default: "build"
    description: "npm script to run for building"
  node_version:
    type: string
    default: "18"
    description: "Node.js version to use"
template:
  description: "Build Node.js application (v{{ params.node_version }})"
  cache:
    inputs: ["src/**/*", "package.json", "package-lock.json"]
    outputs: ["dist/**/*"]
  run: |
    node --version
    npm ci
    npm run {{ params.build_script }}
```

### Example 2: Test Template with Inheritance
```yaml
# .bake/templates/base-test.yml
name: "base-test"
parameters:
  test_framework:
    type: string
    required: true
template:
  description: "Run {{ params.test_framework }} tests"
  dependencies: ["build"]

# .bake/templates/jest-test.yml
name: "jest-test"
extends: "base-test"
parameters:
  test_framework:
    type: string
    default: "Jest"
  coverage:
    type: boolean
    default: false
template:
  run: |
    {% if params.coverage %}
    npm test -- --coverage
    {% else %}
    npm test
    {% endif %}
```

### Example 3: Usage in Cookbook
```yaml
# frontend/cookbook.yml
name: "frontend"
recipes:
  build:
    template: "node-build"
    parameters:
      build_script: "build:prod"
      node_version: "20"
  
  test:
    template: "jest-test"
    parameters:
      coverage: true
  
  deploy:
    template: "deploy-s3"
    parameters:
      bucket: "my-frontend-bucket"
      region: "us-east-1"
```

## Error Handling

### Template Validation Errors
- Missing required parameters
- Invalid parameter types
- Template not found
- Circular template inheritance
- Invalid template syntax

### Runtime Errors
- Parameter substitution failures
- Template instantiation errors
- Dependency resolution conflicts

This plan provides a comprehensive recipe reuse system that maintains Bake's philosophy while adding powerful DRY capabilities, properly integrated with the existing .bake directory structure.