use std::{collections::BTreeMap, path::PathBuf};

use anyhow::bail;
use serde::{Deserialize, Serialize};
use serde_json::json;
use serde_yaml::Value;

use crate::template::VariableContext;

/// Return type for a custom helper
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum HelperReturnType {
    #[default]
    String,
    Array,
}

/// Represents a parameter type for helper validation (reuse from recipe templates)
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ParameterType {
    String,
    Number,
    Boolean,
    Array,
    Object,
}

/// Represents a helper parameter definition with validation rules
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HelperParameter {
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
}

/// Represents a custom Handlebars helper defined in YAML
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Helper {
    /// Name of the helper (must match filename)
    pub name: String,

    /// Description of what this helper does
    pub description: Option<String>,

    /// Return type (string or array)
    #[serde(default)]
    pub returns: HelperReturnType,

    /// Parameters that can be passed to this helper
    #[serde(default)]
    pub parameters: BTreeMap<String, HelperParameter>,

    /// Helper-specific variables
    #[serde(default)]
    pub variables: BTreeMap<String, Value>,

    /// Environment variables the helper needs
    #[serde(default)]
    pub environment: Vec<String>,

    /// The script to execute (with template substitution)
    pub run: String,

    /// Path to the helper file (set during loading)
    #[serde(skip)]
    pub helper_path: PathBuf,
}

impl Helper {
    /// Loads a helper from a file path
    pub fn from_file(path: &PathBuf) -> anyhow::Result<Self> {
        let config_str = match std::fs::read_to_string(path) {
            Ok(contents) => contents,
            Err(err) => bail!(
                "Helper Load: Failed to read helper file at '{}': {}. Check file existence and permissions.",
                path.display(),
                err
            ),
        };

        let mut helper: Self = serde_yaml::from_str(&config_str).map_err(|e| {
            anyhow::anyhow!(
                "Helper Load: Failed to parse helper at '{}': {}. Check YAML syntax.",
                path.display(),
                e
            )
        })?;

        // Validate that filename matches helper name
        if let Some(file_stem) = path.file_stem() {
            if let Some(file_name) = file_stem.to_str() {
                if file_name != helper.name {
                    bail!(
                        "Helper Load: Helper name '{}' doesn't match filename '{}'. They must be identical.",
                        helper.name,
                        file_name
                    );
                }
            }
        }

        helper.helper_path = path.clone();
        Ok(helper)
    }

    /// Resolves parameters by applying defaults and validating required parameters
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
                    "Helper '{}': Required parameter '{}' is missing",
                    self.name,
                    param_name
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
        param_def: &HelperParameter,
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
                "Helper '{}': Parameter '{}' expected type {:?} but got {:?}",
                self.name,
                param_name,
                param_def.parameter_type,
                value
            );
        }

        Ok(())
    }

    /// Builds a template context for rendering the helper's run script
    pub fn build_context(
        &self,
        base_context: &VariableContext,
        resolved_params: &BTreeMap<String, Value>,
    ) -> VariableContext {
        let mut context = base_context.clone();

        // Add params as a constant (similar to recipe templates)
        context.constants.insert(
            "params".to_owned(),
            json!(resolved_params
                .iter()
                .map(|(k, v)| (k.clone(), VariableContext::yaml_to_json(v)))
                .collect::<BTreeMap<String, serde_json::Value>>()),
        );

        // Add helper-specific variables
        if !self.variables.is_empty() {
            context.variables.extend(
                self.variables
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect::<BTreeMap<String, Value>>(),
            );
        }

        // Add helper-specific environment variables
        if !self.environment.is_empty() {
            context.environment.extend(self.environment.iter().cloned());
        }

        context
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_helper_from_file() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test-helper.yml");

        let helper_yaml = r#"
name: "test-helper"
description: "A test helper"
returns: string
parameters:
  arg1:
    type: string
    required: true
  arg2:
    type: number
    default: 42
run: |
  echo "{{params.arg1}} {{params.arg2}}"
"#;
        std::fs::write(&file_path, helper_yaml).unwrap();

        let helper = Helper::from_file(&file_path).unwrap();

        assert_eq!(helper.name, "test-helper");
        assert_eq!(helper.description, Some("A test helper".to_string()));
        assert_eq!(helper.returns, HelperReturnType::String);
        assert_eq!(helper.parameters.len(), 2);
        assert!(helper.parameters.contains_key("arg1"));
        assert!(helper.parameters.contains_key("arg2"));
    }

    #[test]
    fn test_resolve_parameters_with_defaults() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test-helper.yml");

        let helper_yaml = r#"
name: "test-helper"
returns: string
parameters:
  required_param:
    type: string
    required: true
  optional_param:
    type: string
    default: "default_value"
run: echo "test"
"#;
        std::fs::write(&file_path, helper_yaml).unwrap();

        let helper = Helper::from_file(&file_path).unwrap();

        let provided = BTreeMap::from([(
            "required_param".to_string(),
            Value::String("provided".to_string()),
        )]);

        let resolved = helper.resolve_parameters(&provided).unwrap();

        assert_eq!(
            resolved.get("required_param"),
            Some(&Value::String("provided".to_string()))
        );
        assert_eq!(
            resolved.get("optional_param"),
            Some(&Value::String("default_value".to_string()))
        );
    }

    #[test]
    fn test_resolve_parameters_missing_required() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test-helper.yml");

        let helper_yaml = r#"
name: "test-helper"
returns: string
parameters:
  required_param:
    type: string
    required: true
run: echo "test"
"#;
        std::fs::write(&file_path, helper_yaml).unwrap();

        let helper = Helper::from_file(&file_path).unwrap();

        let provided = BTreeMap::new();
        let result = helper.resolve_parameters(&provided);

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Required parameter 'required_param' is missing"));
    }

    #[test]
    fn test_validate_parameter_type() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test-helper.yml");

        let helper_yaml = r#"
name: "test-helper"
returns: string
parameters:
  string_param:
    type: string
  number_param:
    type: number
run: echo "test"
"#;
        std::fs::write(&file_path, helper_yaml).unwrap();

        let helper = Helper::from_file(&file_path).unwrap();

        // Valid types
        let valid_params = BTreeMap::from([
            (
                "string_param".to_string(),
                Value::String("test".to_string()),
            ),
            (
                "number_param".to_string(),
                Value::Number(serde_yaml::Number::from(42)),
            ),
        ]);
        assert!(helper.resolve_parameters(&valid_params).is_ok());

        // Invalid type
        let invalid_params = BTreeMap::from([(
            "number_param".to_string(),
            Value::String("not a number".to_string()),
        )]);
        let result = helper.resolve_parameters(&invalid_params);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("expected type Number"));
    }

    #[test]
    fn test_helper_returns_array() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test-helper.yml");

        let helper_yaml = r#"
name: "test-helper"
returns: array
run: echo "line1\nline2"
"#;
        std::fs::write(&file_path, helper_yaml).unwrap();

        let helper = Helper::from_file(&file_path).unwrap();

        assert_eq!(helper.returns, HelperReturnType::Array);
    }
}
