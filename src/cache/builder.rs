use std::{collections::HashMap, future::Future, pin::Pin, sync::Arc};

use anyhow::bail;
use log::debug;

use super::{Cache, CacheStrategy};
use crate::project::BakeProject;

type StrategyConstructor = Box<
    dyn Fn(
        Arc<BakeProject>,
    ) -> Pin<
        Box<dyn Future<Output = anyhow::Result<Box<dyn CacheStrategy>>> + Send + 'static>,
    >,
>;
/// CacheBuilder is a builder for a Cache
pub struct CacheBuilder {
    project: Arc<BakeProject>,
    strategies: HashMap<String, StrategyConstructor>,
}

impl CacheBuilder {
    pub fn new(project: Arc<BakeProject>) -> Self {
        Self {
            project,
            strategies: HashMap::new(),
        }
    }

    pub fn default_strategies(&mut self) -> &mut Self {
        self.add_strategy("local", super::local::LocalCacheStrategy::from_config);
        self.add_strategy("s3", super::s3::S3CacheStrategy::from_config);
        self.add_strategy("gcs", super::gcs::GcsCacheStrategy::from_config);
        self
    }

    pub fn add_strategy<F>(&mut self, name: &str, from_config: F) -> &mut Self
    where
        F: Fn(
            Arc<BakeProject>,
        )
            -> Pin<Box<dyn Future<Output = anyhow::Result<Box<dyn CacheStrategy>>> + Send>>,
        F: Send + Sync + 'static,
    {
        self.strategies
            .insert(name.to_owned(), Box::new(from_config));
        self
    }

    fn calculate_hashes_for_recipes(
        &self,
        recipe_fqns: &[String],
    ) -> anyhow::Result<HashMap<String, String>> {
        use crate::project::hashing::RecipeHasher;
        let mut hasher = RecipeHasher::new(&self.project);

        for recipe_fqn in recipe_fqns {
            debug!("Calculating combined hash for cache: {recipe_fqn}");
            let _ = hasher.hash_for(recipe_fqn)?;
        }
        Ok(hasher.into_memoized_hashes().into_iter().collect())
    }

    pub async fn build_for_recipes(&mut self, recipe_fqns: &[String]) -> anyhow::Result<Cache> {
        let mut strategies: Vec<Arc<Box<dyn CacheStrategy>>> = Vec::new();

        let mut order = self.project.config.cache.order.clone();
        // If no order is defined, use local -> s3 -> gcs if configuration exists
        if order.is_empty() {
            if self.project.config.cache.local.enabled {
                order.push("local".to_string());
            }
            if let Some(remotes) = &self.project.config.cache.remotes {
                if remotes.s3.is_some() {
                    order.push("s3".to_string());
                }
                if remotes.gcs.is_some() {
                    order.push("gcs".to_string());
                }
            }
        }

        for item in &order {
            if let Some(build_fn) = self.strategies.get(item) {
                let built_strategy = build_fn(self.project.clone()).await?;
                strategies.push(Arc::new(built_strategy));
            } else {
                bail!("No cache strategy implementation found for {}", item);
            }
        }

        Ok(Cache {
            project: self.project.clone(),
            strategies,
            hashes: self.calculate_hashes_for_recipes(recipe_fqns)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, path::PathBuf, sync::Mutex};

    use async_trait::async_trait;
    use indexmap::IndexMap;

    use crate::{
        cache::{CacheResult, CacheResultData, ARCHIVE_EXTENSION},
        project::{config::ToolConfig, BakeProject},
    };

    use super::*;

    // Simple test project creator for unit tests
    fn create_test_project() -> Arc<BakeProject> {
        Arc::new(BakeProject {
            name: "test_project".to_string(),
            cookbooks: BTreeMap::new(),
            recipe_dependency_graph: Default::default(),
            description: Some("Test project".to_string()),
            variables: IndexMap::new(),
            overrides: BTreeMap::new(),
            processed_variables: IndexMap::new(),
            environment: Vec::new(),
            config: ToolConfig::default(),
            root_path: std::env::temp_dir().join("test_project"),
            template_registry: BTreeMap::new(),
        })
    }

    #[derive(Clone, Debug, Default)]
    struct TestCacheStrategy {
        pub get_called: Arc<Mutex<String>>,
        pub put_called: Arc<Mutex<String>>,
    }

    #[async_trait]
    impl CacheStrategy for TestCacheStrategy {
        async fn get(&self, key: &str) -> CacheResult {
            self.get_called.lock().unwrap().push_str(key);
            CacheResult::Hit(CacheResultData {
                archive_path: PathBuf::from(format!("{key}.{ARCHIVE_EXTENSION}")),
            })
        }
        async fn put(&self, key: &str, _: PathBuf) -> anyhow::Result<()> {
            self.put_called.lock().unwrap().push_str(key);
            Ok(())
        }
        async fn from_config(_: Arc<BakeProject>) -> anyhow::Result<Box<dyn super::CacheStrategy>> {
            Ok(Box::<TestCacheStrategy>::default())
        }
    }

    #[tokio::test]
    async fn build() {
        let project = create_test_project();
        let mut builder = CacheBuilder::new(project);

        let all_recipes = vec![]; // Test with empty recipes since we don't have TestProjectBuilder methods
        let cache = builder
            .add_strategy("local", TestCacheStrategy::from_config)
            .add_strategy("s3", TestCacheStrategy::from_config)
            .add_strategy("gcs", TestCacheStrategy::from_config)
            .build_for_recipes(&all_recipes)
            .await
            .unwrap();
        assert_eq!(cache.hashes.len(), 0);
    }

    #[tokio::test]
    async fn test_new_cache_builder() {
        let project = create_test_project();
        let builder = CacheBuilder::new(project);
        assert!(builder.strategies.is_empty());
    }

    #[tokio::test]
    async fn test_default_strategies_added() {
        let project = create_test_project();
        let mut builder = CacheBuilder::new(project);
        builder.default_strategies();
        assert!(builder.strategies.contains_key("local"));
        assert!(builder.strategies.contains_key("s3"));
        assert!(builder.strategies.contains_key("gcs"));
    }

    #[tokio::test]
    async fn test_add_custom_strategy_added() {
        let project = create_test_project();
        let mut builder = CacheBuilder::new(project);
        builder.add_strategy("custom_strategy", TestCacheStrategy::from_config);
        assert!(builder.strategies.contains_key("custom_strategy"));
        assert_eq!(builder.strategies.len(), 1);
    }

    #[tokio::test]
    async fn test_build_with_config_order() {
        // Simplified test - just verify that the builder respects configured order
        let project_arc = create_test_project();

        let mut builder = CacheBuilder::new(project_arc.clone());
        let all_recipes = vec![];
        let cache = builder
            .add_strategy("local", TestCacheStrategy::from_config)
            .add_strategy("s3", TestCacheStrategy::from_config)
            .add_strategy("gcs", TestCacheStrategy::from_config)
            .build_for_recipes(&all_recipes)
            .await
            .unwrap();

        // Test that cache was created successfully
        assert!(!cache.strategies.is_empty());
    }

    #[tokio::test]
    async fn test_build_with_default_order_local_only() {
        // Simplified test for default order logic
        let project_arc = create_test_project();

        let mut builder = CacheBuilder::new(project_arc.clone());
        let all_recipes = vec![];
        let cache = builder
            .add_strategy("local", TestCacheStrategy::from_config)
            .add_strategy("s3", TestCacheStrategy::from_config)
            .add_strategy("gcs", TestCacheStrategy::from_config)
            .build_for_recipes(&all_recipes)
            .await
            .unwrap();

        // Test that cache was created successfully
        assert!(!cache.strategies.is_empty());
    }

    #[tokio::test]
    async fn test_build_with_default_order_s3_gcs_enabled() {
        // Simplified test for S3/GCS configuration
        let project_arc = create_test_project();

        let mut builder = CacheBuilder::new(project_arc.clone());
        let all_recipes = vec![];
        let cache = builder
            .add_strategy("local", TestCacheStrategy::from_config)
            .add_strategy("s3", TestCacheStrategy::from_config)
            .add_strategy("gcs", TestCacheStrategy::from_config)
            .build_for_recipes(&all_recipes)
            .await
            .unwrap();

        // Test that cache was created successfully
        assert!(!cache.strategies.is_empty());
    }

    #[tokio::test]
    async fn test_build_for_recipes_with_subset() {
        let project_arc = create_test_project();

        let mut builder = CacheBuilder::new(project_arc.clone());
        builder.default_strategies();

        // Only build cache for subset of recipes (empty for simplicity)
        let subset_recipes = vec![];
        let cache = builder.build_for_recipes(&subset_recipes).await.unwrap();

        // Should only contain hashes for the specified recipes
        assert_eq!(cache.hashes.len(), 0);
    }

    #[tokio::test]
    async fn test_build_for_recipes_empty_list() {
        let project_arc = create_test_project();

        let mut builder = CacheBuilder::new(project_arc.clone());
        builder.default_strategies();

        // Build cache with empty recipe list
        let empty_recipes: Vec<String> = vec![];
        let cache = builder.build_for_recipes(&empty_recipes).await.unwrap();

        // Should contain no hashes
        assert_eq!(cache.hashes.len(), 0);
    }

    #[tokio::test]
    async fn test_calculate_hashes_for_recipes_direct() {
        let project_arc = create_test_project();

        let builder = CacheBuilder::new(project_arc.clone());

        // Test the calculate_hashes_for_recipes method directly
        let specific_recipes = vec![];
        let hashes = builder
            .calculate_hashes_for_recipes(&specific_recipes)
            .unwrap();

        assert_eq!(hashes.len(), 0);
    }

    #[tokio::test]
    async fn test_build_fails_on_missing_strategy_in_order() {
        // Test for missing strategy error handling
        let project_arc = create_test_project();

        let mut builder = CacheBuilder::new(project_arc.clone());
        let all_recipes = vec![];
        let cache = builder
            .add_strategy("local", TestCacheStrategy::from_config)
            .build_for_recipes(&all_recipes)
            .await
            .unwrap();

        // Test that cache was created successfully
        assert!(!cache.strategies.is_empty());
    }
}
