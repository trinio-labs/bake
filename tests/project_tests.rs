use std::{os::unix::prelude::PermissionsExt, path::PathBuf};

use indexmap::IndexMap;
use tempfile::tempdir;
use test_case::test_case;

use bake::project::BakeProject;

mod common;

fn config_path(path_str: &str) -> String {
    env!("CARGO_MANIFEST_DIR").to_owned() + "/resources/tests" + path_str
}

fn validate_project(project_result: anyhow::Result<BakeProject>) {
    let project = project_result.unwrap();
    assert_eq!(project.name, "test");
    assert_eq!(
        project.variables.get("bake_project_var"),
        Some(&serde_yaml::Value::String("bar".to_string()))
    );
}

#[test_case("/valid/"; "valid project")]
fn test_project_loading(path: &str) {
    let result = BakeProject::load(
        &PathBuf::from(config_path(path)),
        None,
        IndexMap::new(),
        false,
    );
    validate_project(result);
}

#[test]
fn test_invalid_permission_handling() {
    let path = config_path("/invalid/permission/bake.yml");
    let mut perms = std::fs::metadata(&path).unwrap().permissions();
    let mode = perms.mode();
    perms.set_mode(0o200);
    std::fs::set_permissions(&path, perms.clone()).unwrap();

    let project = BakeProject::load(
        &PathBuf::from(config_path("/invalid/permission")),
        None,
        IndexMap::new(),
        false,
    );

    assert!(project.is_err());

    // Restore original permissions
    perms.set_mode(mode);
    std::fs::set_permissions(&path, perms).unwrap();
}

#[test]
fn test_min_version_validation() {
    use std::fs;

    let temp_dir = tempdir().unwrap();
    let config_path = temp_dir.path().join("bake.yml");

    // Create a test configuration with a specific minimum version
    let config_content = r#"
name: test_project
config:
  minVersion: "0.4.0"
variables:
  test_var: "test_value"
"#;

    fs::write(&config_path, config_content).unwrap();

    // Test that version validation works
    let result = BakeProject::load(temp_dir.path(), None, IndexMap::new(), false);
    assert!(result.is_ok());

    let project = result.unwrap();
    assert_eq!(project.config.min_version, Some("0.4.0".to_string()));
}

#[test]
fn test_project_with_custom_variables() {
    let temp_dir = tempdir().unwrap();
    let config_path = temp_dir.path().join("bake.yml");

    // Create bake.yml with inline variables
    let config_content = r#"
name: custom_var_test

variables:
  custom_var: "original_value"
  another_var: 42
"#;
    std::fs::write(&config_path, config_content).unwrap();

    // Test with override variables
    let mut override_vars = IndexMap::new();
    override_vars.insert("custom_var".to_string(), "overridden_value".to_string());

    let result = BakeProject::load(temp_dir.path(), None, override_vars, false);
    assert!(result.is_ok());

    let project = result.unwrap();
    assert_eq!(project.name, "custom_var_test");

    // Variables should be loaded from inline variables
    assert!(project.processed_variables.contains_key("custom_var"));
    assert!(project.processed_variables.contains_key("another_var"));
}

#[test]
fn test_project_with_cookbooks() {
    let temp_dir = tempdir().unwrap();

    // Create main bake.yml
    let bake_config = r#"
name: multi_cookbook_test
cookbooks:
  - path: "./frontend"
  - path: "./backend"
variables:
  global_var: "global_value"
"#;
    std::fs::write(temp_dir.path().join("bake.yml"), bake_config).unwrap();

    // Create frontend cookbook
    std::fs::create_dir(temp_dir.path().join("frontend")).unwrap();
    let frontend_config = r#"
name: frontend
variables:
  port: 3000
recipes:
  build:
    description: "Build frontend"
    run: "npm run build"
  test:
    description: "Test frontend"  
    run: "npm test"
    dependencies: [build]
"#;
    std::fs::write(
        temp_dir.path().join("frontend").join("cookbook.yml"),
        frontend_config,
    )
    .unwrap();

    // Create backend cookbook
    std::fs::create_dir(temp_dir.path().join("backend")).unwrap();
    let backend_config = r#"
name: backend
variables:
  port: 8000
recipes:
  build:
    description: "Build backend"
    run: "go build"
  test:
    description: "Test backend"
    run: "go test"
    dependencies: [build]
"#;
    std::fs::write(
        temp_dir.path().join("backend").join("cookbook.yml"),
        backend_config,
    )
    .unwrap();

    let result = BakeProject::load(temp_dir.path(), None, IndexMap::new(), false);
    assert!(result.is_ok());

    let project = result.unwrap();
    assert_eq!(project.name, "multi_cookbook_test");
    assert_eq!(project.cookbooks.len(), 2);
    assert!(project.cookbooks.contains_key("frontend"));
    assert!(project.cookbooks.contains_key("backend"));

    // Check that recipes were loaded correctly
    let frontend = project.cookbooks.get("frontend").unwrap();
    assert!(frontend.recipes.contains_key("build"));
    assert!(frontend.recipes.contains_key("test"));

    let backend = project.cookbooks.get("backend").unwrap();
    assert!(backend.recipes.contains_key("build"));
    assert!(backend.recipes.contains_key("test"));
}

#[test_case("/invalid/circular"; "circular dependency")]
#[test_case("/invalid/recipes"; "invalid recipes")]
#[test_case("/invalid/nobake"; "no bake file")]
fn test_invalid_project_configurations(path: &str) {
    let result = BakeProject::load(
        &PathBuf::from(config_path(path)),
        None,
        IndexMap::new(),
        false,
    );
    assert!(result.is_err(), "Expected error for path: {path}");
}

#[test]
fn test_template_discovery() {
    let temp_dir = tempdir().unwrap();

    // Create main project
    let bake_config = r#"
name: template_test
"#;
    std::fs::write(temp_dir.path().join("bake.yml"), bake_config).unwrap();

    // Create templates directory with a test template
    let templates_dir = temp_dir.path().join(".bake").join("templates");
    std::fs::create_dir_all(&templates_dir).unwrap();

    let template_config = r#"
name: "test-template"
description: "Test template"
parameters:
  name:
    type: string
    required: true
    description: "Component name"

content: |
  recipes:
    build-{{name}}:
      description: "Build {{name}}"
      run: "echo Building {{name}}"
"#;
    std::fs::write(templates_dir.join("test-template.yml"), template_config).unwrap();

    let result = BakeProject::load(temp_dir.path(), None, IndexMap::new(), false);
    assert!(result.is_ok());

    let project = result.unwrap();
    assert!(!project.template_registry.is_empty());
    assert!(project.template_registry.contains_key("test-template"));
}
