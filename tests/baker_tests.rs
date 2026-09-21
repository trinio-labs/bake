use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use bytes::Bytes;
use test_case::test_case;

use bake::{
    baker,
    cache::{
        Cache,
        cas::{BlobHash, BlobStore, LocalBlobStore},
    },
};

mod common;

async fn build_cache(project: &Arc<bake::project::BakeProject>) -> Cache {
    let cache_root = project.get_project_bake_path().join("cache");
    Cache::local(
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

/// Blob store whose reads stall, standing in for a slow remote cache.
struct SlowBlobStore {
    inner: LocalBlobStore,
    delay: Duration,
}

#[async_trait]
impl BlobStore for SlowBlobStore {
    async fn contains(&self, hash: &BlobHash) -> anyhow::Result<bool> {
        self.inner.contains(hash).await
    }

    async fn get(&self, hash: &BlobHash) -> anyhow::Result<Bytes> {
        tokio::time::sleep(self.delay).await;
        self.inner.get(hash).await
    }

    async fn put(&self, content: Bytes) -> anyhow::Result<BlobHash> {
        self.inner.put(content).await
    }

    async fn delete(&self, hash: &BlobHash) -> anyhow::Result<()> {
        self.inner.delete(hash).await
    }

    async fn size(&self, hash: &BlobHash) -> anyhow::Result<Option<u64>> {
        self.inner.size(hash).await
    }

    async fn list(&self) -> anyhow::Result<Vec<BlobHash>> {
        self.inner.list().await
    }
}

async fn build_slow_cache(project: &Arc<bake::project::BakeProject>, delay: Duration) -> Cache {
    let cache_root = project.get_project_bake_path().join("cache");
    let inner = LocalBlobStore::new(cache_root.join("cas/blobs"));
    inner.init().await.unwrap();
    Cache::with_blob_store(
        cache_root,
        project.root_path.clone(),
        bake::cache::CacheConfig::default(),
        Arc::new(SlowBlobStore { inner, delay }),
    )
    .await
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_cache_lookups_do_not_hold_execution_permits() {
    const RESTORE_DELAY: Duration = Duration::from_secs(1);
    const CACHED_RECIPES: usize = 4;

    let cached_names: Vec<String> = (0..CACHED_RECIPES).map(|i| format!("cached{i}")).collect();
    let mut recipe_names: Vec<&str> = cached_names.iter().map(String::as_str).collect();
    recipe_names.push("uncached");

    let mut builder = common::TestProjectBuilder::new().with_cookbook("foo", &recipe_names);
    for name in &cached_names {
        builder = builder.with_recipe_cache_outputs(&format!("foo:{name}"), vec![]);
    }
    let mut project = builder.build();
    // A single execution slot: if lookups held it, the cached recipes would restore one at a time.
    project.config.max_parallel = 1;
    project.config.reserved_threads = 0;

    let plan = get_execution_plan(&mut project, None, false, &[]).unwrap();
    let project = Arc::new(project);

    // The first bake misses and populates the cache; nothing is read from the blob store.
    let cache = build_slow_cache(&project, RESTORE_DELAY).await;
    baker::bake(project.clone(), cache, plan.clone(), false)
        .await
        .unwrap();

    let cache = build_slow_cache(&project, RESTORE_DELAY).await;
    let started = Instant::now();
    baker::bake(project, cache, plan, false).await.unwrap();
    let elapsed = started.elapsed();

    // Every hit reads two blobs (stdout and stderr), so one restore costs twice the delay and
    // four of them serialized behind the permit would cost eight times the delay.
    let serialized = RESTORE_DELAY * 2 * CACHED_RECIPES as u32;
    assert!(
        elapsed < serialized / 2,
        "cache lookups appear to be serialized behind execution permits: {elapsed:?} (serialized would be {serialized:?})"
    );
}
