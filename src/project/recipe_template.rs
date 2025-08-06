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

        // Try normal YAML parsing first
        match serde_yaml::from_str::<Self>(&config_str) {
            Ok(mut template) => {
                template.template_path = path.clone();
                Ok(template)
            }
            Err(_) => {
                // If normal parsing fails, it might contain handlebars control structures
                // Try to extract metadata (name, description, parameters) from the non-template sections
                let (name, description, parameters) = Self::extract_template_metadata(&config_str)?;
                
                Ok(Self {
                    name,
                    description,
                    extends: None,
                    parameters,
                    template: TemplateDefinition {
                        description: None,
                        cache: None,
                        environment: vec![],
                        variables: IndexMap::new(),
                        dependencies: None,
                        run: "# Template will be processed during instantiation".to_string(),
                    },
                    template_path: path.clone(),
                })
            }
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

    /// Extracts template metadata (name, description, parameters) from YAML content with handlebars
    fn extract_template_metadata(yaml_content: &str) -> anyhow::Result<(String, Option<String>, BTreeMap<String, TemplateParameter>)> {
        let lines: Vec<&str> = yaml_content.lines().collect();
        let mut metadata_lines = Vec::new();
        
        // Extract everything before the template section
        for line in lines {
            let trimmed = line.trim();
            if trimmed.starts_with("template:") {
                break;
            }
            metadata_lines.push(line);
        }
        
        let metadata_yaml = metadata_lines.join("\n");
        
        // Try to parse the metadata section
        if let Ok(serde_yaml::Value::Mapping(map)) = serde_yaml::from_str::<serde_yaml::Value>(&metadata_yaml) {
            let name = map.get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown-template")
                .to_string();
                
            let description = map.get("description")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
                
            let mut parameters = BTreeMap::new();
            if let Some(serde_yaml::Value::Mapping(params_map)) = map.get("parameters") {
                for (key, value) in params_map {
                    if let Some(param_name) = key.as_str() {
                        if let Ok(param_def) = serde_yaml::from_value::<TemplateParameter>(value.clone()) {
                            parameters.insert(param_name.to_string(), param_def);
                        }
                    }
                }
            }
            
            return Ok((name, description, parameters));
        }
        
        // Fallback: extract name from filename if metadata parsing fails
        Ok(("unknown-template".to_string(), None, BTreeMap::new()))
    }

    /// Extracts and parses the template section with handlebars rendering
    fn extract_and_parse_template_section(yaml_content: &str, context: &VariableContext) -> anyhow::Result<TemplateDefinition> {
        let lines: Vec<&str> = yaml_content.lines().collect();
        let mut template_lines = Vec::new();
        let mut in_template_section = false;
        let mut template_indent = 0;
        
        for line in lines {
            let trimmed = line.trim();
            
            // Check if we're entering the template section
            if trimmed.starts_with("template:") {
                in_template_section = true;
                template_indent = line.len() - line.trim_start().len();
                template_lines.push("template:".to_string()); // Add section header
                continue;
            }
            
            if in_template_section {
                let line_indent = line.len() - line.trim_start().len();
                
                // If we hit a line with same or less indentation than template, we've left the section
                if line_indent <= template_indent && !trimmed.is_empty() && !trimmed.starts_with('#') {
                    break;
                }
                
                // Add the line to template section
                template_lines.push(line.to_string());
            }
        }
        
        let template_yaml = template_lines.join("\n");
        
        // Render handlebars in the template section
        let rendered_template = context.render_raw_template(&template_yaml)?;
        
        // Parse the rendered template section
        if let Ok(serde_yaml::Value::Mapping(map)) = serde_yaml::from_str::<serde_yaml::Value>(&rendered_template) {
            if let Some(template_value) = map.get("template") {
                return serde_yaml::from_value::<TemplateDefinition>(template_value.clone())
                    .map_err(|e| anyhow::anyhow!("Failed to parse template definition: {}", e));
            }
        }
        
        bail!("Failed to extract and parse template section");
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
        
        // Convert resolved parameters to JSON values for structured constants
        let params_json = resolved_params
            .iter()
            .map(|(k, v)| {
                let json_value = match v {
                    Value::String(s) => serde_json::Value::String(s.clone()),
                    Value::Number(n) => {
                        if let Some(i) = n.as_i64() {
                            serde_json::Value::Number(serde_json::Number::from(i))
                        } else if let Some(f) = n.as_f64() {
                            serde_json::Value::Number(serde_json::Number::from_f64(f).unwrap_or(serde_json::Number::from(0)))
                        } else {
                            serde_json::Value::Number(serde_json::Number::from(0))
                        }
                    },
                    Value::Bool(b) => serde_json::Value::Bool(*b),
                    Value::Sequence(seq) => {
                        let json_seq: Vec<serde_json::Value> = seq.iter().map(|item| {
                            // Convert each item in the sequence
                            match item {
                                Value::String(s) => serde_json::Value::String(s.clone()),
                                Value::Number(n) => {
                                    if let Some(i) = n.as_i64() {
                                        serde_json::Value::Number(serde_json::Number::from(i))
                                    } else if let Some(f) = n.as_f64() {
                                        serde_json::Value::Number(serde_json::Number::from_f64(f).unwrap_or(serde_json::Number::from(0)))
                                    } else {
                                        serde_json::Value::Number(serde_json::Number::from(0))
                                    }
                                },
                                Value::Bool(b) => serde_json::Value::Bool(*b),
                                Value::Null => serde_json::Value::Null,
                                _ => serde_json::Value::String(serde_yaml::to_string(item).unwrap_or_default().trim().to_string()),
                            }
                        }).collect();
                        serde_json::Value::Array(json_seq)
                    },
                    Value::Mapping(_) => {
                        // Convert YAML mapping to JSON object
                        serde_yaml::from_value(v.clone()).unwrap_or(serde_json::Value::Object(Default::default()))
                    },
                    Value::Null => serde_json::Value::Null,
                    Value::Tagged(tagged) => {
                        // Handle tagged values by converting the inner value
                        serde_yaml::from_value(tagged.value.clone()).unwrap_or(serde_json::Value::Null)
                    },
                };
                (k.clone(), json_value)
            })
            .collect();

        template_context.constants.insert("params".to_string(), serde_json::Value::Object(params_json));

        // Check if this template needs template-first parsing (has placeholder run command)
        let template_def = if self.template.run == "# Template will be processed during instantiation" {
            // Re-read the template file and do template-first parsing
            let template_content = std::fs::read_to_string(&self.template_path)
                .map_err(|e| anyhow::anyhow!("Failed to read template file '{}': {}", self.template_path.display(), e))?;
            
            // Extract and parse the template section with handlebars rendering
            Self::extract_and_parse_template_section(&template_content, &template_context)?
        } else {
            // Use the already parsed template definition
            self.template.clone()
        };

        // Process the template definition with parameter substitution
        let description = if let Some(desc) = &template_def.description {
            Some(template_context.parse_template(desc)?)
        } else {
            None
        };

        let run_command = template_context.parse_template(&template_def.run)?;

        // Process variables
        let processed_variables = template_def.variables
            .iter()
            .try_fold(IndexMap::new(), |mut acc, (k, v)| {
                let processed_value = template_context.parse_template(v)?;
                acc.insert(k.clone(), processed_value);
                Ok::<IndexMap<String, String>, anyhow::Error>(acc)
            })?;

        // Process dependencies
        let processed_dependencies = if let Some(deps) = &template_def.dependencies {
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
            cache: template_def.cache.clone(),
            description,
            variables: processed_variables,
            environment: template_def.environment.clone(),
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