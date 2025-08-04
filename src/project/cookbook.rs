use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use crate::{project::Recipe, template::VariableContext};
use anyhow::bail;
use ignore::WalkBuilder;
use indexmap::IndexMap;
use log::debug;
use serde::Deserialize;
use serde_yaml::Value;

#[derive(Debug, Deserialize, Default)]
pub struct Cookbook {
    pub name: String,

    #[serde(default)]
    pub environment: Vec<String>,

    #[serde(default)]
    pub variables: IndexMap<String, String>,

    pub recipes: BTreeMap<String, Recipe>,

    #[serde(skip)]
    pub config_path: PathBuf,
}
impl Cookbook {
    /// Creates a cookbook config from a path to a cookbook file
    ///
    /// # Arguments
    /// * `path` - Path to a cookbook file
    /// * `project_root` - Path to the project root
    /// * `context` - Variable context containing environment, variables, and overrides
    ///
    pub fn from(
        path: &PathBuf,
        project_root: &Path,
        context: &VariableContext,
    ) -> anyhow::Result<Self> {
        let config: Cookbook;

        let config_str = match std::fs::read_to_string(path) {
            Ok(contents) => contents,
            Err(_) => bail!("Cookbook: Failed to read cookbook configuration file at '{}': Check file existence and permissions.", path.display()),
        };

        // First parse as generic YAML to allow template processing
        let mut yaml_value: Value = match serde_yaml::from_str(&config_str) {
            Ok(value) => value,
            Err(err) => bail!("Cookbook: Failed to parse cookbook configuration file at '{}': {}. Check YAML syntax.", path.display(), err),
        };

        // Create a new context for this cookbook, inheriting from the project context
        let mut cookbook_context = context.clone();

        // Add project and cookbook constants
        cookbook_context.merge(&VariableContext::with_project_constants(project_root));
        cookbook_context.merge(&VariableContext::with_cookbook_constants(path)?);

        // Process template variables in the YAML structure (but skip the variables and run fields)
        VariableContext::process_template_in_value(&mut yaml_value, &cookbook_context, true)?;

        // Now deserialize into the Cookbook struct
        match serde_yaml::from_value::<Self>(yaml_value) {
            Ok(mut parsed) => {
                parsed.config_path = path.to_path_buf();

                // Inherit environment and variables from project
                let mut cookbook_environment = context.environment.clone();
                cookbook_environment.extend(parsed.environment.iter().cloned());
                parsed.environment = cookbook_environment;

                let mut cookbook_variables = context.variables.clone();
                cookbook_variables.extend(parsed.variables.clone());

                // Process cookbook variables with project variables only
                let mut cookbook_var_context = cookbook_context.clone();
                cookbook_var_context.variables = cookbook_variables;
                parsed.variables = cookbook_var_context.process_variables()?;
                let resolved_cookbook_vars = parsed.variables.clone();

                parsed.recipes.iter_mut().try_for_each(|(name, recipe)| {
                    recipe.name = name.clone();
                    recipe.cookbook = parsed.name.clone();
                    recipe.config_path = path.to_path_buf();
                    recipe.project_root = project_root.to_path_buf();

                    // Inherit environment and variables from cookbook
                    let mut recipe_environment = parsed.environment.clone();
                    recipe_environment.extend(recipe.environment.iter().cloned());
                    recipe.environment = recipe_environment.clone();

                    // Start with resolved cookbook variables, then add recipe variables
                    let mut recipe_variables = resolved_cookbook_vars.clone();
                    recipe_variables.extend(recipe.variables.clone());

                    // Process recipe variables with access to cookbook variables
                    let mut recipe_context = cookbook_context.clone();
                    recipe_context.environment = recipe_environment;
                    recipe_context.variables = recipe_variables;
                    match recipe_context.process_variables() {
                        Ok(variables) => {
                            recipe.variables = variables;
                        }
                        Err(_) => {
                            bail!("Cookbook '{}': Failed to parse variables for recipe '{}'. Check syntax and variable definitions.", parsed.name, recipe.name)
                        }
                    }

                    // Process the run field with the recipe's processed variables
                    let mut run_context = cookbook_context.clone();
                    run_context.variables = recipe.variables.clone();
                    recipe.run = run_context.parse_template(&recipe.run)?;

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

                    Ok(())
                })?;
                config = parsed;
            }
            Err(err) => bail!("Cookbook: Failed to deserialize cookbook configuration after template processing at '{}': {}. Check YAML syntax and template variable usage.", path.display(), err),
        }

        Ok(config)
    }

    /// Gets all cookbooks recursively in a directory
    ///
    /// map_from recursively searches for all cookbooks in a directory respecting `.gitignore` and
    /// `.bakeignore` files
    ///
    /// # Arguments
    /// * `path` - Path to a directory
    /// * `context` - Variable context containing environment, variables, and overrides
    ///
    pub fn map_from(
        path: &PathBuf,
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
                        match Self::from(&file.into_path(), path, context) {
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

    use crate::template::VariableContext;
    use indexmap::IndexMap;
    use test_case::test_case;

    fn config_path(path_str: &str) -> String {
        env!("CARGO_MANIFEST_DIR").to_owned() + "/resources/tests" + path_str
    }

    fn validate_cookbook_foo(actual: anyhow::Result<super::Cookbook>) {
        assert_eq!(actual.as_ref().unwrap().name, "foo");
    }

    fn validate_cookbook_vec(actual: anyhow::Result<BTreeMap<String, super::Cookbook>>) {
        assert_eq!(actual.unwrap().len(), 4)
    }

    #[test_case(config_path("/valid/"), config_path("/valid/foo/cookbook.yml") => using validate_cookbook_foo; "Valid cookbook file")]
    #[test_case(config_path("/valid/"),config_path("/invalid/config/cookbook.yml") => matches Err(_); "Invalid cookbook file")]
    #[test_case(config_path("/valid/"), config_path("/invalid/config") => matches Err(_); "Cant read directory")]
    fn read_cookbook(project_root: String, path_str: String) -> anyhow::Result<super::Cookbook> {
        super::Cookbook::from(
            &PathBuf::from(path_str),
            &PathBuf::from(project_root),
            &VariableContext::builder()
                .environment(vec![])
                .variables(IndexMap::new())
                .overrides(IndexMap::new())
                .build(),
        )
    }

    #[test_case(config_path("/valid/") => using validate_cookbook_vec; "Root dir")]
    #[test_case(config_path("/invalid/config") => matches Err(_); "Invalid dir")]
    fn read_all_cookbooks(path_str: String) -> anyhow::Result<BTreeMap<String, super::Cookbook>> {
        super::Cookbook::map_from(
            &PathBuf::from(path_str),
            &VariableContext::builder()
                .environment(vec![])
                .variables(IndexMap::new())
                .overrides(IndexMap::new())
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

        // Create a context with the variables
        let variables = IndexMap::from([
            ("force_build".to_owned(), "false".to_owned()),
            ("max_parallel".to_owned(), "4".to_owned()),
            ("debug_enabled".to_owned(), "true".to_owned()),
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
}
