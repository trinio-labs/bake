use std::{collections::BTreeMap, path::PathBuf};

use anyhow::bail;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_yaml::Value;

use crate::template::VariableContext;

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

/// Represents the template definition part of a recipe template
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TemplateDefinition {
    /// Description of the recipe when instantiated
    pub description: Option<String>,
    
    /// Cache configuration for the recipe
    pub cache: Option<crate::project::RecipeCacheConfig>,
    
    /// Environment variables for the recipe
    #[serde(default)]
    pub environment: Vec<String>,
    
    /// Variables for the recipe
    #[serde(default)]
    pub variables: IndexMap<String, String>,
    
    /// Dependencies for the recipe
    pub dependencies: Option<Vec<String>>,
    
    /// The command to run
    pub run: String,
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
    
    /// The actual template definition
    pub template: TemplateDefinition,
    
    /// Path to the template file (set during loading)
    #[serde(skip)]
    pub template_path: PathBuf,
}

impl RecipeTemplate {
    /// Loads a template from a file path
    pub fn from_file(path: &PathBuf) -> anyhow::Result<Self> {
        let config_str = match std::fs::read_to_string(path) {
            Ok(contents) => contents,
            Err(err) => bail!(
                "Recipe Template Load: Failed to read template file at '{}': {}. Check file existence and permissions.",
                path.display(),
                err
            ),
        };

        match serde_yaml::from_str::<Self>(&config_str) {
            Ok(mut template) => {
                template.template_path = path.clone();
                Ok(template)
            }
            Err(err) => bail!(
                "Recipe Template Parse: Failed to parse template file at '{}': {}. Check YAML syntax and template structure.",
                path.display(),
                err
            ),
        }
    }

    /// Validates that all parameter values match their type definitions
    pub fn validate_parameters(&self, parameters: &BTreeMap<String, Value>) -> anyhow::Result<()> {
        // Check for required parameters
        for (param_name, param_def) in &self.parameters {
            if param_def.required && !parameters.contains_key(param_name)
                && param_def.default.is_none() {
                bail!(
                    "Recipe Template Validation: Required parameter '{}' is missing for template '{}'",
                    param_name,
                    self.name
                );
            }
        }

        // Validate each provided parameter
        for (param_name, param_value) in parameters {
            if let Some(param_def) = self.parameters.get(param_name) {
                Self::validate_parameter_value(param_name, param_value, param_def)?;
            } else {
                bail!(
                    "Recipe Template Validation: Unknown parameter '{}' for template '{}'",
                    param_name,
                    self.name
                );
            }
        }

        Ok(())
    }

    /// Validates a single parameter value against its definition
    fn validate_parameter_value(
        param_name: &str,
        value: &Value,
        param_def: &TemplateParameter,
    ) -> anyhow::Result<()> {
        match (&param_def.parameter_type, value) {
            (ParameterType::String, Value::String(s)) => {
                if let Some(pattern) = &param_def.pattern {
                    let regex = regex::Regex::new(pattern).map_err(|e| {
                        anyhow::anyhow!(
                            "Recipe Template Validation: Invalid regex pattern '{}' for parameter '{}': {}",
                            pattern,
                            param_name,
                            e
                        )
                    })?;
                    if !regex.is_match(s) {
                        bail!(
                            "Recipe Template Validation: Parameter '{}' value '{}' does not match pattern '{}'",
                            param_name,
                            s,
                            pattern
                        );
                    }
                }
            }
            (ParameterType::Number, Value::Number(n)) => {
                let num_val = n.as_f64().ok_or_else(|| {
                    anyhow::anyhow!("Recipe Template Validation: Invalid number format for parameter '{}'", param_name)
                })?;
                
                if let Some(min) = param_def.min {
                    if num_val < min {
                        bail!(
                            "Recipe Template Validation: Parameter '{}' value {} is less than minimum {}",
                            param_name,
                            num_val,
                            min
                        );
                    }
                }
                
                if let Some(max) = param_def.max {
                    if num_val > max {
                        bail!(
                            "Recipe Template Validation: Parameter '{}' value {} is greater than maximum {}",
                            param_name,
                            num_val,
                            max
                        );
                    }
                }
            }
            (ParameterType::Boolean, Value::Bool(_)) => {
                // Boolean validation is implicit
            }
            (ParameterType::Array, Value::Sequence(seq)) => {
                if let Some(item_def) = &param_def.items {
                    for (index, item) in seq.iter().enumerate() {
                        Self::validate_parameter_value(
                            &format!("{param_name}[{index}]"),
                            item,
                            item_def,
                        )?;
                    }
                }
            }
            (ParameterType::Object, Value::Mapping(_)) => {
                // Object validation could be enhanced with schema validation
            }
            (expected_type, actual_value) => {
                bail!(
                    "Recipe Template Validation: Parameter '{}' expected type {:?} but got {:?}",
                    param_name,
                    expected_type,
                    actual_value
                );
            }
        }

        Ok(())
    }

    /// Resolves parameters with defaults, returning the final parameter values
    pub fn resolve_parameters(&self, provided: &BTreeMap<String, Value>) -> anyhow::Result<BTreeMap<String, Value>> {
        let mut resolved = BTreeMap::new();

        // Start with defaults
        for (param_name, param_def) in &self.parameters {
            if let Some(default_value) = &param_def.default {
                resolved.insert(param_name.clone(), default_value.clone());
            }
        }

        // Override with provided values
        resolved.extend(provided.clone());

        // Validate the final parameter set
        self.validate_parameters(&resolved)?;

        Ok(resolved)
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
        // Resolve parameters with defaults
        let resolved_params = self.resolve_parameters(parameters)?;

        // Create a new variable context with template parameters
        let mut template_context = context.clone();
        
        // Add template parameters to the context as 'params'
        let params_map: IndexMap<String, String> = resolved_params
            .iter()
            .map(|(k, v)| {
                let value_str = match v {
                    Value::String(s) => s.clone(),
                    Value::Number(n) => n.to_string(),
                    Value::Bool(b) => b.to_string(),
                    Value::Sequence(_) => serde_yaml::to_string(v).unwrap_or_default().trim().to_string(),
                    Value::Mapping(_) => serde_yaml::to_string(v).unwrap_or_default().trim().to_string(),
                    Value::Null => "null".to_string(),
                    Value::Tagged(tagged) => serde_yaml::to_string(&tagged.value).unwrap_or_default().trim().to_string(),
                };
                (k.clone(), value_str)
            })
            .collect();

        template_context.constants.insert("params".to_string(), params_map);

        // Process the template definition with parameter substitution
        let description = if let Some(desc) = &self.template.description {
            Some(template_context.parse_template(desc)?)
        } else {
            None
        };

        let run_command = template_context.parse_template(&self.template.run)?;

        // Process variables
        let processed_variables = self.template.variables
            .iter()
            .try_fold(IndexMap::new(), |mut acc, (k, v)| {
                let processed_value = template_context.parse_template(v)?;
                acc.insert(k.clone(), processed_value);
                Ok::<IndexMap<String, String>, anyhow::Error>(acc)
            })?;

        // Process dependencies
        let processed_dependencies = if let Some(deps) = &self.template.dependencies {
            Some(
                deps.iter()
                    .map(|dep| template_context.parse_template(dep))
                    .collect::<Result<Vec<String>, _>>()?
                    .into_iter()
                    .map(|dep| {
                        // Apply same dependency resolution as regular recipes
                        if !dep.contains(':') {
                            format!("{cookbook_name}:{dep}")
                        } else {
                            dep
                        }
                    })
                    .collect(),
            )
        } else {
            None
        };

        // Create the recipe
        Ok(crate::project::Recipe {
            name: recipe_name,
            cookbook: cookbook_name,
            config_path,
            project_root,
            cache: self.template.cache.clone(),
            description,
            variables: processed_variables,
            environment: self.template.environment.clone(),
            dependencies: processed_dependencies,
            run: run_command,
            template: None, // Clear template field since this is an instantiated recipe
            parameters: std::collections::BTreeMap::new(), // Clear parameters since they've been processed
            run_status: Default::default(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_yaml::Value;
    use std::path::PathBuf;
    use tempfile::tempdir;

    #[test]
    fn test_template_parameter_validation() {
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
                        min: Some(0.0),
                        max: Some(100.0),
                        items: None,
                    },
                ),
            ]),
            template: TemplateDefinition {
                description: Some("Test template".to_string()),
                cache: None,
                environment: vec![],
                variables: IndexMap::new(),
                dependencies: None,
                run: "echo test".to_string(),
            },
            template_path: PathBuf::new(),
        };

        // Test missing required parameter
        let params = BTreeMap::new();
        assert!(template.validate_parameters(&params).is_err());

        // Test valid parameters
        let params = BTreeMap::from([
            ("required_string".to_string(), Value::String("test".to_string())),
            ("optional_number".to_string(), Value::Number(serde_yaml::Number::from(50))),
        ]);
        assert!(template.validate_parameters(&params).is_ok());

        // Test invalid parameter type
        let params = BTreeMap::from([
            ("required_string".to_string(), Value::Number(serde_yaml::Number::from(42))),
        ]);
        assert!(template.validate_parameters(&params).is_err());

        // Test number out of range
        let params = BTreeMap::from([
            ("required_string".to_string(), Value::String("test".to_string())),
            ("optional_number".to_string(), Value::Number(serde_yaml::Number::from(150))),
        ]);
        assert!(template.validate_parameters(&params).is_err());
    }

    #[test]
    fn test_template_from_file() {
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

        let template = RecipeTemplate::from_file(&template_path).unwrap();
        assert_eq!(template.name, "test-template");
        assert_eq!(template.parameters.len(), 2);
        assert!(template.parameters.contains_key("service_name"));
        assert!(template.parameters.contains_key("port"));
    }

    #[test]
    fn test_parameter_resolution() {
        let template = RecipeTemplate {
            name: "test-template".to_string(),
            description: None,
            extends: None,
            parameters: BTreeMap::from([
                (
                    "required_param".to_string(),
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
                    "optional_param".to_string(),
                    TemplateParameter {
                        parameter_type: ParameterType::String,
                        required: false,
                        default: Some(Value::String("default_value".to_string())),
                        description: None,
                        pattern: None,
                        min: None,
                        max: None,
                        items: None,
                    },
                ),
            ]),
            template: TemplateDefinition {
                description: None,
                cache: None,
                environment: vec![],
                variables: IndexMap::new(),
                dependencies: None,
                run: "echo test".to_string(),
            },
            template_path: PathBuf::new(),
        };

        let provided = BTreeMap::from([
            ("required_param".to_string(), Value::String("provided_value".to_string())),
        ]);

        let resolved = template.resolve_parameters(&provided).unwrap();
        assert_eq!(resolved.len(), 2);
        assert_eq!(resolved.get("required_param"), Some(&Value::String("provided_value".to_string())));
        assert_eq!(resolved.get("optional_param"), Some(&Value::String("default_value".to_string())));
    }
}