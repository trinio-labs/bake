use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use crate::{
    project::Recipe,
    template::{extract_variables_blocks, process_variable_blocks, VariableContext},
};
use anyhow::bail;
use ignore::WalkBuilder;
use indexmap::IndexMap;
use log::debug;
use serde::Deserialize;
use serde_json::{json, Value as JsonValue};

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Cookbook {
    pub name: String,

    #[serde(default)]
    pub environment: Vec<String>,

    /// Tags for filtering recipes (e.g., "frontend", "backend", "api")
    #[serde(default)]
    pub tags: Vec<String>,

    /// Cookbook-level variables
    #[serde(default)]
    pub variables: IndexMap<String, serde_yaml::Value>,

    /// Environment-specific variable overrides for this cookbook
    #[serde(default)]
    pub overrides: BTreeMap<String, IndexMap<String, serde_yaml::Value>>,

    /// Processed variables for runtime use (combines variables + overrides)
    #[serde(skip)]
    pub processed_variables: IndexMap<String, serde_yaml::Value>,

    pub recipes: BTreeMap<String, Recipe>,

    #[serde(skip)]
    pub config_path: PathBuf,

    /// Tracks whether this cookbook has been fully loaded with Handlebars rendering
    /// false = minimal loading (only name, dependencies, tags)
    /// true = full loading (all fields rendered with Handlebars)
    #[serde(skip)]
    pub fully_loaded: bool,
}
impl Cookbook {
    /// Generates builtin constants for the cookbook context
    pub fn generate_cookbook_constants(
        cookbook_path: &Path,
    ) -> anyhow::Result<IndexMap<String, JsonValue>> {
        let cookbook_constants = json!({
            "root": cookbook_path
                .parent()
                .ok_or_else(|| anyhow::anyhow!("Cookbook path has no parent directory"))?
                .display()
                .to_string()
        });
        Ok(IndexMap::from([(
            "cookbook".to_owned(),
            cookbook_constants,
        )]))
    }

    /// Creates a minimally loaded cookbook (no Handlebars rendering)
    /// Only loads essential fields: name, recipe names, dependencies, and tags
    /// Used for building the dependency graph before determining which recipes to execute
    ///
    /// # Arguments
    /// * `path` - Path to a cookbook file
    /// * `project_root` - Path to the project root
    ///
    pub fn from_minimal(path: &PathBuf, project_root: &Path) -> anyhow::Result<Self> {
        let config_str = match std::fs::read_to_string(path) {
            Ok(contents) => contents,
            Err(_) => bail!("Cookbook: Failed to read cookbook configuration file at '{}': Check file existence and permissions.", path.display()),
        };

        // Parse as serde_yaml::Value first to handle template expressions in non-essential fields
        let mut yaml_value: serde_yaml::Value = serde_yaml::from_str(&config_str)
            .map_err(|e| anyhow::anyhow!("Cookbook: Failed to parse YAML at '{}': {}. Check YAML syntax.", path.display(), e))?;

        // Extract only the fields we need for minimal loading: name, tags, recipes (with just names, tags, and dependencies)
        if let serde_yaml::Value::Mapping(ref mut map) = yaml_value {
            // Keep only essential top-level fields
            let essential_keys: Vec<&str> = vec!["name", "tags", "environment", "recipes"];
            map.retain(|k, _| {
                k.as_str().is_some_and(|key| essential_keys.contains(&key))
            });

            // For each recipe, keep only essential fields
            if let Some(serde_yaml::Value::Mapping(recipes_map)) = map.get_mut("recipes") {
                for (_, recipe_value) in recipes_map.iter_mut() {
                    if let serde_yaml::Value::Mapping(recipe_map) = recipe_value {
                        // Keep only: dependencies, tags, description (for debugging)
                        let essential_recipe_keys: Vec<&str> = vec!["dependencies", "tags", "description"];
                        recipe_map.retain(|k, _| {
                            k.as_str().is_some_and(|key| essential_recipe_keys.contains(&key))
                        });
                    }
                }
            }
        }

        // Now deserialize the filtered YAML into our struct
        let mut parsed: Self = serde_yaml::from_value(yaml_value)
            .map_err(|e| anyhow::anyhow!("Cookbook: Failed to deserialize minimal YAML at '{}': {}.", path.display(), e))?;

        // Set cookbook metadata
        parsed.config_path = path.to_path_buf();
        parsed.fully_loaded = false; // Mark as minimally loaded

        // Process each recipe with minimal data
        for (name, recipe) in parsed.recipes.iter_mut() {
            recipe.name = name.clone();
            recipe.cookbook = parsed.name.clone();
            recipe.config_path = path.to_path_buf();
            recipe.project_root = project_root.to_path_buf();

            // Inherit tags from cookbook if recipe has no tags
            if recipe.tags.is_empty() {
                recipe.tags = parsed.tags.clone();
            }

            // Process dependencies (add cookbook prefix if needed)
            if let Some(dependencies) = recipe.dependencies.as_ref() {
                let new_deps = dependencies.iter().map(|dep| {
                    if !dep.contains(':') {
                        recipe.cookbook.clone() + ":" + dep
                    } else {
                        dep.clone()
                    }
                });
                recipe.dependencies = Some(new_deps.collect());
            }
        }

        Ok(parsed)
    }

    /// Creates a cookbook config from a path to a cookbook file
    /// Fully loads with complete Handlebars rendering
    ///
    /// # Arguments
    /// * `path` - Path to a cookbook file
    /// * `project_root` - Path to the project root
    /// * `environment` - Environment name for variable loading (e.g., "dev", "prod", "default")
    /// * `context` - Variable context containing environment, variables, and overrides
    ///
    pub fn from(
        path: &PathBuf,
        project_root: &Path,
        environment: Option<&str>,
        context: &VariableContext,
    ) -> anyhow::Result<Self> {
        let config_str = match std::fs::read_to_string(path) {
            Ok(contents) => contents,
            Err(_) => bail!("Cookbook: Failed to read cookbook configuration file at '{}': Check file existence and permissions.", path.display()),
        };

        // Build hierarchical context for cookbook processing
        let mut cookbook_context = context.clone(); // Contains project variables and constants

        // Add cookbook constants to the context
        let cookbook_constants = Self::generate_cookbook_constants(path)?;
        cookbook_context.constants.extend(cookbook_constants);

        // Set working directory to cookbook directory for helper execution
        cookbook_context.working_directory = Some(
            path.parent()
                .ok_or_else(|| anyhow::anyhow!("Cookbook path has no parent directory"))?
                .to_path_buf(),
        );

        // Extract cookbook variable blocks from raw YAML
        let (cb_vars_block, cb_overrides_block) = extract_variables_blocks(&config_str);

        // Process cookbook variables with hierarchical context (project + built-ins)
        let cookbook_processed_variables = process_variable_blocks(
            cb_vars_block.as_deref(),
            cb_overrides_block.as_deref(),
            &cookbook_context,
            environment,
        )?;

        // Build complete context with cookbook variables for rendering entire config
        cookbook_context
            .variables
            .extend(cookbook_processed_variables.clone());

        // Render entire cookbook YAML with complete context
        let rendered_yaml = cookbook_context.render_raw_template(&config_str)?;

        // Parse rendered YAML into cookbook struct
        let mut parsed: Self = serde_yaml::from_str(&rendered_yaml)
            .map_err(|e| anyhow::anyhow!("Cookbook: Failed to parse rendered cookbook YAML at '{}': {}. Check YAML syntax and template variable usage.", path.display(), e))?;

        // Set cookbook metadata
        parsed.config_path = path.to_path_buf();
        parsed.processed_variables = cookbook_processed_variables.clone();
        parsed.fully_loaded = true; // Mark as fully loaded

        // Inherit environment from project
        let mut cookbook_environment = context.environment.clone();
        cookbook_environment.extend(parsed.environment.iter().cloned());
        parsed.environment = cookbook_environment;

        // Process each recipe
        for (name, recipe) in parsed.recipes.iter_mut() {
            recipe.name = name.clone();
            recipe.cookbook = parsed.name.clone();
            recipe.config_path = path.to_path_buf();
            recipe.project_root = project_root.to_path_buf();

            // Inherit environment from cookbook
            let mut recipe_environment = parsed.environment.clone();
            recipe_environment.extend(recipe.environment.iter().cloned());
            recipe.environment = recipe_environment.clone();

            // Inherit tags from cookbook if recipe has no tags
            if recipe.tags.is_empty() {
                recipe.tags = parsed.tags.clone();
            }

            // Build recipe context with project + cookbook variables
            let mut recipe_context = cookbook_context.clone();
            recipe_context.environment = recipe_environment.clone();

            // Process recipe-level variables if they exist
            let recipe_processed_variables =
                if recipe.variables.is_empty() && recipe.overrides.is_empty() {
                    // No recipe variables, inherit from cookbook
                    cookbook_processed_variables.clone()
                } else {
                    // Start with cookbook variables and add recipe variables (recipe takes precedence)
                    let mut combined = cookbook_processed_variables.clone();
                    combined.extend(recipe.variables.clone());

                    // Apply environment overrides if specified
                    if let Some(env) = environment {
                        if let Some(env_overrides) = recipe.overrides.get(env) {
                            combined.extend(env_overrides.clone());
                        }
                    }

                    combined
                };

            recipe.processed_variables = recipe_processed_variables.clone();

            // Process recipe run command with complete variable context
            let mut run_context = recipe_context.clone();
            run_context.variables = recipe_processed_variables;
            recipe.run = run_context.parse_template(&recipe.run)?;

            // Process dependencies (add cookbook prefix if needed)
            if let Some(dependencies) = recipe.dependencies.as_ref() {
                let new_deps = dependencies.iter().map(|dep| {
                    if !dep.contains(':') {
                        recipe.cookbook.clone() + ":" + dep
                    } else {
                        dep.clone()
                    }
                });
                recipe.dependencies = Some(new_deps.collect());
            }
        }

        Ok(parsed)
    }

    /// Gets all cookbooks recursively in a directory with minimal loading (no Handlebars)
    ///
    /// Recursively searches for all cookbooks in a directory respecting `.gitignore` and
    /// `.bakeignore` files, but only performs minimal parsing without template rendering.
    ///
    /// # Arguments
    /// * `path` - Path to a directory
    ///
    pub fn map_from_minimal(path: &PathBuf) -> anyhow::Result<BTreeMap<String, Self>> {
        let all_files = WalkBuilder::new(path)
            .add_custom_ignore_filename(".bakeignore")
            .build();
        all_files
            .filter_map(|x| match x {
                Ok(file) => {
                    let filename = match file.file_name().to_str() {
                        Some(name) => name,
                        None => return None, // Skip files with invalid UTF-8 names
                    };
                    if filename.contains("cookbook.yaml") || filename.contains("cookbook.yml") {
                        match Self::from_minimal(&file.into_path(), path) {
                            Ok(cookbook) => Some(Ok((cookbook.name.clone(), cookbook))),
                            Err(err) => Some(Err(err)),
                        }
                    } else {
                        None
                    }
                }
                Err(_) => {
                    debug!("Ignored file: {}", x.unwrap_err());
                    None
                }
            })
            .collect()
    }

    /// Gets all cookbooks recursively in a directory with full Handlebars rendering
    ///
    /// map_from recursively searches for all cookbooks in a directory respecting `.gitignore` and
    /// `.bakeignore` files
    ///
    /// # Arguments
    /// * `path` - Path to a directory
    /// * `environment` - Environment name for variable loading (e.g., "dev", "prod", "default")
    /// * `context` - Variable context containing environment, variables, and overrides
    ///
    pub fn map_from(
        path: &PathBuf,
        environment: Option<&str>,
        context: &VariableContext,
    ) -> anyhow::Result<BTreeMap<String, Self>> {
        let all_files = WalkBuilder::new(path)
            .add_custom_ignore_filename(".bakeignore")
            .build();
        all_files
            .filter_map(|x| match x {
                Ok(file) => {
                    let filename = match file.file_name().to_str() {
                        Some(name) => name,
                        None => return None, // Skip files with invalid UTF-8 names
                    };
                    if filename.contains("cookbook.yaml") || filename.contains("cookbook.yml") {
                        match Self::from(&file.into_path(), path, environment, context) {
                            Ok(cookbook) => Some(Ok((cookbook.name.clone(), cookbook))),
                            Err(err) => Some(Err(err)),
                        }
                    } else {
                        None
                    }
                }
                Err(_) => {
                    debug!("Ignored file: {}", x.unwrap_err());
                    None
                }
            })
            .collect()
    }
}
#[cfg(test)]
mod test {
    use std::{collections::BTreeMap, path::PathBuf};

    use crate::{project::Helper, template::VariableContext};
    use indexmap::IndexMap;
    use test_case::test_case;

    fn config_path(path_str: &str) -> String {
        env!("CARGO_MANIFEST_DIR").to_owned() + "/resources/tests" + path_str
    }

    /// Loads helpers from the test project's .bake/helpers directory
    fn load_test_helpers(project_root: &str) -> Vec<Helper> {
        let helpers_path = PathBuf::from(project_root).join(".bake").join("helpers");

        if !helpers_path.exists() {
            return vec![];
        }

        std::fs::read_dir(&helpers_path)
            .ok()
            .into_iter()
            .flatten()
            .filter_map(|entry| {
                let entry = entry.ok()?;
                let path = entry.path();
                if path.is_file() && (path.extension()? == "yml" || path.extension()? == "yaml") {
                    Helper::from_file(&path).ok()
                } else {
                    None
                }
            })
            .collect()
    }

    fn validate_cookbook_foo(actual: anyhow::Result<super::Cookbook>) {
        assert_eq!(actual.as_ref().unwrap().name, "foo");
    }

    fn validate_cookbook_vec(actual: anyhow::Result<BTreeMap<String, super::Cookbook>>) {
        assert_eq!(actual.unwrap().len(), 6)
    }

    #[test_case(config_path("/valid/"), config_path("/valid/foo/cookbook.yml") => using validate_cookbook_foo; "Valid cookbook file")]
    #[test_case(config_path("/valid/"),config_path("/invalid/config/cookbook.yml") => matches Err(_); "Invalid cookbook file")]
    #[test_case(config_path("/valid/"), config_path("/invalid/config") => matches Err(_); "Cant read directory")]
    fn read_cookbook(project_root: String, path_str: String) -> anyhow::Result<super::Cookbook> {
        let helpers = load_test_helpers(&project_root);

        super::Cookbook::from(
            &PathBuf::from(path_str),
            &PathBuf::from(project_root),
            Some("default"),
            &VariableContext::builder()
                .environment(vec![])
                .variables(IndexMap::new())
                .overrides(IndexMap::new())
                .helpers(helpers)
                .build(),
        )
    }

    #[test_case(config_path("/valid/") => using validate_cookbook_vec; "Root dir")]
    #[test_case(config_path("/invalid/config") => matches Err(_); "Invalid dir")]
    fn read_all_cookbooks(path_str: String) -> anyhow::Result<BTreeMap<String, super::Cookbook>> {
        let helpers = load_test_helpers(&path_str);

        super::Cookbook::map_from(
            &PathBuf::from(path_str),
            Some("default"),
            &VariableContext::builder()
                .environment(vec![])
                .variables(IndexMap::new())
                .overrides(IndexMap::new())
                .helpers(helpers)
                .build(),
        )
    }

    #[test]
    fn test_yaml_type_preservation() {
        use crate::template::VariableContext;
        use serde_yaml::Value;

        // Create a YAML value with mixed types
        let yaml_str = r#"
name: test-cookbook
variables:
  force_build: false
  max_parallel: 4
  debug_enabled: true
  cache_path: "/tmp/cache"
  template_value: "{{ var.force_build }}"
  template_number: "{{ var.max_parallel }}"
  template_bool: "{{ var.debug_enabled }}"
recipes:
  build:
    run: echo "building"
    force_build: "{{ var.force_build }}"
    max_workers: "{{ var.max_parallel }}"
    debug: "{{ var.debug_enabled }}"
"#;

        let mut yaml_value: Value = serde_yaml::from_str(yaml_str).unwrap();
        let _my_str = "str".to_owned();

        // Create a context with the variables
        let variables = IndexMap::from([
            ("force_build".to_owned(), serde_yaml::Value::Bool(false)),
            (
                "max_parallel".to_owned(),
                serde_yaml::Value::Number(serde_yaml::Number::from(4)),
            ),
            ("debug_enabled".to_owned(), serde_yaml::Value::Bool(true)),
        ]);

        let context = VariableContext::builder().variables(variables).build();

        // Process the template
        VariableContext::process_template_in_value(&mut yaml_value, &context, true).unwrap();

        // Check that the processed values have the correct types
        if let Value::Mapping(map) = &yaml_value {
            if let Some(Value::Mapping(recipes)) = map.get("recipes") {
                if let Some(Value::Mapping(build_recipe)) = recipes.get("build") {
                    // These should be converted back to their original types
                    assert!(matches!(
                        build_recipe.get("force_build"),
                        Some(Value::Bool(false))
                    ));
                    assert!(
                        matches!(build_recipe.get("max_workers"), Some(Value::Number(n)) if n.as_i64() == Some(4))
                    );
                    assert!(matches!(build_recipe.get("debug"), Some(Value::Bool(true))));
                }
            }
        }
    }

    #[test]
    fn test_handlebars_cookbook_parsing() {
        use std::fs;
        use tempfile::tempdir;

        let temp_dir = tempdir().unwrap();
        let cookbook_path = temp_dir.path().join("handlebars_test.yml");

        let cookbook_content = r#"
name: "handlebars-test-simple"
description: "Simple handlebars test"

variables:
  service_name: "api"
  enable_cache: true

recipes:
  # Test simple conditionals
  {{#if var.enable_cache}}
  build-with-cache:
    description: "Build with caching enabled"
    run: |
      echo "Building with cache enabled..."
      echo "Service: {{var.service_name}}"
  {{else}}
  build-without-cache:
    description: "Build without caching"  
    run: |
      echo "Building without cache..."
  {{/if}}

  deploy-{{var.service_name}}:
    description: "Deploy {{var.service_name}} service"
    run: |
      echo "Deploying {{var.service_name}}..."
"#;

        fs::write(&cookbook_path, cookbook_content).unwrap();

        let context = VariableContext::builder()
            .environment(vec![])
            .variables(IndexMap::new())
            .overrides(IndexMap::new())
            .build();

        let result =
            super::Cookbook::from(&cookbook_path, temp_dir.path(), Some("default"), &context);

        match result {
            Ok(cookbook) => {
                println!("Successfully parsed cookbook: {}", cookbook.name);
                println!("Recipes: {:?}", cookbook.recipes.keys().collect::<Vec<_>>());

                // Check that handlebars were processed
                assert_eq!(cookbook.name, "handlebars-test-simple");
                assert!(cookbook.recipes.contains_key("build-with-cache"));
                assert!(cookbook.recipes.contains_key("deploy-api"));

                // Verify the run commands were processed
                let deploy_recipe = &cookbook.recipes["deploy-api"];
                assert!(deploy_recipe.run.contains("Deploying api..."));
            }
            Err(e) => {
                panic!("Failed to parse handlebars cookbook: {e}");
            }
        }
    }

    #[test]
    fn test_cookbook_tags_inheritance() {
        use std::fs;
        use tempfile::tempdir;

        let temp_dir = tempdir().unwrap();
        let cookbook_path = temp_dir.path().join("tags_test.yml");

        let cookbook_content = r#"
name: "tags-test"
tags: ["backend", "api"]
recipes:
  build:
    run: echo "building"

  deploy:
    tags: ["frontend", "deploy"]
    run: echo "deploying"
"#;

        fs::write(&cookbook_path, cookbook_content).unwrap();

        let context = VariableContext::builder()
            .environment(vec![])
            .variables(IndexMap::new())
            .overrides(IndexMap::new())
            .build();

        let result =
            super::Cookbook::from(&cookbook_path, temp_dir.path(), Some("default"), &context);

        match result {
            Ok(cookbook) => {
                assert_eq!(cookbook.tags, vec!["backend", "api"]);

                // Recipe without tags should inherit from cookbook
                let build_recipe = &cookbook.recipes["build"];
                assert_eq!(build_recipe.tags, vec!["backend", "api"]);

                // Recipe with tags should use its own tags (replace cookbook tags)
                let deploy_recipe = &cookbook.recipes["deploy"];
                assert_eq!(deploy_recipe.tags, vec!["frontend", "deploy"]);
            }
            Err(e) => {
                panic!("Failed to parse cookbook with tags: {e}");
            }
        }
    }

    #[test]
    fn test_cookbook_tags_serialization() {
        let yaml = r#"
name: test
tags: ["frontend", "backend"]
recipes: {}
"#;
        let cookbook: super::Cookbook = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(cookbook.tags, vec!["frontend", "backend"]);
    }
}
