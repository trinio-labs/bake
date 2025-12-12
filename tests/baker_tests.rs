use std::sync::Arc;
use test_case::test_case;

use bake::{baker, cache::Cache};

mod common;

async fn build_cache(project: &Arc<bake::project::BakeProject>) -> Cache {
    let cache_root = project.get_project_bake_path().join("cache");
    Cache::new(
        cache_root,
        project.root_path.clone(),
        bake::cache::CacheConfig::default(),
    )
    .await
    .unwrap()
}

fn create_test_project() -> bake::project::BakeProject {
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
    project: &mut bake::project::BakeProject,
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
    let cache = build_cache(&project).await;
    let result = baker::bake(project.clone(), cache, execution_plan, false).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_run_bar_recipes_only() {
    let mut project = create_test_project();
    project.config.verbose = true;
    let execution_plan = get_execution_plan(&mut project, Some("bar:"), false, &[]).unwrap();
    let project = Arc::new(project);
    let cache = build_cache(&project).await;
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
    let cache = build_cache(&project).await;
    let result = baker::bake(project.clone(), cache, execution_plan, false).await;
    assert!(result.is_err());
}

#[test_case("foo:", &["foo:build", "foo:test"]; "filter_by_cookbook")]
#[test_case(":build", &["bar:build", "foo:build"]; "filter_by_recipe_name")]
fn test_recipe_filtering(pattern: &str, expected: &[&str]) {
    let mut project = create_test_project();
    let execution_plan = get_execution_plan(&mut project, Some(pattern), false, &[]).unwrap();

    // Flatten execution plan to get all recipe FQNs
    let recipe_fqns: Vec<String> = execution_plan
        .into_iter()
        .flatten()
        .map(|r| r.full_name())
        .collect();

    // Check that all expected recipes are present
    for expected_fqn in expected {
        assert!(
            recipe_fqns.contains(&expected_fqn.to_string()),
            "Expected recipe {} to be in execution plan",
            expected_fqn
        );
    }
}
