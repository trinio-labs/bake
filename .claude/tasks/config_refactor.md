# Configuration and Templating System Refactoring Plan

## Overview
Refactor Bake's configuration system to use dedicated variable files (`variables.yml` or `vars.yml`) with environment-specific sections, while maintaining full Handlebars templating support including flow controls and simplified recipe template rendering.

## Current System Analysis
- Variables defined inline in `bake.yml` and `cookbook.yml` files
- Hierarchy: project → cookbook → recipe → CLI overrides
- Handlebars templating with `{{var.NAME}}`, `{{env.VAR}}`, `{{project.root}}`, flow controls
- Recipe templates use `{{params.NAME}}` for template parameters during instantiation
- Template instantiation creates new recipes with resolved variables and parameters

## Target System Design

### Variable File Structure (Standardized)
```yaml
# Project level: vars.yml or variables.yml
default:
  api_url: "https://api.example.com"
  debug: false
  port: 8080
  timeout: 30

envs:
  dev:
    api_url: "https://dev-api.example.com"
    debug: true
    port: 3000
    # timeout inherited from defaults
  
  prod:
    api_url: "https://prod-api.example.com"
    port: 443
    # debug and timeout inherited from defaults
  
  staging:
    api_url: "https://staging-api.example.com"
    # all others inherited from defaults
```

**Key Features:**
- `default` section contains base values for all variables
- `envs` section contains environment-specific overrides
- Environment values inherit from defaults and override only specified keys
- Clean, structured format with clear inheritance hierarchy

### Recipe Template Integration (Simplified)
Recipe templates will have access to ONLY:
- Template parameters via `{{params.NAME}}`
- Built-in constants (`{{project.root}}`, `{{cookbook.root}}`)
- Full Handlebars flow controls (`{{#if}}`, `{{#each}}`, etc.)

**Explicitly excluded from recipe templates:**
- Project/cookbook variables (`{{var.NAME}}`) - to avoid confusion
- Environment variables (`{{env.VAR}}`) - to keep templates environment-agnostic

### Maintained Features
- Full Handlebars support (`{{#if}}`, `{{#unless}}`, `{{#each}}`, etc.) in cookbook definitions
- Variable hierarchy: project > cookbook > recipe > CLI overrides
- Built-in constants (`{{project.root}}`, `{{cookbook.root}}`)
- Recipe template parameters (`{{params.NAME}}`) during instantiation
- Environment variable access (`{{env.VAR}}`) in cookbook/recipe definitions

## Implementation Progress

### ✅ Phase 0: Planning and Setup
- [x] Research current system implementation
- [x] Design new variable system with environment support
- [x] Plan recipe template simplification
- [x] Save plan to .claude/tasks/config_refactor.md

### ✅ Phase 1: Variable Loading System (`src/template.rs`)
- [x] Add `VariableFileLoader` struct with environment support
- [x] Add `load_variables_from_file()` function that handles environment sections
- [x] Add environment resolution logic to `VariableContext`
- [x] Update `VariableContext::builder()` to support environment parameter
- [x] Maintain existing template processing capabilities
- [x] Add comprehensive tests for variable file loading
- [x] **UPDATED**: Standardized format with `default` + `envs.{environment}` structure
- [x] **UPDATED**: Proper environment inheritance (defaults + overrides)
- [x] **UPDATED**: Enhanced error handling and logging for missing environments

### ✅ Phase 2: Project Configuration (`src/project/mod.rs`)
- [x] Modify `BakeProject::from()` to accept environment parameter
- [x] Update `initialize_project_variables()` to use environment-aware loading  
- [x] Update all test calls to include environment parameter
- [x] Temporarily fix main.rs to compile with default environment
- [x] **FIXED**: Updated test project variable files to use standardized schema
- [x] **VERIFIED**: Project loading and execution works with new variable files

### ✅ **Updated Test Files**: 
- `resources/tests/valid/vars.yml`: Project-level variables with test/dev/prod environments
- `resources/tests/valid/foo/vars.yml`: Cookbook-level variables with default/dev/test environments

### ⏳ Phase 3: Cookbook Configuration (`src/project/cookbook.rs`)
- [ ] Update `Cookbook::from()` to load cookbook-level `vars.yml`/`variables.yml`
- [ ] Remove inline `variables:` field processing from `cookbook.yml` 
- [ ] Integrate cookbook variables with project environment context
- [ ] Update variable resolution hierarchy to maintain cookbook → recipe inheritance
- [ ] Keep cookbook definitions able to use `{{var.NAME}}` and `{{env.VAR}}`

### ⏳ Phase 4: Recipe Template Integration (`src/project/recipe_template.rs`)
- [ ] **Simplify** `instantiate()` method to use limited template context:
  - [ ] Template parameters as `{{params.NAME}}`
  - [ ] Built-in constants (`{{project.root}}`, `{{cookbook.root}}`)
  - [ ] **Remove** access to project/cookbook variables and environment variables
- [ ] Create separate template context for recipe templates (minimal)
- [ ] Verify handlebars flow controls work with template parameters only
- [ ] Test template instantiation with various parameter configurations

### ⏳ Phase 5: Recipe Processing (`src/project/mod.rs` - `resolve_template_recipes()`)
- [ ] Update template resolution to use simplified template context
- [ ] After template instantiation, apply environment-resolved variables to resulting recipe
- [ ] Ensure instantiated recipes receive environment variables through normal recipe processing
- [ ] Maintain parameter override capabilities for recipe-specific customization

### ⏳ Phase 6: CLI Integration (`src/main.rs`)
- [ ] Add `--env` flag for environment selection (defaults to "default")
- [ ] Update argument parsing with clap
- [ ] Pass environment parameter through execution chain
- [ ] Maintain `--var key=value` override functionality (highest priority)

### ⏳ Phase 7: Configuration Schema Updates
- [ ] Remove `variables` fields from `BakeProject` and `Cookbook` structs
- [ ] Update deserialization to ignore deprecated `variables` sections
- [ ] Update validation to check for variable files instead
- [ ] Maintain backward compatibility for a deprecation period if needed

### ⏳ Phase 8: Testing & Examples
- [ ] Update test projects to use new variable file format
- [ ] Test recipe template instantiation with template parameters only
- [ ] Verify handlebars flow controls work with template parameters in templates
- [ ] Test variable hierarchy: project env vars → cookbook env vars → recipe vars → CLI overrides
- [ ] Test that instantiated recipes properly inherit environment-resolved variables after instantiation
- [ ] Update documentation and examples

## File Changes Summary

**Modified Files:**
- `src/template.rs` - Add variable file loading and environment support
- `src/project/mod.rs` - Update project loading and template resolution logic  
- `src/project/cookbook.rs` - Update cookbook loading logic
- `src/project/recipe_template.rs` - Simplify template context (params + built-ins only)
- `src/main.rs` - Add CLI environment flag
- Test files in `resources/tests/` - Convert to new format
- `.claude/tasks/config_refactor.md` - Track implementation progress

**Removed Features:**
- Inline `variables:` sections in `bake.yml` and `cookbook.yml`
- Direct variable processing in configuration structs
- Variable access in recipe templates (simplified to params + built-ins only)

**New Features:**
- Environment-specific variable files (`vars.yml`, `variables.yml`)
- `--env` CLI flag for environment selection
- Environment-aware variable resolution
- Simplified recipe templates (parameters and built-ins only)

## Variable Resolution Flow with Recipe Templates

1. **Load Environment Variables**: Load vars.yml files with environment selection
2. **Project Variables**: Resolve project-level environment variables
3. **Cookbook Variables**: Resolve cookbook-level environment variables (inherit from project)
4. **Recipe Template Instantiation**: Templates access ONLY:
   - `{{params.NAME}}` - Template parameters from recipe definition
   - `{{project.root}}`, `{{cookbook.root}}` - Built-in constants
   - Handlebars flow controls
5. **Post-Instantiation**: Apply environment-resolved variables to instantiated recipe
6. **Recipe Variables**: Recipe-specific variables override template variables
7. **CLI Overrides**: `--var` flags override everything

## Risk Mitigation
- No backward compatibility required per user request
- Comprehensive test coverage for variable resolution and simplified template instantiation
- Maintain all existing templating capabilities including flow controls
- Preserve variable hierarchy and override behavior
- Simplified recipe template context reduces confusion and complexity

This refactoring will provide cleaner environment management and simpler recipe templates while preserving all necessary templating functionality.