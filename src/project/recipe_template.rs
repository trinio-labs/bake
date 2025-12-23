use std::{collections::BTreeMap, path::PathBuf};

use anyhow::bail;
use serde::{Deserialize, Serialize};
use serde_yaml::Value;

use crate::template::{VariableContext, extract_yaml_block};

/// Represents a parameter type for template validation
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ParameterType {
    String,
    Number,
    Boolean,
    Array,
    Object,
}

/// Represents a template parameter definition with validation rules
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TemplateParameter {
    /// The type of this parameter
    #[serde(rename = "type")]
    pub parameter_type: ParameterType,

    /// Whether this parameter is required
    #[serde(default)]
    pub required: bool,

    /// Default value for this parameter (as YAML value)
    pub default: Option<Value>,

    /// Human-readable description of this parameter
    pub description: Option<String>,

    /// For string types: regex pattern validation
    pub pattern: Option<String>,

    /// For number types: minimum value
    pub min: Option<f64>,

    /// For number types: maximum value
    pub max: Option<f64>,

    /// For array types: type of items in the array
    pub items: Option<Box<TemplateParameter>>,
}

/// Represents a complete recipe template
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RecipeTemplate {
    /// Name of the template
    pub name: String,

    /// Description of what this template does
    pub description: Option<String>,

    /// Template this one extends (for inheritance)
    pub extends: Option<String>,

    /// Parameters that can be passed to this template
    #[serde(default)]
    pub parameters: BTreeMap<String, TemplateParameter>,

    /// Path to the template file (set during loading)
    #[serde(skip)]
    pub template_path: PathBuf,

    /// Raw template content (everything after parameters section)
    #[serde(skip)]
    pub template_content: String,
}

impl RecipeTemplate {
    /// Loads a template from a file path
    pub fn load(path: &PathBuf) -> anyhow::Result<Self> {
        let config_str = match std::fs::read_to_string(path) {
            Ok(contents) => contents,
            Err(err) => bail!(
                "Recipe Template Load: Failed to read template file at '{}': {}. Check file existence and permissions.",
                path.display(),
                err
            ),
        };

        // Split the content into metadata, parameters, and template sections
        let (metadata_section, parameters_section, template_section) =
            Self::split_template_content(&config_str)?;

        // Parse the metadata section (name, description, extends)
        let metadata: Self = serde_yaml::from_str(&metadata_section)
            .map_err(|e| anyhow::anyhow!(
                "Recipe Template Load: Failed to parse template metadata at '{}': {}. Check YAML syntax.",
                path.display(), e
            ))?;

        // Parse the parameters section if it exists
        let parameters = if parameters_section.trim().is_empty() {
            BTreeMap::new()
        } else {
            serde_yaml::from_str(&parameters_section)
                .map_err(|e| anyhow::anyhow!(
                    "Recipe Template Load: Failed to parse template parameters at '{}': {}. Check YAML syntax.",
                    path.display(), e
                ))?
        };

        Ok(Self {
            name: metadata.name,
            description: metadata.description,
            extends: metadata.extends,
            parameters,
            template_path: path.clone(),
            template_content: template_section,
        })
    }

    /// Splits template content into metadata, parameters, and template sections
    /// Extracts both parameters and template blocks, treating the remainder as metadata
    fn split_template_content(content: &str) -> anyhow::Result<(String, String, String)> {
        let lines: Vec<&str> = content.lines().collect();

        // Extract parameters block first
        let (remaining_after_params, parameters_content) = extract_yaml_block(lines, "parameters");

        // Extract template block from what remains
        let (metadata_lines, template_content) =
            extract_yaml_block(remaining_after_params, "template");

        Ok((
            metadata_lines.join("\n"),
            parameters_content,
            template_content,
        ))
    }

    /// Validates parameters and returns resolved values with defaults
    pub fn resolve_parameters(
        &self,
        provided: &BTreeMap<String, Value>,
    ) -> anyhow::Result<BTreeMap<String, Value>> {
        let mut resolved = BTreeMap::new();

        // Start with defaults
        for (param_name, param_def) in &self.parameters {
            if let Some(default_value) = &param_def.default {
                resolved.insert(param_name.clone(), default_value.clone());
            }
        }

        // Override with provided values
        resolved.extend(provided.clone());

        // Validate required parameters
        for (param_name, param_def) in &self.parameters {
            if param_def.required && !resolved.contains_key(param_name) {
                bail!(
                    "Recipe Template: Required parameter '{}' is missing for template '{}'",
                    param_name,
                    self.name
                );
            }
        }

        // Basic type validation
        for (param_name, param_value) in &resolved {
            if let Some(param_def) = self.parameters.get(param_name) {
                self.validate_parameter_type(param_name, param_value, param_def)?;
            }
        }

        Ok(resolved)
    }

    /// Basic parameter type validation
    fn validate_parameter_type(
        &self,
        param_name: &str,
        value: &Value,
        param_def: &TemplateParameter,
    ) -> anyhow::Result<()> {
        let matches = matches!(
            (&param_def.parameter_type, value),
            (ParameterType::String, Value::String(_))
                | (ParameterType::Number, Value::Number(_))
                | (ParameterType::Boolean, Value::Bool(_))
                | (ParameterType::Array, Value::Sequence(_))
                | (ParameterType::Object, Value::Mapping(_))
        );

        if !matches {
            bail!(
                "Recipe Template: Parameter '{}' expected type {:?} but got {:?}",
                param_name,
                param_def.parameter_type,
                value
            );
        }

        Ok(())
    }

    /// Instantiates this template into a Recipe with the given parameters
    pub fn instantiate(
        &self,
        recipe_name: String,
        cookbook_name: String,
        config_path: PathBuf,
        project_root: PathBuf,
        parameters: &BTreeMap<String, Value>,
        context: &VariableContext,
    ) -> anyhow::Result<crate::project::Recipe> {
        // Resolve parameters with defaults and validate
        let resolved_params = self.resolve_parameters(parameters)?;

        // Use provided context (already has project and cookbook constants)
        let mut template_context = context.clone();

        // Add parameters to template context for rendering
        let params_json: serde_json::Map<String, serde_json::Value> = resolved_params
            .iter()
            .map(|(k, v)| (k.clone(), crate::template::VariableContext::yaml_to_json(v)))
            .collect();

        template_context
            .constants
            .insert("params".to_string(), serde_json::Value::Object(params_json));

        // Render the template content with parameters
        let rendered_template = template_context.render_raw_template(&self.template_content)?;

        // Parse the rendered YAML into a Recipe
        let mut recipe_value: serde_yaml::Value = serde_yaml::from_str(&rendered_template)
            .map_err(|e| anyhow::anyhow!(
                "Recipe Template: Failed to parse rendered template '{}': {}. Check template syntax and parameter usage.",
                self.name, e
            ))?;

        // Process any remaining template variables in the YAML structure
        VariableContext::process_template_in_value(&mut recipe_value, &template_context, true)?;

        // Deserialize into Recipe
        let mut recipe: crate::project::Recipe =
            serde_yaml::from_value(recipe_value).map_err(|e| {
                anyhow::anyhow!(
                    "Recipe Template: Failed to deserialize rendered template '{}' into recipe: {}",
                    self.name,
                    e
                )
            })?;

        // Set recipe metadata
        recipe.name = recipe_name;
        recipe.cookbook = cookbook_name;
        recipe.config_path = config_path;
        recipe.project_root = project_root;
        recipe.template = None; // Clear template field since this is instantiated
        recipe.parameters = std::collections::BTreeMap::new(); // Clear parameters since they've been processed

        // Process dependencies - add cookbook prefix if needed
        if let Some(deps) = recipe.dependencies.as_mut() {
            for dep in deps {
                if !dep.contains(':') {
                    *dep = format!("{}:{}", recipe.cookbook, dep);
                }
            }
        }

        Ok(recipe)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_yaml::Value;
    use std::path::PathBuf;
    use tempfile::tempdir;

    #[test]
    fn test_parameter_resolution() {
        let template = RecipeTemplate {
            name: "test-template".to_string(),
            description: Some("Test template".to_string()),
            extends: None,
            parameters: BTreeMap::from([
                (
                    "required_string".to_string(),
                    TemplateParameter {
                        parameter_type: ParameterType::String,
                        required: true,
                        default: None,
                        description: None,
                        pattern: None,
                        min: None,
                        max: None,
                        items: None,
                    },
                ),
                (
                    "optional_number".to_string(),
                    TemplateParameter {
                        parameter_type: ParameterType::Number,
                        required: false,
                        default: Some(Value::Number(serde_yaml::Number::from(42))),
                        description: None,
                        pattern: None,
                        min: None,
                        max: None,
                        items: None,
                    },
                ),
            ]),
            template_path: PathBuf::new(),
            template_content: "run: echo test".to_string(),
        };

        // Test missing required parameter
        let params = BTreeMap::new();
        assert!(template.resolve_parameters(&params).is_err());

        // Test valid parameters
        let params = BTreeMap::from([(
            "required_string".to_string(),
            Value::String("test".to_string()),
        )]);
        let resolved = template.resolve_parameters(&params).unwrap();
        assert_eq!(
            resolved.get("required_string"),
            Some(&Value::String("test".to_string()))
        );
        assert_eq!(
            resolved.get("optional_number"),
            Some(&Value::Number(serde_yaml::Number::from(42)))
        );

        // Test invalid parameter type
        let params = BTreeMap::from([(
            "required_string".to_string(),
            Value::Number(serde_yaml::Number::from(42)),
        )]);
        assert!(template.resolve_parameters(&params).is_err());
    }

    #[test]
    fn test_template_load() {
        let temp_dir = tempdir().unwrap();
        let template_path = temp_dir.path().join("test-template.yml");

        let template_content = r#"
name: "test-template"
description: "A test template"
parameters:
  service_name:
    type: string
    required: true
  port:
    type: number
    default: 3000
template:
  description: "Service {{ params.service_name }}"
  run: |
    echo "Starting {{ params.service_name }} on port {{ params.port }}"
"#;

        std::fs::write(&template_path, template_content).unwrap();

        let template = RecipeTemplate::load(&template_path).unwrap();
        assert_eq!(template.name, "test-template");
        assert_eq!(template.parameters.len(), 2);
        assert!(template.parameters.contains_key("service_name"));
        assert!(template.parameters.contains_key("port"));
    }

    #[test]
    fn test_split_template_content() {
        let content = r#"name: test-template
description: A test template
parameters:
  service_name:
    type: string
    required: true
template:
  description: "Service {{ params.service_name }}"
  run: echo "Starting {{ params.service_name }}"
"#;

        let (metadata, parameters, template) =
            RecipeTemplate::split_template_content(content).unwrap();

        // Metadata should only contain name and description
        assert!(metadata.contains("name: test-template"));
        assert!(metadata.contains("description: A test template"));
        assert!(!metadata.contains("parameters:"));
        assert!(!metadata.contains("template:"));

        // Parameters should contain the parameters block
        assert!(parameters.contains("service_name:"));
        assert!(parameters.contains("type: string"));
        assert!(parameters.contains("required: true"));
        assert!(!parameters.contains("name: test-template"));

        // Template should contain the template block
        assert!(template.contains("description: \"Service {{ params.service_name }}\""));
        assert!(template.contains("run: echo"));
        assert!(!template.contains("name: test-template"));
    }

    #[test]
    fn test_split_template_content_out_of_order() {
        // Test when template section comes first
        let content = r#"template:
  description: "Service {{ params.service_name }}"
  run: echo "Starting {{ params.service_name }}"
name: test-template
description: A test template
parameters:
  service_name:
    type: string
    required: true
"#;

        let (metadata, parameters, template) =
            RecipeTemplate::split_template_content(content).unwrap();

        // Metadata should contain only name and description
        assert!(metadata.contains("name: test-template"));
        assert!(metadata.contains("description: A test template"));
        assert!(!metadata.contains("parameters:"));
        assert!(!metadata.contains("template:"));

        // Parameters should contain the parameters block
        assert!(parameters.contains("service_name:"));
        assert!(parameters.contains("type: string"));
        assert!(parameters.contains("required: true"));

        // Template should only contain the template section content
        assert!(template.contains("description: \"Service {{ params.service_name }}\""));
        assert!(template.contains("run: echo"));
        assert!(!template.contains("name: test-template"));
    }

    #[test]
    fn test_split_template_content_no_template_section() {
        // Test when there's no template section
        let content = r#"name: test-template
description: A test template
parameters:
  service_name:
    type: string
    required: true
"#;

        let (metadata, parameters, template) =
            RecipeTemplate::split_template_content(content).unwrap();

        // Metadata should contain only name and description
        assert!(metadata.contains("name: test-template"));
        assert!(metadata.contains("description: A test template"));
        assert!(!metadata.contains("parameters:"));

        // Parameters should contain the parameters block
        assert!(parameters.contains("service_name:"));
        assert!(parameters.contains("type: string"));

        // Template should be empty
        assert!(template.trim().is_empty());
    }
}
