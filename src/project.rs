pub mod config;
pub mod cookbook;
pub mod recipe;

use anyhow::bail;

pub use cookbook::*;
use indexmap::IndexMap;
pub use recipe::*;

pub use validator::Validate;

use std::{
    collections::{BTreeMap, HashSet},
    path::{Path, PathBuf},
};

use serde::Deserialize;

use crate::template::parse_variable_list;

use self::config::ToolConfig;

#[derive(Debug, Deserialize, Validate)]
pub struct BakeProject {
    /// Project name
    pub name: String,

    #[serde(skip)]
    /// Map of all cookbooks in the project
    pub cookbooks: BTreeMap<String, Cookbook>,

    #[serde(skip)]
    /// Map of all recipes by fully qualified name (eg. my-cookbook:my-recipe)
    pub recipes: BTreeMap<String, Recipe>,

    /// Project description
    pub description: Option<String>,

    /// Global project variables
    #[serde(default)]
    pub variables: IndexMap<String, String>,

    /// List of environment variables that should be available to all recipes
    #[serde(default)]
    pub environment: Vec<String>,

    #[serde(default)]
    #[validate]
    /// Main configuration of the project
    pub config: ToolConfig,

    #[serde(skip)]
    /// Root path of the project
    pub root_path: PathBuf,

    #[serde(skip)]
    /// Maps all dependencies, direct and indirect of each recipe in the project
    pub dependency_map: BTreeMap<String, HashSet<String>>,
}

impl BakeProject {
    /// Creates a bake project from a path to a bake.yml file or a directory in a bake project
    ///
    /// # Arguments
    /// * `path` - Path to either a config file or a directory. If a directory is passed,
    /// load_config will search for a bake.ya?ml file in that directory and in parent directories.
    ///
    pub fn from(path: &Path) -> anyhow::Result<Self> {
        // TODO: Better organize validation for config and recipes
        let file_path: PathBuf;
        let mut project: Self;

        if !path.exists() {
            bail!("Path does not exist: {}", path.display());
        }

        if path.is_dir() {
            file_path = Self::find_config_file_in_dir(path)?;
        } else if path.is_file() {
            file_path = PathBuf::from(path);
        } else {
            bail!("Invalid path");
        }

        let config_str = match std::fs::read_to_string(&file_path) {
            Ok(contents) => contents,
            Err(_) => {
                bail!("Could not read config file: {}", file_path.display());
            }
        };

        match serde_yaml::from_str::<Self>(&config_str) {
            Ok(mut parsed) => {
                if let Err(err) = parsed.validate() {
                    bail!("Could not parse config file: {}", err);
                }
                parsed.root_path = file_path.parent().unwrap().to_path_buf();
                project = parsed;
            }
            Err(err) => bail!("Could not parse config file: {}", err),
        }

        project.variables =
            parse_variable_list(project.environment.as_slice(), &project.variables)?;

        project.cookbooks =
            Cookbook::map_from(&project.root_path, &project.environment, &project.variables)?;

        project.recipes = project
            .cookbooks
            .iter()
            .flat_map(|(_, cookbook)| {
                cookbook
                    .recipes
                    .values()
                    .map(|recipe| (recipe.full_name(), recipe.clone()))
            })
            .collect();

        // let all_recipes = project.recipes(RecipeSearch::All);
        //
        // Validate if all recipe dependencies exist
        let err_msg = project
            .recipes
            .iter()
            .fold("".to_owned(), |msg, (_, recipe)| {
                let mut missing_deps: Vec<String> = Vec::new();
                if let Some(dependencies) = recipe.dependencies.as_ref() {
                    dependencies.iter().for_each(|dep| {
                        if project.recipes.get(dep).is_none() {
                            missing_deps.push(format!("\t- {}", dep));
                        }
                    });
                }
                if !missing_deps.is_empty() {
                    format!(
                        "{}{} {}:\n{}\n",
                        msg,
                        console::Emoji("📖", "in"),
                        recipe.config_path.display(),
                        missing_deps.join("\n"),
                    )
                } else {
                    msg
                }
            });

        if !err_msg.is_empty() {
            bail!(
                "{}:\n{}",
                console::style("Recipe dependencies not found").bold(),
                err_msg
            );
        }

        // Validate if project doesn't have circular dependencies
        match project.get_dependencies() {
            Ok(deps) => {
                project.dependency_map = deps;
            }
            Err(circular_dependency) => {
                let message = circular_dependency.iter().fold("".to_owned(), |acc, x| {
                    format!("{}\n{}", acc, x.join(" => "))
                });
                bail!("Circular dependencies detected:\n{:}", message);
            }
        }

        Ok(project)
    }

    pub fn create_project_bake_dirs(&self) -> anyhow::Result<()> {
        // Create .bake directories
        if let Err(err) = std::fs::create_dir_all(self.get_project_bake_path()) {
            bail!("Could not create .bake directory: {}", err);
        };

        if let Err(err) = std::fs::create_dir_all(self.get_project_log_path()) {
            bail!("Could not create logs directory: {}", err);
        };

        Ok(())
    }

    /// Recursively find a config file in a directory or its parent up until /
    /// or until the git repo root.
    fn find_config_file_in_dir(dir: &Path) -> anyhow::Result<PathBuf> {
        let file_yml = dir.join("bake.yml");
        let file_yaml = dir.join("bake.yaml");

        if file_yml.exists() {
            Ok(file_yml)
        } else if file_yaml.exists() {
            Ok(file_yaml)
        } else {
            let parent = dir.parent();

            // Stop if directory is root in the file system or in a git repository
            if let Some(parent) = parent {
                if !dir.join(".git").is_dir() {
                    return Self::find_config_file_in_dir(&PathBuf::from(parent));
                }
            }

            bail!("Could not find bake.yml");
        }
    }

    /// Returns a list of recipes given a recipe name pattern, including all dependent
    /// recipes recursively
    ///
    /// # Arguments
    /// * `cookbook_name` - Cookbook name
    /// * `recipe_name` - Recipe name
    ///
    pub fn get_recipes(&self, pattern: Option<&str>) -> BTreeMap<String, Recipe> {
        if let Some(pattern) = pattern {
            let filtered_recipes: BTreeMap<String, Recipe> = self
                .recipes
                .iter()
                .filter_map(|(name, recipe)| {
                    if name.contains(pattern) {
                        Some((name.clone(), recipe.clone()))
                    } else {
                        None
                    }
                })
                .collect();

            let mut recipes = filtered_recipes
                .iter()
                .flat_map(|(name, _)| {
                    self.dependency_map
                        .get(name)
                        .unwrap()
                        .iter()
                        .map(|dep| {
                            let dep_recipe = self.recipes.get(dep).unwrap().clone();
                            (dep.clone(), dep_recipe)
                        })
                        .collect::<Vec<(String, Recipe)>>()
                })
                .collect::<BTreeMap<String, Recipe>>();

            recipes.extend(filtered_recipes.clone());

            recipes
        } else {
            self.recipes.clone()
        }
    }

    /// Returns a map of all direct and indirect dependencies of all recipes if there are no circular dependencies
    /// or a list of all circular dependencies found
    fn get_dependencies(&self) -> Result<BTreeMap<String, HashSet<String>>, Vec<Vec<String>>> {
        // Context struct used for memoization during recursion
        struct Context<'a> {
            project: &'a BakeProject,
            visited: HashSet<String>,
            cur_path: Vec<String>,
            result: Vec<Vec<String>>,
            deps: BTreeMap<String, HashSet<String>>,
        }

        let mut ctx = Context {
            // recipes: &self.recipes(None, None),
            project: self,
            visited: HashSet::new(),
            cur_path: Vec::new(),
            result: Vec::new(),
            deps: BTreeMap::new(),
        };

        for recipe in self.recipes.values() {
            if !ctx.visited.contains(&recipe.name) {
                ctx.cur_path = Vec::new();
                check_cycle(&recipe.full_name(), &mut ctx);
                // ctx.deps.insert(recipe.full_name(), deps);
            }
        }

        fn check_cycle(cur_node_name: &str, ctx: &mut Context) {
            ctx.cur_path.push(cur_node_name.to_string());
            ctx.visited.insert(cur_node_name.to_string());
            if !ctx.deps.contains_key(cur_node_name) {
                ctx.deps.insert(cur_node_name.to_string(), HashSet::new());
            }

            if let Some(dependencies) = ctx
                .project
                .recipes
                .get(cur_node_name)
                .unwrap()
                .dependencies
                .as_ref()
            {
                dependencies.iter().for_each(|dep_name| {
                    // If current path contains dep_name, then it has a circular dependency.
                    // Add it to the result variable of Context
                    if ctx.cur_path.contains(dep_name) {
                        let mut path = ctx.cur_path.clone();
                        path.push(dep_name.to_string());
                        ctx.result.push(path);
                    }
                    // Check cycle for this dependencies dependency if we haven't visited it yet
                    if !ctx.visited.contains(dep_name) {
                        check_cycle(dep_name, ctx);
                    }

                    // Create a set of dependencies by getting all the deps of the current
                    // dependency being checked.
                    let mut deps = HashSet::new();
                    deps.insert(dep_name.clone());
                    deps.extend(ctx.deps.get(dep_name).unwrap().clone());

                    // Extend the set of dependencies for the current node with the deps of this
                    // dependency
                    ctx.deps.get_mut(cur_node_name).unwrap().extend(deps);
                })
            }
        }

        if !ctx.result.is_empty() {
            Err(ctx.result)
        } else {
            Ok(ctx.deps)
        }
    }

    pub fn get_recipe_log_path(&self, recipe_name: &str) -> PathBuf {
        self.get_project_log_path()
            .join(format!("{}.log", recipe_name.replace(':', ".")))
    }

    fn get_project_log_path(&self) -> PathBuf {
        self.get_project_bake_path().join("logs")
    }

    pub fn get_project_bake_path(&self) -> PathBuf {
        self.root_path.join(".bake")
    }
}

#[cfg(test)]
mod tests {
    use std::{os::unix::prelude::PermissionsExt, path::PathBuf};

    use test_case::test_case;

    fn config_path(path_str: &str) -> String {
        env!("CARGO_MANIFEST_DIR").to_owned() + "/resources/tests" + path_str
    }

    fn validate_project(project: anyhow::Result<super::BakeProject>) {
        let project = project.unwrap();
        assert_eq!(project.name, "test");
        assert_eq!(
            project.variables.get("bake_project_var"),
            Some(&"bar".to_string())
        );
        assert_eq!(
            project.recipes.get("foo:build").unwrap().variables["foo"],
            "build-bar"
        );
        assert_eq!(
            project.recipes.get("foo:build").unwrap().variables["baz"],
            "bar"
        );
        assert_eq!(
            project.recipes.get("foo:build").unwrap().run.trim(),
            "./build.sh build-bar test"
        );
        assert_eq!(
            project.recipes.get("foo:post-test").unwrap().variables["foo"],
            "bar"
        );
        // assert_eq!(project.recipes.len(), 5);
        // assert_eq!(project.recipes["foo:build"].name, "build");
    }

    #[test]
    fn get_dependencies() {
        let project = super::BakeProject::from(&PathBuf::from(config_path("/invalid/circular")));

        assert!(project
            .unwrap_err()
            .to_string()
            .contains("Circular dependencies"));

        let project = super::BakeProject::from(&PathBuf::from(config_path("/valid")));
        assert!(project.is_ok());
        let project = project.unwrap();
        assert_eq!(project.dependency_map.len(), 7);
        assert_eq!(project.dependency_map.get("bar:test").unwrap().len(), 2);
        assert_eq!(
            project.dependency_map.get("foo:post-test").unwrap().len(),
            3
        );
    }

    #[test_case(config_path("/valid/foo") => using validate_project; "Valid subdir")]
    #[test_case(config_path("/valid") => using validate_project; "Root dir")]
    #[test_case(config_path("/valid/bake.yml") => using validate_project; "Existing file")]
    #[test_case(config_path("/invalid/asdf") => matches Err(_); "Invalid subdir")]
    #[test_case(config_path("/invalid/circular") => matches Err(_); "Circular dependencies")]
    #[test_case(config_path("/invalid/recipes") => matches Err(_); "Inexistent recipes")]
    #[test_case(config_path("/invalid/config") => matches Err(_); "Invalid config")]
    #[test_case(config_path("/invalid/nobake/internal") => matches Err(_); "No bake file with .git root")]
    fn read_config(path_str: String) -> anyhow::Result<super::BakeProject> {
        std::env::set_var("TEST_BAKE_VAR", "test");
        super::BakeProject::from(&PathBuf::from(path_str))
    }

    #[test]
    fn invalid_permission() {
        let path = config_path("/invalid/permission/bake.yml");
        let mut perms = std::fs::metadata(&path).unwrap().permissions();
        let mode = perms.mode();
        perms.set_mode(0o200);
        std::fs::set_permissions(&path, perms.clone()).unwrap();
        let project = super::BakeProject::from(&PathBuf::from(config_path("/invalid/permission")));
        assert!(project.is_err());
        perms.set_mode(mode);
        std::fs::set_permissions(&path, perms.clone()).unwrap();
    }

    // #[test]
    // fn recipes() {
    //     let project = super::BakeProject::from(&PathBuf::from(config_path("/valid/"))).unwrap();
    //
    //     // Should return empty when not specifying format "<cookbook>:<recipe>"
    //     let recipes = project.get_recipes(RecipeSearch::ByPattern("foo"));
    //     assert_eq!(recipes.len(), 0);
    //
    //     let recipes = project.get_recipes(RecipeSearch::ByPattern("foo:build"));
    //     assert_eq!(recipes.len(), 1);
    //     assert_eq!(recipes[0].name, "build");
    //
    //     let recipes = project.get_recipes(RecipeSearch::ByPattern("foo:"));
    //     assert_eq!(recipes.len(), 3);
    //
    //     let recipes = project.get_recipes(RecipeSearch::ByPattern(":build"));
    //     assert_eq!(recipes.len(), 2);
    //
    //     let recipes = project.get_recipes(RecipeSearch::ByPattern(":test"));
    //     assert_eq!(recipes.len(), 2);
    //
    //     let recipes = project.get_recipes(RecipeSearch::All);
    //     assert_eq!(recipes.len(), 6);
    // }
}
