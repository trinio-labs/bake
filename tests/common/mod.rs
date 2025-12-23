use std::{collections::BTreeMap, sync::Arc};

use indexmap::IndexMap;
use tempfile::TempDir;

use bake::project::{
    BakeProject, Cookbook, Recipe, RecipeCacheConfig,
    config::{CacheConfig, LocalCacheConfig, ToolConfig},
    graph::RecipeDependencyGraph,
};
use rand::distr::{Alphanumeric, SampleString};

/// Helper function to create a BakeProject with a specific ToolConfig for testing
#[allow(dead_code)]
pub fn create_test_project_with_config(tool_config: ToolConfig) -> Arc<BakeProject> {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let project_root_path = temp_dir.path().to_path_buf();

    // Leak the temp_dir to prevent it from being dropped during tests
    Box::leak(Box::new(temp_dir));

    Arc::new(BakeProject {
        name: "test_project".to_string(),
        cookbooks: BTreeMap::new(),
        recipe_dependency_graph: RecipeDependencyGraph::default(),
        description: Some("A test project".to_string()),
        variables: IndexMap::new(),
        overrides: BTreeMap::new(),
        processed_variables: IndexMap::new(),
        environment: Vec::new(),
        config: tool_config,
        root_path: project_root_path,
        template_registry: BTreeMap::new(),
        helper_registry: BTreeMap::new(),
    })
}

/// Helper function to create a BakeProject with default configuration for testing
#[allow(dead_code)]
pub fn create_default_test_project() -> Arc<BakeProject> {
    let tool_config = ToolConfig {
        cache: CacheConfig {
            local: LocalCacheConfig {
                enabled: true,
                path: None,
                compression_level: 1,
            },
            remotes: None,
            order: vec!["local".to_string()],
        },
        ..ToolConfig::default()
    };
    create_test_project_with_config(tool_config)
}

/// Test project builder for creating projects with cookbooks and recipes
#[allow(dead_code)]
pub struct TestProjectBuilder {
    pub project: BakeProject,
}

#[allow(dead_code)]
impl TestProjectBuilder {
    pub fn new() -> Self {
        let temp_dir = std::env::temp_dir().join(Alphanumeric.sample_string(&mut rand::rng(), 16));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let project = BakeProject {
            name: "test".to_owned(),
            cookbooks: BTreeMap::new(),
            description: Some("".to_owned()),
            variables: IndexMap::new(),
            overrides: BTreeMap::new(),
            processed_variables: IndexMap::new(),
            environment: vec![],
            config: ToolConfig::default(),
            root_path: temp_dir,
            recipe_dependency_graph: Default::default(),
            template_registry: BTreeMap::new(),
            helper_registry: BTreeMap::new(),
        };
        Self { project }
    }

    pub fn with_cookbook(mut self, name: &str, recipes: &[&str]) -> Self {
        let config_path = self.project.root_path.join(format!("{name}.yml"));
        let recipes: BTreeMap<String, Recipe> = recipes
            .iter()
            .map(|recipe| {
                (
                    String::from(*recipe),
                    Recipe {
                        name: String::from(*recipe),
                        project_root: self.project.root_path.clone(),
                        cookbook: name.to_owned(),
                        description: None,
                        tags: vec![],
                        dependencies: None,
                        cache: Default::default(),
                        environment: vec![],
                        variables: IndexMap::new(),
                        overrides: BTreeMap::new(),
                        processed_variables: IndexMap::new(),
                        run: format!("echo Hello from recipe {recipe}"),
                        run_status: Default::default(),
                        config_path: config_path.clone(),
                        template: None,
                        parameters: std::collections::BTreeMap::new(),
                    },
                )
            })
            .collect();

        let cookbook = Cookbook {
            name: name.to_owned(),
            environment: vec![],
            tags: vec![],
            variables: IndexMap::new(),
            overrides: BTreeMap::new(),
            processed_variables: IndexMap::new(),
            recipes,
            config_path: config_path.clone(),
            fully_loaded: true,
        };

        self.project
            .cookbooks
            .insert(cookbook.name.clone(), cookbook);

        self
    }

    pub fn with_dependency(mut self, recipe_fqn: &str, dependency_fqn: &str) -> Self {
        if let Some((cookbook_name, recipe_name)) = recipe_fqn.split_once(':') {
            if let Some(cookbook) = self.project.cookbooks.get_mut(cookbook_name) {
                if let Some(recipe) = cookbook.recipes.get_mut(recipe_name) {
                    if recipe.dependencies.is_none() {
                        recipe.dependencies = Some(Vec::new());
                    }
                    recipe
                        .dependencies
                        .as_mut()
                        .unwrap()
                        .push(dependency_fqn.to_string());
                } else {
                    panic!(
                        "Recipe '{recipe_name}' not found in cookbook '{cookbook_name}' for adding dependency"
                    );
                }
            } else {
                panic!(
                    "Cookbook '{cookbook_name}' not found for adding dependency to recipe '{recipe_fqn}'"
                );
            }
        } else {
            panic!("Invalid recipe FQN '{recipe_fqn}' for adding dependency");
        }
        self
    }

    pub fn build(mut self) -> BakeProject {
        self.project
            .recipe_dependency_graph
            .populate_from_cookbooks(&self.project.cookbooks)
            .expect("Test project graph setup failed during build");
        self.project
    }

    pub fn with_recipe_cache_outputs(mut self, recipe_fqn: &str, outputs: Vec<String>) -> Self {
        let (cookbook_name, recipe_name) = recipe_fqn.split_once(':').unwrap_or_else(|| {
            panic!("Invalid recipe FQN '{recipe_fqn}' for setting cache outputs")
        });
        let cookbook = self
            .project
            .cookbooks
            .get_mut(cookbook_name)
            .unwrap_or_else(|| {
                panic!("Cookbook '{cookbook_name}' not found in TestProjectBuilder")
            });
        let recipe = cookbook.recipes.get_mut(recipe_name).unwrap_or_else(|| {
            panic!(
                "Recipe '{recipe_name}' not found in cookbook '{cookbook_name}' in TestProjectBuilder"
            )
        });
        recipe.cache = Some(RecipeCacheConfig {
            outputs,
            ..Default::default()
        });
        self
    }
}

impl Default for TestProjectBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper to create a dummy file for testing
#[allow(dead_code)]
pub async fn create_dummy_file(path: &std::path::PathBuf) -> anyhow::Result<()> {
    use tokio::fs::File;
    use tokio::io::AsyncWriteExt;

    let mut file = File::create(path).await?;
    file.write_all(b"test data").await?;
    Ok(())
}
