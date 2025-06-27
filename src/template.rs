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
    /// Creates a new variable context with the given components
    pub fn new(
        environment: Vec<String>,
        variables: IndexMap<String, String>,
        constants: IndexMap<String, IndexMap<String, String>>,
        overrides: IndexMap<String, String>,
    ) -> Self {
        Self {
            environment,
            variables,
            constants,
            overrides,
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
        handlebars
            .register_template_string("template", template)
            .unwrap_or_else(|_| {
                panic!("Template Parsing: Failed to register template string '{template}'")
            });

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
    pub fn with_cookbook_constants(cookbook_path: &std::path::Path) -> Self {
        let cookbook_constants = IndexMap::from([(
            "root".to_owned(),
            cookbook_path.parent().unwrap().display().to_string(),
        )]);
        let constants = IndexMap::from([("cookbook".to_owned(), cookbook_constants)]);

        Self {
            environment: Vec::new(),
            variables: IndexMap::new(),
            constants,
            overrides: IndexMap::new(),
        }
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

    pub fn constants(mut self, constants: IndexMap<String, IndexMap<String, String>>) -> Self {
        self.context.constants = constants;
        self
    }

    pub fn overrides(mut self, overrides: IndexMap<String, String>) -> Self {
        self.context.overrides = overrides;
        self
    }

    pub fn add_environment(mut self, env_var: String) -> Self {
        self.context.environment.push(env_var);
        self
    }

    pub fn add_variable(mut self, key: String, value: String) -> Self {
        self.context.variables.insert(key, value);
        self
    }

    pub fn add_constant(mut self, namespace: String, key: String, value: String) -> Self {
        self.context
            .constants
            .entry(namespace)
            .or_insert_with(IndexMap::new)
            .insert(key, value);
        self
    }

    pub fn add_override(mut self, key: String, value: String) -> Self {
        self.context.overrides.insert(key, value);
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

// Legacy functions for backward compatibility
pub fn parse_template(
    template: &str,
    environment: &[String],
    variables: &IndexMap<String, String>,
    constants: &IndexMap<String, IndexMap<String, String>>,
) -> anyhow::Result<String> {
    let context = VariableContext::new(
        environment.to_vec(),
        variables.clone(),
        constants.clone(),
        IndexMap::new(),
    );
    context.parse_template(template)
}

pub fn parse_variable_list(
    environment: &[String],
    variables: &IndexMap<String, String>,
    constants: &IndexMap<String, IndexMap<String, String>>,
    override_variables: &IndexMap<String, String>,
) -> anyhow::Result<IndexMap<String, String>> {
    let context = VariableContext::new(
        environment.to_vec(),
        variables.clone(),
        constants.clone(),
        override_variables.clone(),
    );
    context.process_variables()
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_parse_template() {
        let variables = IndexMap::from([("foo".to_owned(), "bar".to_owned())]);
        let constants = IndexMap::from([(
            "project".to_owned(),
            IndexMap::from([("foo".to_owned(), "bar".to_owned())]),
        )]);

        let environment = vec!["TEST_PARSE_TEMPLATE".to_owned()];
        env::set_var("TEST_PARSE_TEMPLATE", "env_var");

        let result = parse_template(
            "{{var.foo}}",
            environment.as_slice(),
            &variables,
            &constants,
        )
        .unwrap();
        assert_eq!(result, "bar");

        let result = parse_template(
            "{{project.foo}}",
            environment.as_slice(),
            &variables,
            &constants,
        )
        .unwrap();
        assert_eq!(result, "bar");

        let result = parse_template(
            "{{env.TEST_PARSE_TEMPLATE}}",
            environment.as_slice(),
            &variables,
            &constants,
        )
        .unwrap();
        assert_eq!(result, "env_var");
    }

    #[test]
    fn test_parse_variable_list() {
        let environment = vec!["TEST_PARSE_VARIABLE_LIST".to_owned()];
        env::set_var("TEST_PARSE_VARIABLE_LIST", "bar");

        let constants = IndexMap::from([(
            "project".to_owned(),
            IndexMap::from([("foo".to_owned(), "bar".to_owned())]),
        )]);

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

        let result =
            parse_variable_list(environment.as_slice(), &variables, &constants, &overrides)
                .unwrap();
        assert_eq!(result.get("foo").unwrap(), "bar");
        assert_eq!(result.get("baz").unwrap(), "bar");
        assert_eq!(result.get("bar").unwrap(), "override");
        assert_eq!(result.get("goo").unwrap(), "override");
    }

    #[test]
    fn test_variable_context_builder() {
        let context = VariableContext::builder()
            .add_environment("TEST_BUILDER".to_owned())
            .add_variable("foo".to_owned(), "bar".to_owned())
            .add_constant("project".to_owned(), "root".to_owned(), "/tmp".to_owned())
            .add_override("override".to_owned(), "value".to_owned())
            .build();

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
        let mut context1 = VariableContext::builder()
            .add_environment("ENV1".to_owned())
            .add_variable("var1".to_owned(), "value1".to_owned())
            .build();

        let context2 = VariableContext::builder()
            .add_environment("ENV2".to_owned())
            .add_variable("var2".to_owned(), "value2".to_owned())
            .build();

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
        println!("Project variables: {:?}", processed_cookbook_vars);

        // Now process cookbook variables with project variables
        let mut cookbook_context = VariableContext::builder()
            .variables(processed_cookbook_vars.clone())
            .build();
        cookbook_context.variables.extend(cookbook_variables);

        let processed_cookbook_vars = cookbook_context.process_variables().unwrap();
        println!("Cookbook variables: {:?}", processed_cookbook_vars);

        // Now process recipe variables with access to cookbook variables
        let mut recipe_context = VariableContext::builder()
            .variables(processed_cookbook_vars.clone())
            .build();
        recipe_context.variables.extend(recipe_variables);

        let processed_recipe_vars = recipe_context.process_variables().unwrap();
        println!("Recipe variables: {:?}", processed_recipe_vars);

        // The cookbook-level baz should still be "bar" (from project foo), not "build-bar"
        assert_eq!(processed_recipe_vars.get("baz"), Some(&"bar".to_owned()));
        assert_eq!(
            processed_recipe_vars.get("foo"),
            Some(&"build-bar".to_owned())
        );
    }
}
