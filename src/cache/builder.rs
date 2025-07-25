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
    use std::{path::PathBuf, sync::Mutex};

    use async_trait::async_trait;

    use crate::{
        cache::{CacheResult, CacheResultData, ARCHIVE_EXTENSION},
        project::{
            config::{
                GcsCacheConfig,
                // CacheConfig is implicitly part of BakeProject.config.cache
                // LocalCacheConfig, // Commenting out to test if truly unused
                RemoteCacheConfig,
                S3CacheConfig,
            },
            BakeProject,
        },
        test_utils::TestProjectBuilder,
    };

    use super::*;

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
        let project = Arc::new(
            TestProjectBuilder::new()
                .with_cookbook("foo", &["build"])
                .build(),
        );
        let mut builder = CacheBuilder::new(project);

        let all_recipes = vec!["foo:build".to_string()];
        let cache = builder
            .add_strategy("local", TestCacheStrategy::from_config)
            .add_strategy("s3", TestCacheStrategy::from_config)
            .add_strategy("gcs", TestCacheStrategy::from_config)
            .build_for_recipes(&all_recipes)
            .await
            .unwrap();
        assert!(cache.hashes.contains_key("foo:build"));
    }

    #[tokio::test]
    async fn test_new_cache_builder() {
        let project = Arc::new(TestProjectBuilder::new().build());
        let builder = CacheBuilder::new(project);
        assert!(builder.strategies.is_empty());
    }

    #[tokio::test]
    async fn test_default_strategies_added() {
        let project = Arc::new(TestProjectBuilder::new().build());
        let mut builder = CacheBuilder::new(project);
        builder.default_strategies();
        assert!(builder.strategies.contains_key("local"));
        assert!(builder.strategies.contains_key("s3"));
        assert!(builder.strategies.contains_key("gcs"));
    }

    #[tokio::test]
    async fn test_add_custom_strategy_added() {
        let project = Arc::new(TestProjectBuilder::new().build());
        let mut builder = CacheBuilder::new(project);
        builder.add_strategy("custom_strategy", TestCacheStrategy::from_config);
        assert!(builder.strategies.contains_key("custom_strategy"));
        assert_eq!(builder.strategies.len(), 1);
    }

    #[tokio::test]
    async fn test_build_with_config_order() {
        let mut project = TestProjectBuilder::new()
            .with_cookbook("foo", &["build"])
            .build();

        // Configure cache order
        project.config.cache.order = vec!["s3".to_string(), "local".to_string()];
        // Ensure the strategies in the order are considered enabled for the builder's logic
        // if it relies on individual .enabled flags (though order should override)
        project.config.cache.local.enabled = true;
        project.config.cache.remotes = Some(RemoteCacheConfig {
            s3: Some(S3CacheConfig {
                bucket: "test-bucket".to_string(),
                region: None,
            }),
            gcs: None,
        });

        let project_arc = Arc::new(project);

        let mut builder = CacheBuilder::new(project_arc.clone());
        let all_recipes = vec!["foo:build".to_string()];
        let cache = builder
            .add_strategy("local", TestCacheStrategy::from_config)
            .add_strategy("s3", TestCacheStrategy::from_config)
            .add_strategy("gcs", TestCacheStrategy::from_config) // gcs is registered but not in order
            .build_for_recipes(&all_recipes)
            .await
            .unwrap();

        assert_eq!(
            cache.strategies.len(),
            2,
            "Cache should only contain strategies specified in the order"
        );
        // TODO: Add a way to identify the strategies to assert their exact order e.g. s3 then local
        // For now, we assume if the count is correct, the builder respected the order.
        // A more robust check would be:
        // assert_eq!(cache.strategies[0].name(), "s3");
        // assert_eq!(cache.strategies[1].name(), "local");
    }

    #[tokio::test]
    async fn test_build_with_default_order_local_only() {
        let mut project = TestProjectBuilder::new()
            .with_cookbook("foo", &["build"])
            .build();

        // Ensure no explicit order and only local cache is enabled
        project.config.cache.order = vec![]; // Empty order
        project.config.cache.local.enabled = true;
        project.config.cache.remotes = None; // No S3 or GCS configured

        let project_arc = Arc::new(project);

        let mut builder = CacheBuilder::new(project_arc.clone());
        let all_recipes = vec!["foo:build".to_string()];
        let cache = builder
            .add_strategy("local", TestCacheStrategy::from_config)
            .add_strategy("s3", TestCacheStrategy::from_config) // s3 is registered
            .add_strategy("gcs", TestCacheStrategy::from_config) // gcs is registered
            .build_for_recipes(&all_recipes)
            .await
            .unwrap();

        assert_eq!(cache.strategies.len(), 1, "Cache should only contain the local strategy when it's the only one enabled and no order is set");
        // TODO: Add a way to identify the strategy to assert it is indeed 'local'.
    }

    #[tokio::test]
    async fn test_build_with_default_order_s3_gcs_enabled() {
        let mut project = TestProjectBuilder::new()
            .with_cookbook("foo", &["build"])
            .build();

        // Ensure no explicit order, local is disabled, and S3/GCS are enabled
        project.config.cache.order = vec![]; // Empty order
        project.config.cache.local.enabled = false; // Local cache disabled
        project.config.cache.remotes = Some(RemoteCacheConfig {
            s3: Some(S3CacheConfig {
                bucket: "test-s3-bucket".to_string(),
                region: None,
            }),
            gcs: Some(GcsCacheConfig {
                bucket: "test-gcs-bucket".to_string(),
            }),
        });

        let project_arc = Arc::new(project);

        let mut builder = CacheBuilder::new(project_arc.clone());
        let all_recipes = vec!["foo:build".to_string()];
        let cache = builder
            .add_strategy("local", TestCacheStrategy::from_config) // local is registered but disabled
            .add_strategy("s3", TestCacheStrategy::from_config)
            .add_strategy("gcs", TestCacheStrategy::from_config)
            .build_for_recipes(&all_recipes)
            .await
            .unwrap();

        assert_eq!(
            cache.strategies.len(),
            2,
            "Cache should contain s3 and gcs strategies based on default order when enabled"
        );
        // TODO: Add a way to identify the strategies to assert their exact order (e.g., s3 then gcs).
    }

    #[tokio::test]
    async fn test_build_for_recipes_with_subset() {
        let project_arc = Arc::new(
            TestProjectBuilder::new()
                .with_cookbook("foo", &["build", "test", "deploy"])
                .with_cookbook("bar", &["build", "test"])
                .build(),
        );

        let mut builder = CacheBuilder::new(project_arc.clone());
        builder.default_strategies();

        // Only build cache for subset of recipes
        let subset_recipes = vec!["foo:build".to_string(), "bar:test".to_string()];
        let cache = builder.build_for_recipes(&subset_recipes).await.unwrap();

        // Should only contain hashes for the specified recipes
        assert_eq!(cache.hashes.len(), 2);
        assert!(cache.hashes.contains_key("foo:build"));
        assert!(cache.hashes.contains_key("bar:test"));
        assert!(!cache.hashes.contains_key("foo:test"));
        assert!(!cache.hashes.contains_key("foo:deploy"));
        assert!(!cache.hashes.contains_key("bar:build"));
    }

    #[tokio::test]
    async fn test_build_for_recipes_empty_list() {
        let project_arc = Arc::new(
            TestProjectBuilder::new()
                .with_cookbook("foo", &["build"])
                .build(),
        );

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
        let project_arc = Arc::new(
            TestProjectBuilder::new()
                .with_cookbook("foo", &["build", "test"])
                .build(),
        );

        let builder = CacheBuilder::new(project_arc.clone());

        // Test the calculate_hashes_for_recipes method directly
        let specific_recipes = vec!["foo:build".to_string()];
        let hashes = builder
            .calculate_hashes_for_recipes(&specific_recipes)
            .unwrap();

        assert_eq!(hashes.len(), 1);
        assert!(hashes.contains_key("foo:build"));
        assert!(!hashes.contains_key("foo:test"));
    }

    #[tokio::test]
    async fn test_build_fails_on_missing_strategy_in_order() {
        let mut project = TestProjectBuilder::new()
            .with_cookbook("foo", &["build"])
            .build();

        // Configure an order with a strategy that won't be registered
        project.config.cache.order = vec!["custom_strategy".to_string(), "local".to_string()];

        let project_arc = Arc::new(project);

        let mut builder = CacheBuilder::new(project_arc.clone());
        let all_recipes = vec!["foo:build".to_string()];
        let result = builder
            .add_strategy("local", TestCacheStrategy::from_config) // Only register "local"
            // "custom_strategy" is in the order but not registered
            .build_for_recipes(&all_recipes)
            .await;

        assert!(
            result.is_err(),
            "Build should fail when a strategy in the order is not registered."
        );

        if let Err(e) = result {
            let error_message = e.to_string();
            assert!(
                error_message
                    .contains("No cache strategy implementation found for custom_strategy"),
                "Error message should indicate that 'custom_strategy' was not found. Got: {error_message}"
            );
        }
    }
}
