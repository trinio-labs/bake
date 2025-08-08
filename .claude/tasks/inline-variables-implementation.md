# Inline Variables Implementation Plan

## Overview
Move from separate `vars.yml` files to inline `variables` and `overrides` fields in project/cookbook/recipe configuration files, with hierarchical handlebars template processing for variable blocks themselves.

## Variable Structure Format
```yaml
# Simple default variables (can use handlebars)
variables:
  service_name: api
  output_dir: "{{project.root}}/dist"
  full_name: "{{var.project_prefix}}-{{var.service_name}}"

# Environment-specific overrides (can use handlebars)
overrides:
  dev:
    output_dir: "{{project.root}}/dev-dist"
    debug: true
  prod:
    output_dir: "/opt/{{var.service_name}}"
    debug: false
```

## Implementation Steps

### Phase 1: Unify YAML Block Extraction

1. **Move `extract_yaml_block()` to `template.rs`**
   - Extract function from `recipe_template.rs:119-172`
   - Make it a public utility function
   - Update `recipe_template.rs` to use moved function
   - Add helper to extract multiple blocks: `extract_variables_blocks(content: &str) -> (Option<String>, Option<String>)`

### Phase 2: Update Struct Definitions

2. **Update Project, Cookbook, and Recipe Structs**
   - Remove `#[serde(skip)]` from variables field
   - Add `variables: IndexMap<String, serde_yaml::Value>` with `#[serde(default)]`
   - Add `overrides: BTreeMap<String, IndexMap<String, serde_yaml::Value>>` with `#[serde(default)]`
   - Keep existing `#[serde(skip)]` processed variables field for runtime use

### Phase 3: Hierarchical Template Processing for Variables

3. **Create Template-Aware Variable Processing Functions**
   ```rust
   // Renders variable blocks with hierarchical context, then parses as YAML
   fn process_variable_blocks(
       variables_block: Option<&str>,
       overrides_block: Option<&str>, 
       context: &VariableContext,
       build_environment: Option<&str>
   ) -> anyhow::Result<IndexMap<String, serde_yaml::Value>>
   ```

4. **Hierarchical Processing Flow**
   - **Project Level**: 
     - Extract `variables` and `overrides` blocks
     - Render with built-in constants only (`{{project.root}}`, `{{env.VAR}}`)
     - Parse rendered blocks as YAML
     - Resolve build environment overrides
   - **Cookbook Level**:
     - Extract cookbook `variables` and `overrides` blocks  
     - Render with built-ins + project variables (`{{project.root}}`, `{{var.project_var}}`)
     - Parse and resolve build environment overrides
     - Merge with project variables (cookbook takes precedence)
   - **Recipe Level**:
     - Extract recipe `variables` and `overrides` blocks
     - Render with built-ins + project + cookbook variables
     - Parse and resolve build environment overrides
     - Merge with project+cookbook (recipe takes precedence)

### Phase 4: Update Parsing Logic

5. **Update Project Parsing** (`project/mod.rs`)
   - Replace `initialize_project_variables()`:
     ```rust
     fn initialize_project_variables(&mut self, config_str: &str, build_environment: Option<&str>) -> anyhow::Result<()> {
         // Extract variables and overrides blocks from raw YAML
         let (vars_block, overrides_block) = extract_variables_blocks(config_str);
         
         // Build context with built-in constants only
         let context = VariableContext::with_project_constants(&self.root_path);
         
         // Process variable blocks with template rendering
         self.variables = process_variable_blocks(vars_block, overrides_block, &context, build_environment)?;
     }
     ```

6. **Update Cookbook Parsing** (`project/cookbook.rs`)
   - Update `from()` method with hierarchical processing:
     ```rust
     // Extract cookbook variable blocks
     let (cb_vars_block, cb_overrides_block) = extract_variables_blocks(&config_str);
     
     // Build context with built-ins + project variables
     let mut cb_context = context.clone(); // Contains project variables
     cb_context.merge(&VariableContext::with_project_constants(project_root));
     cb_context.merge(&VariableContext::with_cookbook_constants(path)?);
     
     // Process cookbook variables with template rendering
     let cb_variables = process_variable_blocks(cb_vars_block, cb_overrides_block, &cb_context, build_environment)?;
     
     // Render entire cookbook YAML with complete context
     let complete_context = build_context_with_variables(&cb_context, &cb_variables);
     let rendered_yaml = complete_context.render_raw_template(&config_str)?;
     
     // Parse final cookbook
     let cookbook: Cookbook = serde_yaml::from_str(&rendered_yaml)?;
     ```

7. **Update Recipe Processing**
   - Process recipe variables within cookbook parsing
   - Use built-ins + project + cookbook variables as context for recipe variable template rendering

### Phase 5: Multi-pass Parsing Implementation

8. **Complete Parsing Flow**
   ```rust
   fn parse_config<T>(
       config_str: &str,
       hierarchical_context: &VariableContext,
       build_environment: Option<&str>
   ) -> anyhow::Result<T> 
   where T: DeserializeOwned {
       // Step 1: Extract variable blocks as raw strings
       let (vars_block, overrides_block) = extract_variables_blocks(config_str);
       
       // Step 2: Render variable blocks with hierarchical context
       let processed_variables = process_variable_blocks(
           vars_block, overrides_block, hierarchical_context, build_environment
       )?;
       
       // Step 3: Build complete context with processed variables
       let mut complete_context = hierarchical_context.clone();
       complete_context.variables.extend(processed_variables);
       
       // Step 4: Render entire config with complete context
       let rendered_yaml = complete_context.render_raw_template(config_str)?;
       
       // Step 5: Parse final YAML into struct
       Ok(serde_yaml::from_str(&rendered_yaml)?)
   }
   ```

### Phase 6: Build Environment Resolution with Template Support

9. **Template-Aware Build Environment Resolution**
   ```rust
   fn resolve_variable_overrides(
       variables_block: Option<&str>,
       overrides_block: Option<&str>,
       context: &VariableContext,
       build_environment: Option<&str>
   ) -> anyhow::Result<IndexMap<String, serde_yaml::Value>> {
       // Render and parse default variables
       let mut result = if let Some(vars_str) = variables_block {
           let rendered_vars = context.render_raw_template(vars_str)?;
           serde_yaml::from_str(&rendered_vars)?
       } else {
           IndexMap::new()
       };
       
       // Render and apply build environment overrides
       if let (Some(env), Some(overrides_str)) = (build_environment, overrides_block) {
           let rendered_overrides = context.render_raw_template(overrides_str)?;
           let overrides_map: BTreeMap<String, IndexMap<String, serde_yaml::Value>> = 
               serde_yaml::from_str(&rendered_overrides)?;
           
           if let Some(build_env_overrides) = overrides_map.get(env) {
               result.extend(build_env_overrides.clone());
           }
       }
       
       Ok(result)
   }
   ```

### Phase 7: Cleanup and Testing

10. **Remove Legacy Code**
    - Remove `VariableFileLoader` struct entirely
    - Remove `variables_from_directory()` builder method  
    - Clean up all references to separate variable files

11. **Update Test Infrastructure**
    - Convert all `vars.yml` files to inline format
    - Test hierarchical template processing
    - Test `overrides` field with different build environments
    - Test handlebars in variable definitions

## Key Features

### Hierarchical Template Processing
Variables at each level can reference:
- Built-in constants (`{{project.root}}`, `{{cookbook.root}}`)
- Variables from higher levels (`{{var.parent_level_var}}`)
- Shell environment variables (`{{env.VAR}}`)

### Example Usage
**Project bake.yml:**
```yaml
name: my-app
variables:
  project_prefix: myapp
  base_dir: "{{project.root}}/build"
overrides:
  dev:
    base_dir: "{{project.root}}/dev-build"
```

**Cookbook cookbook.yml:**
```yaml
name: frontend
variables:
  service_name: "{{var.project_prefix}}-ui"
  output_dir: "{{var.base_dir}}/{{var.service_name}}"
overrides:
  dev:
    output_dir: "{{var.base_dir}}/dev-{{var.service_name}}"
```

**Recipe in cookbook:**
```yaml
recipes:
  build:
    variables:
      final_output: "{{var.output_dir}}/dist"
    overrides:
      prod:
        final_output: "/opt/{{var.service_name}}/dist"
    run: "npm run build --output {{var.final_output}}"
```

## Terminology Clarification
- **Build Environment**: The target environment for the build (dev, prod, staging, etc.) - used for variable overrides
- **Shell Environment Variables**: System environment variables accessible via `{{env.VAR}}` in templates
- **Variable Overrides**: Build environment-specific variable values defined in the `overrides` field

This approach provides powerful template-aware variable processing while maintaining clean separation of concerns and full handlebars support throughout the configuration hierarchy.