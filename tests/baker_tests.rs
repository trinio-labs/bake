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

#[tokio::test]
async fn test_run_all_recipes() {
    let project = Arc::new(create_test_project());
    let cache = build_cache(project.clone()).await;
    let execution_plan = project.get_recipes_for_execution(None, false).unwrap();
    let result = baker::bake(project.clone(), cache, execution_plan, false).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_run_bar_recipes_only() {
    let mut project = create_test_project();
    project.config.verbose = true;
    let project = Arc::new(project);
    let cache = build_cache(project.clone()).await;
    let execution_plan = project
        .get_recipes_for_execution(Some("bar:"), false)
        .unwrap();
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

    let project = Arc::new(project);
    let cache = build_cache(project.clone()).await;
    let execution_plan = project.get_recipes_for_execution(None, false).unwrap();
    let result = baker::bake(project.clone(), cache, execution_plan, false).await;

    // Should fail due to bar:test failing
    assert!(result.is_err());
}

#[test_case("foo:build"; "single recipe")]
#[test_case("foo:"; "cookbook recipes")]
#[test_case(":build"; "recipe pattern")]
#[tokio::test]
async fn test_recipe_filtering(filter: &str) {
    let project = Arc::new(create_test_project());
    let cache = build_cache(project.clone()).await;
    let execution_plan = project
        .get_recipes_for_execution(Some(filter), false)
        .unwrap();
    let result = baker::bake(project.clone(), cache, execution_plan, false).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_verbose_mode() {
    let mut project = create_test_project();
    project.config.verbose = true;

    let project = Arc::new(project);
    let cache = build_cache(project.clone()).await;
    let execution_plan = project.get_recipes_for_execution(None, false).unwrap();
    let result = baker::bake(project.clone(), cache, execution_plan, false).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_with_dependencies() {
    let project = common::TestProjectBuilder::new()
        .with_cookbook("app", &["install", "build", "test"])
        .with_dependency("app:build", "app:install")
        .with_dependency("app:test", "app:build")
        .build();

    let project = Arc::new(project);
    let cache = build_cache(project.clone()).await;
    let execution_plan = project
        .get_recipes_for_execution(Some("app:test"), false)
        .unwrap();
    let result = baker::bake(project.clone(), cache, execution_plan, false).await;
    assert!(result.is_ok());
}
