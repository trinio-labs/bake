use std::collections::BTreeMap;

use indexmap::IndexMap;

use crate::project::config::ToolConfig;
use crate::project::RecipeCacheConfig;
use crate::project::{BakeProject, Cookbook, Recipe};
use rand::distr::{Alphanumeric, SampleString};

pub struct TestProjectBuilder {
    pub project: BakeProject,
}

impl TestProjectBuilder {
    pub fn new() -> Self {
        let temp_dir = std::env::temp_dir().join(Alphanumeric.sample_string(&mut rand::rng(), 16));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let project = BakeProject {
            name: "test".to_owned(),
            cookbooks: BTreeMap::new(),
            // recipes: BTreeMap::new(), // Removed: Handled by graph
            description: Some("".to_owned()),
            variables: IndexMap::new(),
            environment: vec![],
            config: ToolConfig::default(),

            root_path: temp_dir,
            // dependency_map: BTreeMap::new(), // Removed: Handled by graph
            // Initialize graph fields as default, they will be populated by the new graph module
            recipe_dependency_graph: Default::default(),
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
                        dependencies: None,
                        cache: Default::default(),
                        environment: vec![],
                        variables: IndexMap::new(),
                        run: format!("echo Hello from recipe {recipe}"),
                        run_status: Default::default(),
                        config_path: config_path.clone(),
                    },
                )
            })
            .collect();

        // self.project.recipes.extend(...); // Removed: Not using the flat recipes map

        // self.project.dependency_map.extend(...); // Removed: Not using dependency_map

        let cookbook = Cookbook {
            name: name.to_owned(),
            environment: vec![],
            variables: IndexMap::new(),
            recipes,
            config_path: config_path.clone(),
        };

        self.project
            .cookbooks
            .insert(cookbook.name.clone(), cookbook);

        self
    }

    pub fn with_dependency(mut self, recipe_fqn: &str, dependency_fqn: &str) -> Self {
        // self.project.dependency_map... // Removed

        // Modify the Recipe struct within the appropriate Cookbook
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
        // Populate graph fields after all cookbooks and dependencies are set up
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
