use std::collections::{BTreeMap, HashSet};

use indexmap::IndexMap;

use crate::project::config::ToolConfig;
use crate::project::{BakeProject, Cookbook, Recipe};
use rand::distributions::{Alphanumeric, DistString};

pub struct TestProjectBuilder {
    pub project: BakeProject,
}

impl TestProjectBuilder {
    pub fn new() -> Self {
        let temp_dir =
            std::env::temp_dir().join(Alphanumeric.sample_string(&mut rand::thread_rng(), 16));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let project = BakeProject {
            name: "test".to_owned(),
            cookbooks: BTreeMap::new(),
            recipes: BTreeMap::new(),
            description: Some("".to_owned()),
            variables: IndexMap::new(),
            environment: vec![],
            config: ToolConfig::default(),
            root_path: temp_dir,
            dependency_map: BTreeMap::new(),
        };
        Self { project }
    }

    pub fn with_cookbook(mut self, name: &str, recipes: &[&str]) -> Self {
        let config_path = self.project.root_path.join(format!("{}.yml", name));
        let recipes: BTreeMap<String, Recipe> = recipes
            .iter()
            .map(|recipe| {
                (
                    String::from(*recipe),
                    Recipe {
                        name: String::from(*recipe),
                        cookbook: name.to_owned(),
                        description: None,
                        dependencies: None,
                        cache: Default::default(),
                        environment: vec![],
                        variables: IndexMap::new(),
                        run: format!("echo Hello from recipe {}", recipe),
                        run_status: Default::default(),
                        config_path: config_path.clone(),
                    },
                )
            })
            .collect();

        self.project.recipes.extend(
            recipes
                .values()
                .map(|recipe| (format!("{}:{}", name, recipe.name), recipe.clone())),
        );

        self.project.dependency_map.extend(
            recipes
                .keys()
                .map(|key| (format!("{}:{}", name, key), HashSet::new())),
        );

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

    pub fn with_dependency(mut self, recipe: &str, dependency: &str) -> Self {
        self.project
            .dependency_map
            .entry(recipe.to_owned())
            .or_default()
            .insert(dependency.to_owned());

        self.project
            .recipes
            .get_mut(recipe)
            .unwrap()
            .dependencies
            .as_mut()
            .unwrap_or(Vec::new().as_mut())
            .push(dependency.to_owned());
        self
    }

    pub fn build(self) -> BakeProject {
        self.project
    }
}
