use std::{collections::BTreeMap, env, path::Path};

use anyhow::bail;
use handlebars::Handlebars;
use indexmap::IndexMap;
use serde_json::{json, Value as JsonValue};

/// Helper struct for loading variables from variable files with environment support
pub struct VariableFileLoader;

impl VariableFileLoader {
    /// Loads variables from a variable file (vars.yml or variables.yml) for a specific environment
    ///
    /// # Arguments
    /// * `directory` - Directory to search for variable files
    /// * `environment` - Environment name (e.g., "dev", "prod", "default")
    ///
    /// # Returns
    /// * `Ok(IndexMap<String, serde_yaml::Value>)` - Variables for the specified environment
    /// * `Err` if file reading or parsing fails
    pub fn load_variables_from_directory(
        directory: &Path,
        environment: Option<&str>,
    ) -> anyhow::Result<IndexMap<String, serde_yaml::Value>> {
        // Try to find variable files in order of preference
        let var_file_names = ["vars.yml", "vars.yaml", "variables.yml", "variables.yaml"];

        for file_name in &var_file_names {
            let file_path = directory.join(file_name);
            if file_path.exists() {
                return Self::load_variables_from_file(&file_path, environment);
            }
        }

        // No variable file found, return empty variables
        Ok(IndexMap::new())
    }

    /// Loads variables from a specific variable file for a given environment
    ///
    /// # Arguments
    /// * `file_path` - Path to the variable file
    /// * `environment` - Environment name (e.g., "dev", "prod", "default")
    ///
    /// # Returns
    /// * `Ok(IndexMap<String, serde_yaml::Value>)` - Variables for the specified environment
    /// * `Err` if file reading or parsing fails
    pub fn load_variables_from_file(
        file_path: &Path,
        environment: Option<&str>,
    ) -> anyhow::Result<IndexMap<String, serde_yaml::Value>> {
        let content = std::fs::read_to_string(file_path).map_err(|e| {
            anyhow::anyhow!(
                "Failed to read variable file '{}': {}",
                file_path.display(),
                e
            )
        })?;

        // Parse the YAML content
        let yaml_value: serde_yaml::Value = serde_yaml::from_str(&content).map_err(|e| {
            anyhow::anyhow!(
                "Failed to parse variable file '{}': {}",
                file_path.display(),
                e
            )
        })?;

        // Extract environment-specific variables
        Self::extract_environment_variables(&yaml_value, environment, file_path)
    }

    /// Extracts variables for a specific environment from parsed YAML
    ///
    /// # Arguments
    /// * `yaml_value` - Parsed YAML content
    /// * `environment` - Environment name to extract (e.g., "dev", "prod")
    /// * `file_path` - File path for error reporting
    ///
    /// # Returns
    /// * `Ok(IndexMap<String, serde_yaml::Value>)` - Variables for the environment (default + env overrides)
    /// * `Err` if file structure is invalid
    fn extract_environment_variables(
        yaml_value: &serde_yaml::Value,
        environment: Option<&str>,
        file_path: &Path,
    ) -> anyhow::Result<IndexMap<String, serde_yaml::Value>> {
        let yaml_mapping = yaml_value.as_mapping().ok_or_else(|| {
            anyhow::anyhow!(
                "Variable file '{}' must contain a YAML mapping at the root level",
                file_path.display()
            )
        })?;

        // Start with default variables
        let mut variables = IndexMap::new();

        // Load default variables if present
        if let Some(default_value) =
            yaml_mapping.get(serde_yaml::Value::String("default".to_string()))
        {
            let default_mapping = default_value.as_mapping().ok_or_else(|| {
                anyhow::anyhow!(
                    "'default' section in variable file '{}' must contain a mapping of variables",
                    file_path.display()
                )
            })?;

            for (key, value) in default_mapping {
                let key_str = key.as_str().ok_or_else(|| {
                    anyhow::anyhow!(
                        "Variable key in 'default' section must be a string in file '{}'",
                        file_path.display()
                    )
                })?;

                variables.insert(key_str.to_string(), value.clone());
            }
        }

        // Apply environment-specific overrides if environment is specified
        if let Some(environment) = environment {
            if let Some(envs_value) =
                yaml_mapping.get(serde_yaml::Value::String("envs".to_string()))
            {
                let envs_mapping = envs_value.as_mapping()
                    .ok_or_else(|| anyhow::anyhow!(
                        "'envs' section in variable file '{}' must contain a mapping of environments",
                        file_path.display()
                    ))?;

                if let Some(env_value) =
                    envs_mapping.get(serde_yaml::Value::String(environment.to_string()))
                {
                    let env_mapping = env_value.as_mapping()
                        .ok_or_else(|| anyhow::anyhow!(
                            "Environment '{}' in 'envs' section must contain a mapping of variables in file '{}'",
                            environment,
                            file_path.display()
                        ))?;

                    // Override with environment-specific variables
                    for (key, value) in env_mapping {
                        let key_str = key.as_str().ok_or_else(|| {
                            anyhow::anyhow!(
                                "Variable key in environment '{}' must be a string in file '{}'",
                                environment,
                                file_path.display()
                            )
                        })?;

                        variables.insert(key_str.to_string(), value.clone());
                    }
                } else {
                    // Environment not found in envs section, but that's OK - just use defaults
                    // Only show warning if there are envs but the requested one is missing
                    log::debug!(
                        "Environment '{}' not found in 'envs' section of '{}', using defaults only",
                        environment,
                        file_path.display()
                    );
                }
            } else {
                // No envs section, but that's OK for files with only defaults
                log::debug!(
                    "No 'envs' section found in '{}', using defaults only",
                    file_path.display()
                );
            }
        }

        Ok(variables)
    }
}

/// Represents the context for template variable processing, containing all
/// available variables, environment variables, and constants.
#[derive(Debug, Clone)]
pub struct VariableContext {
    /// Environment variables that should be sourced
    pub environment: Vec<String>,
    /// User-defined variables with preserved YAML types
    pub variables: IndexMap<String, serde_yaml::Value>,
    /// System constants that can hold both simple and complex data (project, cookbook, params, etc.)
    pub constants: IndexMap<String, JsonValue>,
    /// Variables that override user-defined variables (from CLI --var flags)
    pub overrides: IndexMap<String, String>,
}

impl VariableContext {
    /// Converts serde_yaml::Value to serde_json::Value for Handlebars compatibility
    pub fn yaml_to_json(yaml_value: &serde_yaml::Value) -> JsonValue {
        match yaml_value {
            serde_yaml::Value::Null => JsonValue::Null,
            serde_yaml::Value::Bool(b) => JsonValue::Bool(*b),
            serde_yaml::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    JsonValue::Number(serde_json::Number::from(i))
                } else if let Some(u) = n.as_u64() {
                    JsonValue::Number(serde_json::Number::from(u))
                } else if let Some(f) = n.as_f64() {
                    JsonValue::Number(
                        serde_json::Number::from_f64(f).unwrap_or(serde_json::Number::from(0)),
                    )
                } else {
                    JsonValue::String(n.to_string())
                }
            }
            serde_yaml::Value::String(s) => JsonValue::String(s.clone()),
            serde_yaml::Value::Sequence(seq) => {
                JsonValue::Array(seq.iter().map(Self::yaml_to_json).collect())
            }
            serde_yaml::Value::Mapping(map) => {
                let json_map: serde_json::Map<String, JsonValue> = map
                    .iter()
                    .filter_map(|(k, v)| {
                        k.as_str()
                            .map(|key| (key.to_string(), Self::yaml_to_json(v)))
                    })
                    .collect();
                JsonValue::Object(json_map)
            }
            serde_yaml::Value::Tagged(tagged) => Self::yaml_to_json(&tagged.value),
        }
    }
    /// Renders handlebars in raw string content before YAML parsing
    pub fn render_raw_template(&self, template: &str) -> anyhow::Result<String> {
        let mut handlebars = Handlebars::new();
        handlebars.register_escape_fn(handlebars::no_escape);

        if let Err(e) = handlebars.register_template_string("raw_template", template) {
            bail!(
                "Raw Template Parsing: Failed to register template string: {}. Template content: '{}'",
                e, template
            );
        }

        // Build data context
        let env_values: BTreeMap<String, String> = self
            .environment
            .iter()
            .map(|name| (name.to_string(), env::var(name).unwrap_or_default()))
            .collect();

        // Convert serde_yaml::Value variables to JsonValue for Handlebars
        let json_variables: BTreeMap<String, JsonValue> = self
            .variables
            .iter()
            .map(|(k, v)| (k.clone(), Self::yaml_to_json(v)))
            .collect();

        let mut data = BTreeMap::from([("env", json!(env_values)), ("var", json!(json_variables))]);
        data.extend(self.constants.iter().map(|(k, v)| (k.as_ref(), v.clone())));

        match handlebars.render("raw_template", &data) {
            Ok(rendered) => Ok(rendered),
            Err(err) => bail!(
                "Raw Template Rendering: Failed to render template: {}. Template content: '{}'",
                err,
                template
            ),
        }
    }
    /// Creates a new variable context with empty collections
    pub fn empty() -> Self {
        Self {
            environment: Vec::new(),
            variables: IndexMap::new(),
            constants: IndexMap::new(),
            overrides: IndexMap::new(),
        }
    }

    /// Creates a builder for constructing variable contexts
    pub fn builder() -> VariableContextBuilder {
        VariableContextBuilder::new()
    }

    /// Merges another context into this one, with this context taking precedence
    pub fn merge(&mut self, other: &Self) {
        self.environment.extend(other.environment.iter().cloned());
        self.variables
            .extend(other.variables.iter().map(|(k, v)| (k.clone(), v.clone())));
        self.constants
            .extend(other.constants.iter().map(|(k, v)| (k.clone(), v.clone())));
        self.overrides
            .extend(other.overrides.iter().map(|(k, v)| (k.clone(), v.clone())));
    }

    /// Processes all variables in the context, resolving templates and applying overrides
    pub fn process_variables(&self) -> anyhow::Result<IndexMap<String, serde_yaml::Value>> {
        self.variables
            .iter()
            .try_fold(IndexMap::new(), |mut acc, (k, v)| {
                if self.overrides.contains_key(k) {
                    // CLI overrides are strings that need to be parsed as YAML values
                    let override_str = &self.overrides[k];
                    let yaml_value = match serde_yaml::from_str::<serde_yaml::Value>(override_str) {
                        Ok(value) => value,
                        Err(_) => serde_yaml::Value::String(override_str.clone()),
                    };
                    acc.insert(k.clone(), yaml_value);
                    return Ok(acc);
                }

                // For string values that contain templates, process them
                let processed_value = match v {
                    serde_yaml::Value::String(s) if s.contains("{{") && s.contains("}}") => {
                        // Create a temporary context with the current processed variables
                        let mut temp_context = self.clone();
                        temp_context.variables = acc.clone();
                        let parsed_str = temp_context.parse_template(s)?;

                        // Try to parse the result back to a YAML value to preserve types
                        match serde_yaml::from_str::<serde_yaml::Value>(&parsed_str) {
                            Ok(parsed_value) => parsed_value,
                            Err(_) => serde_yaml::Value::String(parsed_str),
                        }
                    }
                    _ => v.clone(),
                };

                acc.insert(k.clone(), processed_value);
                Ok(acc)
            })
    }

    /// Parses a template string using this context
    pub fn parse_template(&self, template: &str) -> anyhow::Result<String> {
        // Get environment variables from the environment list
        let env_values: BTreeMap<String, String> = self
            .environment
            .iter()
            .map(|name| (name.to_string(), env::var(name).unwrap_or_default()))
            .collect();

        let mut handlebars = Handlebars::new();
        // Disable HTML escaping since we're generating shell commands, not HTML
        handlebars.register_escape_fn(handlebars::no_escape);
        if let Err(e) = handlebars.register_template_string("template", template) {
            bail!(
                "Template Parsing: Failed to register template string '{}': {}",
                template,
                e
            );
        }

        // Convert serde_yaml::Value variables to JsonValue for Handlebars
        let json_variables: BTreeMap<String, JsonValue> = self
            .variables
            .iter()
            .map(|(k, v)| (k.clone(), Self::yaml_to_json(v)))
            .collect();

        let mut data = BTreeMap::from([("env", json!(env_values)), ("var", json!(json_variables))]);
        data.extend(self.constants.iter().map(|(k, v)| (k.as_ref(), v.clone())));

        match handlebars.render("template", &data) {
            Ok(rendered) => Ok(rendered),
            Err(err) => bail!(
                "Template Rendering ('{}'): Failed to render template: {}. Ensure all referenced variables (env, var, constants) are correctly defined and accessible.",
                template,
                err
            ),
        }
    }

    /// Creates project constants from a project root path
    pub fn with_project_constants(project_root: &std::path::Path) -> Self {
        let project_constants = json!({
            "root": project_root.display().to_string()
        });
        let constants = IndexMap::from([("project".to_owned(), project_constants)]);

        Self {
            environment: Vec::new(),
            variables: IndexMap::new(),
            constants,
            overrides: IndexMap::new(),
        }
    }

    /// Creates cookbook constants from a cookbook path
    pub fn with_cookbook_constants(cookbook_path: &std::path::Path) -> anyhow::Result<Self> {
        let cookbook_constants = json!({
            "root": cookbook_path
                .parent()
                .ok_or_else(|| anyhow::anyhow!("Cookbook path has no parent directory"))?
                .display()
                .to_string()
        });
        let constants = IndexMap::from([("cookbook".to_owned(), cookbook_constants)]);

        Ok(Self {
            environment: Vec::new(),
            variables: IndexMap::new(),
            constants,
            overrides: IndexMap::new(),
        })
    }

    /// Processes template variables in a YAML value recursively, preserving original types
    pub fn process_template_in_value(
        value: &mut serde_yaml::Value,
        context: &Self,
        skip_variables_and_run: bool,
    ) -> anyhow::Result<()> {
        match value {
            serde_yaml::Value::String(s) => {
                // Only process strings that contain template syntax
                if s.contains("{{") && s.contains("}}") {
                    let processed = context.parse_template(s)?;

                    // Use serde_yaml's built-in parsing to preserve types
                    // This automatically handles bool, number, null, etc.
                    if let Ok(parsed_value) = serde_yaml::from_str::<serde_yaml::Value>(&processed)
                    {
                        *value = parsed_value;
                    } else {
                        // If parsing fails, keep as string
                        *s = processed;
                    }
                }
            }
            serde_yaml::Value::Mapping(map) => {
                for (k, v) in map.iter_mut() {
                    // Skip processing the "variables" and "run" fields if requested
                    if skip_variables_and_run
                        && (k.as_str() == Some("variables") || k.as_str() == Some("run"))
                    {
                        continue;
                    }
                    Self::process_template_in_value(v, context, skip_variables_and_run)?;
                }
            }
            serde_yaml::Value::Sequence(seq) => {
                for item in seq.iter_mut() {
                    Self::process_template_in_value(item, context, skip_variables_and_run)?;
                }
            }
            _ => {} // Other types (Number, Bool, Null) don't need processing
        }
        Ok(())
    }
}

/// Builder for creating VariableContext instances
#[derive(Debug)]
pub struct VariableContextBuilder {
    context: VariableContext,
}

impl VariableContextBuilder {
    pub fn new() -> Self {
        Self {
            context: VariableContext::empty(),
        }
    }

    pub fn environment(mut self, environment: Vec<String>) -> Self {
        self.context.environment = environment;
        self
    }

    pub fn variables(mut self, variables: IndexMap<String, serde_yaml::Value>) -> Self {
        self.context.variables = variables;
        self
    }

    pub fn overrides(mut self, overrides: IndexMap<String, String>) -> Self {
        self.context.overrides = overrides;
        self
    }

    /// Loads variables from a directory's variable file for a specific environment
    ///
    /// # Arguments
    /// * `directory` - Directory to search for variable files
    /// * `environment` - Environment name (e.g., "dev", "prod", "default")
    ///
    /// # Returns
    /// * `Self` - Builder instance for chaining
    /// * Will set variables to empty if file not found or environment not available
    pub fn variables_from_directory(
        mut self,
        directory: &Path,
        environment: Option<&str>,
    ) -> anyhow::Result<Self> {
        let variables = VariableFileLoader::load_variables_from_directory(directory, environment)?;
        self.context.variables.extend(variables);
        Ok(self)
    }

    pub fn build(self) -> VariableContext {
        self.context
    }
}

impl Default for VariableContextBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_parse_template() {
        let variables = IndexMap::from([(
            "foo".to_owned(),
            serde_yaml::Value::String("bar".to_owned()),
        )]);
        let environment = vec!["TEST_PARSE_TEMPLATE".to_owned()];
        env::set_var("TEST_PARSE_TEMPLATE", "env_var");

        // Use with_project_constants to inject a constant
        let mut context = VariableContext::with_project_constants(Path::new("/project/root"));
        context.environment = environment.clone();
        context.variables = variables.clone();
        if let Some(JsonValue::Object(project_obj)) = context.constants.get_mut("project") {
            project_obj.insert("foo".to_owned(), JsonValue::String("bar".to_owned()));
        }

        let result = context.parse_template("{{var.foo}}").unwrap();
        assert_eq!(result, "bar");

        let result = context.parse_template("{{project.foo}}").unwrap();
        assert_eq!(result, "bar");

        let result = context
            .parse_template("{{env.TEST_PARSE_TEMPLATE}}")
            .unwrap();
        assert_eq!(result, "env_var");
    }

    #[test]
    fn test_parse_variable_list() {
        let environment = vec!["TEST_PARSE_VARIABLE_LIST".to_owned()];
        env::set_var("TEST_PARSE_VARIABLE_LIST", "bar");

        let overrides = IndexMap::from([("bar".to_owned(), "override".to_owned())]);

        let variables = IndexMap::from([
            (
                "foo".to_owned(),
                serde_yaml::Value::String("{{env.TEST_PARSE_VARIABLE_LIST}}".to_owned()),
            ),
            (
                "baz".to_owned(),
                serde_yaml::Value::String("{{var.foo}}".to_owned()),
            ),
            (
                "bar".to_owned(),
                serde_yaml::Value::String("bar".to_owned()),
            ),
            (
                "goo".to_owned(),
                serde_yaml::Value::String("{{ var.bar }}".to_owned()),
            ),
        ]);

        // Use with_project_constants to inject a constant
        let mut context = VariableContext::with_project_constants(Path::new("/project/root"));
        context.environment = environment;
        context.variables = variables;
        context.overrides = overrides;
        if let Some(JsonValue::Object(project_obj)) = context.constants.get_mut("project") {
            project_obj.insert("foo".to_owned(), JsonValue::String("bar".to_owned()));
        }

        let result = context.process_variables().unwrap();
        assert_eq!(
            result.get("foo"),
            Some(&serde_yaml::Value::String("bar".to_owned()))
        );
        assert_eq!(
            result.get("baz").unwrap(),
            &serde_yaml::Value::String("bar".to_owned())
        );
        assert_eq!(
            result.get("bar").unwrap(),
            &serde_yaml::Value::String("override".to_owned())
        );
        assert_eq!(
            result.get("goo").unwrap(),
            &serde_yaml::Value::String("override".to_owned())
        );
    }

    #[test]
    fn test_variable_context_builder() {
        let mut context = VariableContext::with_project_constants(Path::new("/tmp"));
        context.environment.push("TEST_BUILDER".to_owned());
        context.variables.insert(
            "foo".to_owned(),
            serde_yaml::Value::String("bar".to_owned()),
        );
        if let Some(JsonValue::Object(project_obj)) = context.constants.get_mut("project") {
            project_obj.insert("root".to_owned(), JsonValue::String("/tmp".to_owned()));
        }
        context
            .overrides
            .insert("override".to_owned(), "value".to_owned());

        assert_eq!(context.environment, vec!["TEST_BUILDER"]);
        assert_eq!(
            context.variables.get("foo"),
            Some(&serde_yaml::Value::String("bar".to_owned()))
        );
        if let Some(JsonValue::Object(project_obj)) = context.constants.get("project") {
            assert_eq!(project_obj.get("root"), Some(&json!("/tmp")));
        }
        assert_eq!(context.overrides.get("override"), Some(&"value".to_owned()));
    }

    #[test]
    fn test_variable_context_merge() {
        let mut context1 = VariableContext::builder().build();
        context1.environment.push("ENV1".to_owned());
        context1.variables.insert(
            "var1".to_owned(),
            serde_yaml::Value::String("value1".to_owned()),
        );

        let mut context2 = VariableContext::builder().build();
        context2.environment.push("ENV2".to_owned());
        context2.variables.insert(
            "var2".to_owned(),
            serde_yaml::Value::String("value2".to_owned()),
        );

        context1.merge(&context2);

        assert!(context1.environment.contains(&"ENV1".to_owned()));
        assert!(context1.environment.contains(&"ENV2".to_owned()));
        assert_eq!(
            context1.variables.get("var1"),
            Some(&serde_yaml::Value::String("value1".to_owned()))
        );
        assert_eq!(
            context1.variables.get("var2"),
            Some(&serde_yaml::Value::String("value2".to_owned()))
        );
    }

    #[test]
    fn test_variable_inheritance() {
        // Simulate the project -> cookbook -> recipe variable inheritance
        let project_variables = IndexMap::from([(
            "bake_project_var".to_owned(),
            serde_yaml::Value::String("bar".to_owned()),
        )]);

        // Cookbook variables that reference project variables
        let cookbook_variables = IndexMap::from([
            (
                "foo".to_owned(),
                serde_yaml::Value::String("{{ var.bake_project_var }}".to_owned()),
            ),
            (
                "baz".to_owned(),
                serde_yaml::Value::String("{{ var.foo }}".to_owned()),
            ),
        ]);

        // Recipe variables that override cookbook variables
        let recipe_variables = IndexMap::from([(
            "foo".to_owned(),
            serde_yaml::Value::String("build-bar".to_owned()),
        )]);

        // Process cookbook variables first
        let cookbook_context = VariableContext::builder()
            .variables(project_variables.clone())
            .build();

        let processed_cookbook_vars = cookbook_context.process_variables().unwrap();
        println!("Project variables: {processed_cookbook_vars:?}");

        // Now process cookbook variables with project variables
        let mut cookbook_context = VariableContext::builder()
            .variables(processed_cookbook_vars.clone())
            .build();
        cookbook_context.variables.extend(cookbook_variables);

        let processed_cookbook_vars = cookbook_context.process_variables().unwrap();
        println!("Cookbook variables: {processed_cookbook_vars:?}");

        // Now process recipe variables with access to cookbook variables
        let mut recipe_context = VariableContext::builder()
            .variables(processed_cookbook_vars.clone())
            .build();
        recipe_context.variables.extend(recipe_variables);

        let processed_recipe_vars = recipe_context.process_variables().unwrap();
        println!("Recipe variables: {processed_recipe_vars:?}");

        // The cookbook-level baz should still be "bar" (from project foo), not "build-bar"
        assert_eq!(
            processed_recipe_vars.get("baz"),
            Some(&serde_yaml::Value::String("bar".to_owned()))
        );
        assert_eq!(
            processed_recipe_vars.get("foo"),
            Some(&serde_yaml::Value::String("build-bar".to_owned()))
        );
    }

    #[test]
    fn test_boolean_template_substitution() {
        // Test that boolean values are preserved correctly
        let variables = IndexMap::from([
            ("force_build".to_owned(), serde_yaml::Value::Bool(false)),
            ("enable_cache".to_owned(), serde_yaml::Value::Bool(true)),
            (
                "debug_mode".to_owned(),
                serde_yaml::Value::String("{{ var.force_build }}".to_owned()),
            ),
        ]);

        let context = VariableContext::builder().variables(variables).build();

        let result = context.process_variables().unwrap();

        // The debug_mode should resolve to false boolean since it references force_build
        assert_eq!(
            result.get("debug_mode"),
            Some(&serde_yaml::Value::Bool(false))
        );
        assert_eq!(
            result.get("force_build"),
            Some(&serde_yaml::Value::Bool(false))
        );
        assert_eq!(
            result.get("enable_cache"),
            Some(&serde_yaml::Value::Bool(true))
        );
    }

    #[test]
    fn test_process_template_in_value_type_preservation() {
        use serde_yaml::Value;

        // Create a YAML value with template variables
        let yaml_str = r#"
force_build: "{{ var.force_build }}"
max_workers: "{{ var.max_workers }}"
debug_enabled: "{{ var.debug_enabled }}"
cache_path: "{{ var.cache_path }}"
null_value: "{{ var.null_value }}"
"#;

        let mut yaml_value: Value = serde_yaml::from_str(yaml_str).unwrap();

        // Create a context with the variables
        let variables = IndexMap::from([
            ("force_build".to_owned(), serde_yaml::Value::Bool(false)),
            (
                "max_workers".to_owned(),
                serde_yaml::Value::Number(serde_yaml::Number::from(4)),
            ),
            ("debug_enabled".to_owned(), serde_yaml::Value::Bool(true)),
            (
                "cache_path".to_owned(),
                serde_yaml::Value::String("/tmp/cache".to_owned()),
            ),
            ("null_value".to_owned(), serde_yaml::Value::Null),
        ]);

        let context = VariableContext::builder().variables(variables).build();

        // Process the template
        VariableContext::process_template_in_value(&mut yaml_value, &context, false).unwrap();

        // Check that the processed values have the correct types
        if let Value::Mapping(map) = &yaml_value {
            assert!(matches!(map.get("force_build"), Some(Value::Bool(false))));
            assert!(
                matches!(map.get("max_workers"), Some(Value::Number(n)) if n.as_i64() == Some(4))
            );
            assert!(matches!(map.get("debug_enabled"), Some(Value::Bool(true))));
            assert!(matches!(map.get("cache_path"), Some(Value::String(s)) if s == "/tmp/cache"));
            assert!(matches!(map.get("null_value"), Some(Value::Null)));
        }
    }

    // Filesystem-dependent tests have been moved to tests/integration/template_tests.rs
}
