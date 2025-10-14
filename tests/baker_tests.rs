use std::{path::PathBuf, sync::Arc};

use async_trait::async_trait;
use test_case::test_case;

use bake::{
    baker,
    cache::{Cache, CacheBuilder, CacheResult, CacheResultData, CacheStrategy, ARCHIVE_EXTENSION},
    project::BakeProject,
};

mod common;

#[derive(Clone, Debug)]
struct TestCacheStrategy {
    pub hit: bool,
}

#[async_trait]
impl CacheStrategy for TestCacheStrategy {
    async fn get(&self, _: &str) -> CacheResult {
        if self.hit {
            CacheResult::Hit(CacheResultData {
                archive_path: PathBuf::from(format!("foo.{ARCHIVE_EXTENSION}")),
            })
        } else {
            CacheResult::Miss
        }
    }
    async fn put(&self, _: &str, _: PathBuf) -> anyhow::Result<()> {
        Ok(())
    }

    async fn from_config(_project: Arc<BakeProject>) -> anyhow::Result<Box<dyn CacheStrategy>> {
        Ok(Box::new(TestCacheStrategy { hit: false }))
    }
}

async fn build_cache(project: Arc<BakeProject>) -> Cache {
    let all_recipes: Vec<String> = project
        .cookbooks
        .values()
        .flat_map(|cb| {
            cb.recipes
                .keys()
                .map(|r_name| format!("{}:{}", cb.name, r_name))
        })
        .collect();

    CacheBuilder::new(project)
        .add_strategy("local", TestCacheStrategy::from_config)
        .add_strategy("s3", TestCacheStrategy::from_config)
        .add_strategy("gcs", TestCacheStrategy::from_config)
        .build_for_recipes(&all_recipes)
        .await
        .unwrap()
}

fn create_test_project() -> BakeProject {
    let mut project = common::TestProjectBuilder::new()
        .with_cookbook("foo", &["build", "test"])
        .with_cookbook("bar", &["build", "test"])
        .build();

    project
        .cookbooks
        .get_mut("foo")
        .unwrap()
        .recipes
        .get_mut("test")
        .unwrap()
        .run = String::from("echo 'Running foo:test'");

    project
        .cookbooks
        .get_mut("bar")
        .unwrap()
        .recipes
        .get_mut("test")
        .unwrap()
        .run = String::from("echo 'Running bar:test'");

    project
}

// Helper to get execution plan with proper context
fn get_execution_plan(
    project: &mut BakeProject,
    pattern: Option<&str>,
    use_regex: bool,
    tags: &[String],
) -> anyhow::Result<Vec<Vec<bake::project::Recipe>>> {
    use indexmap::IndexMap;
    let context = project.build_variable_context(&IndexMap::new());
    project.get_recipes_for_execution(pattern, use_regex, tags, None, &context)
}

#[tokio::test]
async fn test_run_all_recipes() {
    let mut project = create_test_project();
    let execution_plan = get_execution_plan(&mut project, None, false, &[]).unwrap();
    let project = Arc::new(project);
    let cache = build_cache(project.clone()).await;
    let result = baker::bake(project.clone(), cache, execution_plan, false).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_run_bar_recipes_only() {
    let mut project = create_test_project();
    project.config.verbose = true;
    let execution_plan = get_execution_plan(&mut project, Some("bar:"), false, &[]).unwrap();
    let project = Arc::new(project);
    let cache = build_cache(project.clone()).await;
    let result = baker::bake(project.clone(), cache, execution_plan, false).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_recipe_failure_handling() {
    let mut project = create_test_project();

    // Modify bar:test to fail
    project
        .cookbooks
        .get_mut("bar")
        .unwrap()
        .recipes
        .get_mut("test")
        .unwrap()
        .run = String::from("false; echo 'bar:test failed intentionally!'");

    // Modify bar:build to depend on bar:test
    project
        .cookbooks
        .get_mut("bar")
        .unwrap()
        .recipes
        .get_mut("build")
        .unwrap()
        .dependencies = Some(vec!["bar:test".to_string()]);

    // Repopulate the graph after modifying dependencies
    project
        .recipe_dependency_graph
        .populate_from_cookbooks(&project.cookbooks)
        .expect("Failed to repopulate dependency graph");

    let execution_plan = get_execution_plan(&mut project, None, false, &[]).unwrap();
    let project = Arc::new(project);
    let cache = build_cache(project.clone()).await;
    let result = baker::bake(project.clone(), cache, execution_plan, false).await;

    // Should fail due to bar:test failing
    assert!(result.is_err());
}

#[test_case("foo:build"; "single recipe")]
#[test_case("foo:"; "cookbook recipes")]
#[test_case(":build"; "recipe pattern")]
#[tokio::test]
async fn test_recipe_filtering(filter: &str) {
    let mut project = create_test_project();
    let execution_plan = get_execution_plan(&mut project, Some(filter), false, &[]).unwrap();
    let project = Arc::new(project);
    let cache = build_cache(project.clone()).await;
    let result = baker::bake(project.clone(), cache, execution_plan, false).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_verbose_mode() {
    let mut project = create_test_project();
    project.config.verbose = true;

    let execution_plan = get_execution_plan(&mut project, None, false, &[]).unwrap();
    let project = Arc::new(project);
    let cache = build_cache(project.clone()).await;
    let result = baker::bake(project.clone(), cache, execution_plan, false).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_with_dependencies() {
    let mut project = common::TestProjectBuilder::new()
        .with_cookbook("app", &["install", "build", "test"])
        .with_dependency("app:build", "app:install")
        .with_dependency("app:test", "app:build")
        .build();

    let execution_plan = get_execution_plan(&mut project, Some("app:test"), false, &[]).unwrap();
    let project = Arc::new(project);
    let cache = build_cache(project.clone()).await;
    let result = baker::bake(project.clone(), cache, execution_plan, false).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_tag_filtering_single_tag() {
    use indexmap::IndexMap;
    let project_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("resources")
        .join("tests")
        .join("valid");

    let mut project =
        BakeProject::from(&project_root, Some("default"), IndexMap::new(), false).unwrap();

    // Filter by "frontend" tag - should match tagged:deploy and tagged:frontend-build
    let execution_plan =
        get_execution_plan(&mut project, None, false, &["frontend".to_string()]).unwrap();

    let all_recipes: Vec<String> = execution_plan
        .iter()
        .flatten()
        .map(|r| r.full_name())
        .collect();

    // Should include frontend-tagged recipes and their dependencies
    assert!(all_recipes.contains(&"tagged:deploy".to_string()));
    assert!(all_recipes.contains(&"tagged:frontend-build".to_string()));
    // Should include dependency even if not tagged
    assert!(all_recipes.contains(&"tagged:build".to_string()));
}

#[tokio::test]
async fn test_tag_filtering_multiple_tags_or_logic() {
    use indexmap::IndexMap;
    let project_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("resources")
        .join("tests")
        .join("valid");

    let mut project =
        BakeProject::from(&project_root, Some("default"), IndexMap::new(), false).unwrap();

    // Filter by "backend" OR "deploy" tags
    let execution_plan = get_execution_plan(
        &mut project,
        None,
        false,
        &["backend".to_string(), "deploy".to_string()],
    )
    .unwrap();

    let all_recipes: Vec<String> = execution_plan
        .iter()
        .flatten()
        .map(|r| r.full_name())
        .collect();

    // Should include recipes with backend tag (inherited from cookbook)
    assert!(all_recipes.contains(&"tagged:build".to_string()));
    assert!(all_recipes.contains(&"tagged:test".to_string()));
    // Should include recipes with deploy tag
    assert!(all_recipes.contains(&"tagged:deploy".to_string()));
}

#[tokio::test]
async fn test_tag_filtering_with_pattern() {
    use indexmap::IndexMap;
    let project_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("resources")
        .join("tests")
        .join("valid");

    let mut project =
        BakeProject::from(&project_root, Some("default"), IndexMap::new(), false).unwrap();

    // Filter by pattern AND tags
    let execution_plan = get_execution_plan(
        &mut project,
        Some("tagged:"),
        false,
        &["frontend".to_string()],
    )
    .unwrap();

    let all_recipes: Vec<String> = execution_plan
        .iter()
        .flatten()
        .map(|r| r.full_name())
        .collect();

    // Should only include recipes from "tagged" cookbook with "frontend" tag
    assert!(all_recipes.contains(&"tagged:deploy".to_string()));
    assert!(all_recipes.contains(&"tagged:frontend-build".to_string()));
    // Should NOT include recipes from other cookbooks
    assert!(!all_recipes.iter().any(|r| r.starts_with("foo:")));
}

#[tokio::test]
async fn test_tag_filtering_case_insensitive() {
    use indexmap::IndexMap;
    let project_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("resources")
        .join("tests")
        .join("valid");

    let mut project =
        BakeProject::from(&project_root, Some("default"), IndexMap::new(), false).unwrap();

    // Test case-insensitive matching with "FRONTEND" (uppercase)
    let execution_plan =
        get_execution_plan(&mut project, None, false, &["FRONTEND".to_string()]).unwrap();

    let all_recipes: Vec<String> = execution_plan
        .iter()
        .flatten()
        .map(|r| r.full_name())
        .collect();

    // Should match despite case difference
    assert!(all_recipes.contains(&"tagged:deploy".to_string()));
    assert!(all_recipes.contains(&"tagged:frontend-build".to_string()));
}

#[tokio::test]
async fn test_tag_filtering_empty_tags_no_filtering() {
    use indexmap::IndexMap;
    let project_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("resources")
        .join("tests")
        .join("valid");

    let mut project =
        BakeProject::from(&project_root, Some("default"), IndexMap::new(), false).unwrap();

    // Empty tags should return all recipes
    let execution_plan_all = get_execution_plan(&mut project, None, false, &[]).unwrap();

    let execution_plan_no_tags = get_execution_plan(&mut project, None, false, &[]).unwrap();

    // Both should be equal (no filtering)
    assert_eq!(execution_plan_all.len(), execution_plan_no_tags.len());
}

#[tokio::test]
async fn test_tag_filtering_no_matches() {
    use indexmap::IndexMap;
    let project_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("resources")
        .join("tests")
        .join("valid");

    let mut project =
        BakeProject::from(&project_root, Some("default"), IndexMap::new(), false).unwrap();

    // Filter by non-existent tag
    let execution_plan =
        get_execution_plan(&mut project, None, false, &["nonexistent".to_string()]).unwrap();

    // Should be empty
    assert!(execution_plan.is_empty());
}
