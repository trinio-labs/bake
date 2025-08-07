use std::fs;

use tempfile::tempdir;

use bake::template::VariableFileLoader;

#[test]
fn test_variable_file_loader() {
    let temp_dir = tempdir().unwrap();
    let vars_file = temp_dir.path().join("vars.yml");

    // Create a test variable file with new standardized format
    let vars_content = r#"
default:
  api_url: "https://api.example.com"
  debug: false
  port: 8080

envs:
  dev:
    api_url: "https://dev-api.example.com"
    debug: true
    port: 3000
  
  prod:
    api_url: "https://prod-api.example.com"
    port: 443
"#;
    fs::write(&vars_file, vars_content).unwrap();

    // Test loading default environment
    let default_vars =
        VariableFileLoader::load_variables_from_file(&vars_file, Some("default")).unwrap();
    assert_eq!(
        default_vars.get("api_url"),
        Some(&serde_yaml::Value::String(
            "https://api.example.com".to_string()
        ))
    );
    assert_eq!(
        default_vars.get("debug"),
        Some(&serde_yaml::Value::Bool(false))
    );
    assert_eq!(
        default_vars.get("port"),
        Some(&serde_yaml::Value::Number(serde_yaml::Number::from(8080)))
    );

    // Test loading dev environment
    let dev_vars = VariableFileLoader::load_variables_from_file(&vars_file, Some("dev")).unwrap();
    assert_eq!(
        dev_vars.get("api_url"),
        Some(&serde_yaml::Value::String(
            "https://dev-api.example.com".to_string()
        ))
    );
    assert_eq!(dev_vars.get("debug"), Some(&serde_yaml::Value::Bool(true)));
    assert_eq!(
        dev_vars.get("port"),
        Some(&serde_yaml::Value::Number(serde_yaml::Number::from(3000)))
    );

    // Test loading prod environment
    let prod_vars = VariableFileLoader::load_variables_from_file(&vars_file, Some("prod")).unwrap();
    assert_eq!(
        prod_vars.get("api_url"),
        Some(&serde_yaml::Value::String(
            "https://prod-api.example.com".to_string()
        ))
    );
    assert_eq!(
        prod_vars.get("debug"),
        Some(&serde_yaml::Value::Bool(false))
    ); // inherited from default
    assert_eq!(
        prod_vars.get("port"),
        Some(&serde_yaml::Value::Number(serde_yaml::Number::from(443)))
    );

    // Test loading non-existent environment - should succeed with defaults only
    let result = VariableFileLoader::load_variables_from_file(&vars_file, Some("staging"));
    assert!(result.is_ok());
    let staging_vars = result.unwrap();

    // Should only have default values since "staging" environment doesn't exist
    assert_eq!(
        staging_vars.get("api_url"),
        Some(&serde_yaml::Value::String(
            "https://api.example.com".to_string()
        ))
    );
    assert_eq!(
        staging_vars.get("debug"),
        Some(&serde_yaml::Value::Bool(false))
    );
    assert_eq!(
        staging_vars.get("port"),
        Some(&serde_yaml::Value::Number(serde_yaml::Number::from(8080)))
    );
}

#[test]
fn test_variable_file_loader_directory_search() {
    let temp_dir = tempdir().unwrap();

    // Create vars.yml file
    let vars_file = temp_dir.path().join("vars.yml");
    let vars_content = r#"
default:
  database_url: "localhost:5432"
  cache_enabled: true

envs:
  dev:
    database_url: "dev-db:5432"
    debug: true
"#;
    fs::write(&vars_file, vars_content).unwrap();

    // Test loading from directory
    let vars =
        VariableFileLoader::load_variables_from_directory(temp_dir.path(), Some("dev")).unwrap();

    assert_eq!(
        vars.get("database_url"),
        Some(&serde_yaml::Value::String("dev-db:5432".to_string()))
    );
    assert_eq!(vars.get("debug"), Some(&serde_yaml::Value::Bool(true)));
    assert_eq!(
        vars.get("cache_enabled"),
        Some(&serde_yaml::Value::Bool(true))
    );

    // Test with empty directory (should return empty map)
    let empty_dir = temp_dir.path().join("empty");
    fs::create_dir(&empty_dir).unwrap();
    let empty_vars =
        VariableFileLoader::load_variables_from_directory(&empty_dir, Some("dev")).unwrap();
    assert!(empty_vars.is_empty());
}

#[test]
fn test_variable_context_builder_with_environment() {
    let temp_dir = tempdir().unwrap();
    let vars_file = temp_dir.path().join("variables.yml");

    // Create a more complex variable file for testing context builder
    let vars_content = r#"
default:
  base_url: "https://example.com"
  timeout: 30
  features:
    - "auth"
    - "logging"

envs:
  staging:
    base_url: "https://staging.example.com"
    timeout: 60
    debug_mode: true
"#;
    fs::write(&vars_file, vars_content).unwrap();

    // Test loading variables with context builder - need to use builder pattern
    use bake::template::VariableContextBuilder;
    let context = VariableContextBuilder::new()
        .variables_from_directory(temp_dir.path(), Some("staging"))
        .unwrap()
        .build();

    let variables = context.process_variables().unwrap();

    // Check staging-specific values
    assert_eq!(
        variables.get("base_url"),
        Some(&serde_yaml::Value::String(
            "https://staging.example.com".to_string()
        ))
    );
    assert_eq!(
        variables.get("timeout"),
        Some(&serde_yaml::Value::Number(serde_yaml::Number::from(60)))
    );
    assert_eq!(
        variables.get("debug_mode"),
        Some(&serde_yaml::Value::Bool(true))
    );

    // Check inherited values from default
    assert!(variables.contains_key("features"));
    if let Some(serde_yaml::Value::Sequence(features)) = variables.get("features") {
        assert_eq!(features.len(), 2);
    } else {
        panic!("Expected features to be a sequence");
    }
}

#[test]
fn test_environment_inheritance() {
    let temp_dir = tempdir().unwrap();
    let vars_file = temp_dir.path().join("vars.yml");

    // Test environment inheritance behavior
    let vars_content = r#"
default:
  api_key: "default_key"
  rate_limit: 1000
  endpoints:
    - "users"
    - "posts"

envs:
  development:
    api_key: "dev_key"
    debug: true
    # rate_limit should be inherited from default
    # endpoints should be inherited from default
"#;
    fs::write(&vars_file, vars_content).unwrap();

    let dev_vars =
        VariableFileLoader::load_variables_from_file(&vars_file, Some("development")).unwrap();

    // Check overridden values
    assert_eq!(
        dev_vars.get("api_key"),
        Some(&serde_yaml::Value::String("dev_key".to_string()))
    );
    assert_eq!(dev_vars.get("debug"), Some(&serde_yaml::Value::Bool(true)));

    // Check inherited values
    assert_eq!(
        dev_vars.get("rate_limit"),
        Some(&serde_yaml::Value::Number(serde_yaml::Number::from(1000)))
    );
    assert!(dev_vars.contains_key("endpoints"));

    // Verify endpoints array was inherited correctly
    if let Some(serde_yaml::Value::Sequence(endpoints)) = dev_vars.get("endpoints") {
        assert_eq!(endpoints.len(), 2);
        assert_eq!(endpoints[0], serde_yaml::Value::String("users".to_string()));
        assert_eq!(endpoints[1], serde_yaml::Value::String("posts".to_string()));
    } else {
        panic!("Expected endpoints to be inherited as sequence");
    }
}
