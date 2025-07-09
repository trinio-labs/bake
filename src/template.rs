use std::{collections::BTreeMap, env};

use anyhow::bail;
use handlebars::Handlebars;
use indexmap::IndexMap;
use serde_json::json;

/// Represents the context for template variable processing, containing all
/// available variables, environment variables, and constants.
#[derive(Debug, Clone)]
pub struct VariableContext {
    /// Environment variables that should be sourced
    pub environment: Vec<String>,
    /// User-defined variables
    pub variables: IndexMap<String, String>,
    /// System constants (project, cookbook, etc.)
    pub constants: IndexMap<String, IndexMap<String, String>>,
    /// Variables that override user-defined variables
    pub overrides: IndexMap<String, String>,
}

impl VariableContext {
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
    pub fn process_variables(&self) -> anyhow::Result<IndexMap<String, String>> {
        self.variables
            .iter()
            .try_fold(IndexMap::new(), |mut acc, (k, v)| {
                if self.overrides.contains_key(k) {
                    acc.insert(k.clone(), self.overrides[k].clone());
                    return Ok(acc);
                }
                // Create a temporary context with the current processed variables
                let mut temp_context = self.clone();
                temp_context.variables = acc.clone();
                let parsed_var = temp_context.parse_template(v)?;
                acc.insert(k.clone(), parsed_var);
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
        if let Err(e) = handlebars.register_template_string("template", template) {
            bail!(
                "Template Parsing: Failed to register template string '{}': {}",
                template,
                e
            );
        }

        let mut data =
            BTreeMap::from([("env", json!(env_values)), ("var", json!(&self.variables))]);
        data.extend(self.constants.iter().map(|(k, v)| (k.as_ref(), json!(v))));

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
        let project_constants =
            IndexMap::from([("root".to_owned(), project_root.display().to_string())]);
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
        let cookbook_constants = IndexMap::from([(
            "root".to_owned(),
            cookbook_path
                .parent()
                .ok_or_else(|| anyhow::anyhow!("Cookbook path has no parent directory"))?
                .display()
                .to_string(),
        )]);
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

    pub fn variables(mut self, variables: IndexMap<String, String>) -> Self {
        self.context.variables = variables;
        self
    }

    pub fn overrides(mut self, overrides: IndexMap<String, String>) -> Self {
        self.context.overrides = overrides;
        self
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
        let variables = IndexMap::from([("foo".to_owned(), "bar".to_owned())]);
        let environment = vec!["TEST_PARSE_TEMPLATE".to_owned()];
        env::set_var("TEST_PARSE_TEMPLATE", "env_var");

        // Use with_project_constants to inject a constant
        let mut context = VariableContext::with_project_constants(Path::new("/project/root"));
        context.environment = environment.clone();
        context.variables = variables.clone();
        context
            .constants
            .get_mut("project")
            .unwrap()
            .insert("foo".to_owned(), "bar".to_owned());

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
                "{{env.TEST_PARSE_VARIABLE_LIST}}".to_owned(),
            ),
            ("baz".to_owned(), "{{var.foo}}".to_owned()),
            ("bar".to_owned(), "bar".to_owned()),
            ("goo".to_owned(), "{{ var.bar }}".to_owned()),
        ]);

        // Use with_project_constants to inject a constant
        let mut context = VariableContext::with_project_constants(Path::new("/project/root"));
        context.environment = environment;
        context.variables = variables;
        context.overrides = overrides;
        context
            .constants
            .get_mut("project")
            .unwrap()
            .insert("foo".to_owned(), "bar".to_owned());

        let result = context.process_variables().unwrap();
        assert_eq!(result.get("foo"), Some(&"bar".to_owned()));
        assert_eq!(result.get("baz").unwrap(), "bar");
        assert_eq!(result.get("bar").unwrap(), "override");
        assert_eq!(result.get("goo").unwrap(), "override");
    }

    #[test]
    fn test_variable_context_builder() {
        let mut context = VariableContext::with_project_constants(Path::new("/tmp"));
        context.environment.push("TEST_BUILDER".to_owned());
        context.variables.insert("foo".to_owned(), "bar".to_owned());
        context
            .constants
            .get_mut("project")
            .unwrap()
            .insert("root".to_owned(), "/tmp".to_owned());
        context
            .overrides
            .insert("override".to_owned(), "value".to_owned());

        assert_eq!(context.environment, vec!["TEST_BUILDER"]);
        assert_eq!(context.variables.get("foo"), Some(&"bar".to_owned()));
        assert_eq!(
            context.constants.get("project").unwrap().get("root"),
            Some(&"/tmp".to_owned())
        );
        assert_eq!(context.overrides.get("override"), Some(&"value".to_owned()));
    }

    #[test]
    fn test_variable_context_merge() {
        let mut context1 = VariableContext::builder().build();
        context1.environment.push("ENV1".to_owned());
        context1
            .variables
            .insert("var1".to_owned(), "value1".to_owned());

        let mut context2 = VariableContext::builder().build();
        context2.environment.push("ENV2".to_owned());
        context2
            .variables
            .insert("var2".to_owned(), "value2".to_owned());

        context1.merge(&context2);

        assert!(context1.environment.contains(&"ENV1".to_owned()));
        assert!(context1.environment.contains(&"ENV2".to_owned()));
        assert_eq!(context1.variables.get("var1"), Some(&"value1".to_owned()));
        assert_eq!(context1.variables.get("var2"), Some(&"value2".to_owned()));
    }

    #[test]
    fn test_variable_inheritance() {
        // Simulate the project -> cookbook -> recipe variable inheritance
        let project_variables = IndexMap::from([("bake_project_var".to_owned(), "bar".to_owned())]);

        // Cookbook variables that reference project variables
        let cookbook_variables = IndexMap::from([
            ("foo".to_owned(), "{{ var.bake_project_var }}".to_owned()),
            ("baz".to_owned(), "{{ var.foo }}".to_owned()),
        ]);

        // Recipe variables that override cookbook variables
        let recipe_variables = IndexMap::from([("foo".to_owned(), "build-bar".to_owned())]);

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
        assert_eq!(processed_recipe_vars.get("baz"), Some(&"bar".to_owned()));
        assert_eq!(
            processed_recipe_vars.get("foo"),
            Some(&"build-bar".to_owned())
        );
    }

    #[test]
    fn test_boolean_template_substitution() {
        // Test that boolean values are preserved correctly
        let variables = IndexMap::from([
            ("force_build".to_owned(), "false".to_owned()),
            ("enable_cache".to_owned(), "true".to_owned()),
            ("debug_mode".to_owned(), "{{ var.force_build }}".to_owned()),
        ]);

        let context = VariableContext::builder().variables(variables).build();

        let result = context.process_variables().unwrap();

        // The debug_mode should be "false" (string) since it references force_build
        assert_eq!(result.get("debug_mode"), Some(&"false".to_owned()));
        assert_eq!(result.get("force_build"), Some(&"false".to_owned()));
        assert_eq!(result.get("enable_cache"), Some(&"true".to_owned()));
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
            ("force_build".to_owned(), "false".to_owned()),
            ("max_workers".to_owned(), "4".to_owned()),
            ("debug_enabled".to_owned(), "true".to_owned()),
            ("cache_path".to_owned(), "/tmp/cache".to_owned()),
            ("null_value".to_owned(), "null".to_owned()),
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
}
