pub mod config;
pub mod cookbook;
pub mod graph;
pub mod hashing;
pub mod recipe;

use anyhow::bail;

pub use cookbook::*;
use indexmap::IndexMap;
// Note: Some petgraph imports were moved or are now managed by RecipeDependencyGraph.
use self::graph::RecipeDependencyGraph;
pub use recipe::*;

pub use validator::Validate;

use std::{
    collections::{BTreeMap, HashSet},
    path::{Path, PathBuf},
};

use serde::Deserialize;

use crate::template::parse_variable_list;

use self::config::ToolConfig;

/// Represents a Bake project, including its configuration, cookbooks, recipes,
/// and dependency information.
///
/// A `BakeProject` is the central structure for managing and executing build tasks.
/// It is typically deserialized from a `bake.yml` or `bake.yaml` file.
#[derive(Debug, Deserialize, Validate)]
#[allow(dead_code)]
pub struct BakeProject {
    /// The name of the project.
    pub name: String,

    #[serde(skip)]
    /// A map of all cookbooks in the project, keyed by cookbook name.
    /// Cookbooks are collections of recipes.
    pub cookbooks: BTreeMap<String, Cookbook>,

    // #[serde(skip)]
    // pub recipes: BTreeMap<String, Recipe>, // This field was replaced by recipe_data and recipe_graph.

    // Stores all Recipe objects, keyed by their fully qualified name.
    // This was removed as Recipes are now accessed via their respective cookbooks.
    // #[serde(skip)]
    // pub recipe_data: BTreeMap<String, Recipe>,
    /// Encapsulates the recipe dependency graph and provides methods for
    /// querying dependencies and execution order.
    #[serde(skip)]
    pub recipe_dependency_graph: RecipeDependencyGraph,

    /// An optional description of the project.
    pub description: Option<String>,

    /// Global variables defined at the project level.
    /// These variables are available to all recipes in the project.
    #[serde(default)]
    pub variables: IndexMap<String, String>,

    /// A list of environment variables that should be sourced and made
    /// available to all recipes during execution.
    #[serde(default)]
    pub environment: Vec<String>,

    #[serde(default)]
    #[validate(nested)]
    /// The main configuration settings for the Bake tool within this project.
    pub config: ToolConfig,

    #[serde(skip)]
    /// The root path of the project, typically the directory containing the `bake.yml` file.
    pub root_path: PathBuf,
    //#[serde(skip)]
    //// Maps all dependencies, direct and indirect of each recipe in the project.
    // pub dependency_map: BTreeMap<String, HashSet<String>>, // This was replaced by recipe_graph.
}

impl BakeProject {
    /// Retrieves a reference to a specific `Recipe` within the project
    /// using its fully qualified name (FQN).
    ///
    /// The FQN is in the format "cookbook_name:recipe_name".
    ///
    /// # Arguments
    ///
    /// * `fqn` - The fully qualified name of the recipe to retrieve.
    ///
    /// # Returns
    ///
    /// An `Option<&Recipe>` which is `Some` if the recipe is found,
    /// or `None` if no recipe matches the FQN or the FQN format is invalid.
    pub fn get_recipe_by_fqn(&self, fqn: &str) -> Option<&Recipe> {
        if let Some((cookbook_name, recipe_name)) = fqn.split_once(':') {
            self.cookbooks
                .get(cookbook_name)
                .and_then(|cb| cb.recipes.get(recipe_name))
        } else {
            None // The provided FQN does not follow the "cookbook:recipe" format.
        }
    }

    /// Creates a `BakeProject` instance by loading and parsing a configuration file.
    ///
    /// This function searches for a `bake.yml` or `bake.yaml` file starting from the given `path`.
    /// If `path` is a directory, it searches within that directory and its parent directories.
    /// If `path` is a file, it attempts to load that file directly.
    ///
    /// The loaded configuration is validated, and project variables are processed,
    /// incorporating any `override_variables` provided. Cookbooks are loaded,
    /// and the recipe dependency graph is populated.
    ///
    /// # Arguments
    ///
    /// * `path` - A `Path` to either a configuration file or a directory within a Bake project.
    /// * `override_variables` - An `IndexMap` of variables to override project-level and
    ///   cookbook-level variables.
    ///
    /// # Returns
    ///
    /// A `Result<Self, anyhow::Error>` which is `Ok(BakeProject)` on successful loading
    /// and parsing, or an `Err` if any step fails (e.g., file not found, parsing error,
    /// validation error, circular dependency).
    /// Finds the configuration file and reads it into a string.
    fn find_and_load_config_str(path: &Path) -> anyhow::Result<(PathBuf, String)> {
        let file_path = if !path.exists() {
            bail!(
                "Project Load: Configuration path '{}' does not exist.",
                path.display()
            );
        } else if path.is_dir() {
            Self::find_config_file_in_dir(path)?
        } else if path.is_file() {
            PathBuf::from(path)
        } else {
            bail!(
                "Project Load: Invalid configuration path '{}'. It is not a file or a directory.",
                path.display()
            );
        };

        match std::fs::read_to_string(&file_path) {
            Ok(contents) => Ok((file_path, contents)),
            Err(err) => {
                bail!(
                    "Project Load: Failed to read configuration file '{}': {}",
                    file_path.display(),
                    err
                );
            }
        }
    }

    /// Parses the configuration string, validates the project, and sets the root path.
    fn parse_and_validate_project(file_path: &Path, config_str: &str) -> anyhow::Result<Self> {
        match serde_yaml::from_str::<Self>(config_str) {
            Ok(mut parsed) => {
                if let Err(err) = parsed.validate() {
                    bail!("Project Load: Configuration file '{}' validation failed: {}", file_path.display(), err);
                }
                parsed.root_path = file_path
                    .parent()
                    .expect("Config file must have a parent directory.")
                    .to_path_buf();
                Ok(parsed)
            }
            Err(err) => bail!(
                "Project Load: Failed to parse configuration file '{}': {}. Check YAML syntax and project structure.",
                file_path.display(),
                err
            ),
        }
    }

    /// Initializes project-level variables.
    fn initialize_project_variables(
        &mut self,
        override_variables: &IndexMap<String, String>,
    ) -> anyhow::Result<()> {
        let project_constants = IndexMap::from([(
            "root".to_owned(),
            self.root_path.clone().display().to_string(),
        )]);

        self.variables = parse_variable_list(
            self.environment.as_slice(),
            &self.variables,
            &IndexMap::from([("project".to_owned(), project_constants)]),
            override_variables,
        )?;
        Ok(())
    }

    /// Loads cookbooks for the project.
    fn load_project_cookbooks(
        &mut self,
        override_variables: &IndexMap<String, String>,
    ) -> anyhow::Result<()> {
        let project_constants = IndexMap::from([(
            "root".to_owned(),
            self.root_path.clone().display().to_string(),
        )]);
        self.cookbooks = Cookbook::map_from(
            &self.root_path,
            &self.environment,
            &self.variables,
            &project_constants,
            override_variables,
        )?;
        Ok(())
    }

    /// Populates the recipe dependency graph.
    fn populate_dependency_graph(&mut self) -> anyhow::Result<()> {
        self.recipe_dependency_graph = RecipeDependencyGraph::new();
        self.recipe_dependency_graph
            .populate_from_cookbooks(&self.cookbooks)?;
        Ok(())
    }

    pub fn from(path: &Path, override_variables: IndexMap<String, String>) -> anyhow::Result<Self> {
        // Find and load the configuration file content.
        let (file_path, config_str) = Self::find_and_load_config_str(path)?;

        // Parse the configuration string, validate, and set the root path.
        let mut project = Self::parse_and_validate_project(&file_path, &config_str)?;

        // Initialize project-level variables.
        project.initialize_project_variables(&override_variables)?;

        // Load cookbooks for the project.
        project.load_project_cookbooks(&override_variables)?;

        // Populate the recipe dependency graph.
        project.populate_dependency_graph()?;

        Ok(project)
    }

    /// Retrieves the combined hash for a specified recipe.
    ///
    /// This hash is calculated based on the recipe's own content and the hashes
    /// of its dependencies. It is used to determine if a recipe or its inputs
    /// have changed, potentially requiring a rebuild.
    ///
    /// # Arguments
    ///
    /// * `recipe_fqn` - The fully qualified name of the recipe.
    ///
    /// # Returns
    ///
    /// A `Result<String, anyhow::Error>` containing the combined hash string if successful,
    /// or an error if the hash calculation fails (e.g., recipe not found).
    pub fn get_combined_hash_for_recipe(&self, recipe_fqn: &str) -> anyhow::Result<String> {
        hashing::calculate_combined_hash_for_recipe(recipe_fqn, self)
    }

    /// Creates the necessary `.bake` and `.bake/logs` directories within the project root.
    ///
    /// These directories are used by Bake to store metadata, cache, and log files.
    ///
    /// # Returns
    ///
    /// An `anyhow::Result<()>` which is `Ok(())` if the directories are created successfully
    /// or already exist, or an `Err` if directory creation fails.
    pub fn create_project_bake_dirs(&self) -> anyhow::Result<()> {
        // Create the main .bake directory.
        if let Err(err) = std::fs::create_dir_all(self.get_project_bake_path()) {
            bail!(
                "Project Setup: Failed to create project .bake directory at '{}': {}",
                self.get_project_bake_path().display(),
                err
            );
        };
        // Create the logs subdirectory within .bake.
        if let Err(err) = std::fs::create_dir_all(self.get_project_log_path()) {
            bail!(
                "Project Setup: Failed to create project logs directory at '{}': {}",
                self.get_project_log_path().display(),
                err
            );
        };

        Ok(())
    }

    /// Recursively searches for a `bake.yml` or `bake.yaml` configuration file.
    ///
    /// The search starts in the specified `dir` and proceeds upwards to parent directories.
    /// The search stops if it reaches the filesystem root or a directory containing a `.git` folder
    /// (indicating the root of a Git repository).
    ///
    /// # Arguments
    ///
    /// * `dir` - The directory to start the search from.
    ///
    /// # Returns
    ///
    /// A `Result<PathBuf, anyhow::Error>` containing the path to the found configuration file,
    /// or an `Err` if no configuration file is found.
    fn find_config_file_in_dir(dir: &Path) -> anyhow::Result<PathBuf> {
        let file_yml = dir.join("bake.yml");
        let file_yaml = dir.join("bake.yaml");

        if file_yml.exists() {
            Ok(file_yml)
        } else if file_yaml.exists() {
            Ok(file_yaml)
        } else {
            let parent = dir.parent();

            // Stop search if we are at the filesystem root or a git repository root.
            if let Some(parent_dir) = parent {
                if !dir.join(".git").is_dir() {
                    // Continue searching in the parent directory.
                    return Self::find_config_file_in_dir(parent_dir);
                }
            }
            // If no config file is found after checking all relevant directories.
            bail!(
                "Project Load: bake.yml or bake.yaml not found in '{}' or any parent directory. Ensure a configuration file exists at the project root.",
                dir.display()
            );
        }
    }

    /// Determines the execution plan for recipes based on a pattern and their dependencies.
    ///
    /// This method identifies a set of target recipes based on the optional `pattern`.
    /// If no pattern is provided, all recipes in the project are considered targets.
    /// It then includes all dependencies of these target recipes.
    ///
    /// The final list of recipes is organized into levels, where recipes within the same
    /// level can be executed in parallel, and recipes in later levels depend on the
    /// completion of recipes in earlier levels. This is achieved using Kahn's algorithm
    /// for topological sorting on the relevant subgraph of recipes.
    ///
    /// # Arguments
    ///
    /// * `pattern` - An optional string pattern. Recipes whose fully qualified names
    ///   contain this pattern are initially selected as targets. If `None`, all recipes
    ///   are considered initial targets.
    ///
    /// # Returns
    ///
    /// A `Result<Vec<Vec<Recipe>>, anyhow::Error>` where:
    /// - `Ok(Vec<Vec<Recipe>>)`: A vector of vectors of `Recipe` objects. Each inner
    ///   vector represents a level of recipes that can be executed in parallel.
    ///   Levels are ordered according to their dependencies.
    /// - `Err(anyhow::Error)`: An error if issues occur, such as:
    ///   - A cycle is detected in the recipe dependency graph for the targeted recipes.
    ///   - A recipe specified in the graph cannot be found in the project's cookbooks.
    ///   - An unexpected state occurs during the planning process.
    ///
    /// If no recipes match the pattern or if the project has no recipes, an empty
    /// vector `Vec::new()` is returned successfully.
    pub fn get_recipes_for_execution(
        &self,
        pattern: Option<&str>,
    ) -> anyhow::Result<Vec<Vec<Recipe>>> {
        let initial_target_fqns: HashSet<String> = if let Some(p_str) = pattern {
            self.recipe_dependency_graph
                .fqn_to_node_index
                .keys()
                .filter(|fqn| fqn.contains(p_str))
                .cloned()
                .collect()
        } else {
            // If no pattern is provided, all recipes in the project are initial targets.
            self.recipe_dependency_graph
                .fqn_to_node_index
                .keys()
                .cloned()
                .collect()
        };

        if initial_target_fqns.is_empty() && pattern.is_some() {
            // Only return early if a pattern was given and it yielded no matches.
            // If no pattern was given and initial_target_fqns is empty, it means an empty project,
            // which get_execution_plan_for_initial_targets will handle by returning Ok(Vec::new()).
            return Ok(Vec::new());
        }

        // Get the full execution plan (including all dependencies and sorted levels) from the graph.
        let fqn_levels = self
            .recipe_dependency_graph
            .get_execution_plan_for_initial_targets(&initial_target_fqns)?;

        // If fqn_levels is empty, it means no recipes need to be run (e.g., empty project,
        // or initial targets had no dependencies and were themselves empty after some filtering,
        // or a cycle was detected and an error was returned by the graph method).
        // The graph method already handles returning Ok(Vec::new()) for an empty initial_target_fqns set.
        if fqn_levels.is_empty() {
            return Ok(Vec::new());
        }

        let mut result_levels: Vec<Vec<Recipe>> = Vec::new();

        for fqn_level in fqn_levels {
            let mut recipes_this_level = Vec::new();
            for fqn in fqn_level {
                match self.get_recipe_by_fqn(&fqn) {
                    Some(recipe_ref) => recipes_this_level.push(recipe_ref.clone()),
                    None => bail!(
                        "Execution Plan: Recipe '{}' from the execution plan was not found in the loaded project cookbooks. \
                        This suggests an internal inconsistency, possibly due to a corrupted or manually altered dependency graph state.",
                        fqn
                    ),
                }
            }
            // Sorting recipes within a level by FQN for deterministic output, if desired.
            // recipes_this_level.sort_by(|a, b| a.full_name().cmp(&b.full_name()));
            // Note: The FQNs from get_execution_order_for_targets are already sorted within each level.
            // So, the order of `recipes_this_level` will correspond to that sorted FQN order.

            if !recipes_this_level.is_empty() {
                result_levels.push(recipes_this_level);
            }
        }

        Ok(result_levels)
    }

    /// Constructs the path to the log file for a given recipe.
    ///
    /// Log files are stored in the `.bake/logs/` directory within the project root.
    /// The filename is derived from the recipe's fully qualified name, with colons
    /// replaced by periods (e.g., `cookbook:recipe` becomes `cookbook.recipe.log`).
    ///
    /// # Arguments
    ///
    /// * `recipe_name` - The fully qualified name of the recipe.
    ///
    /// # Returns
    ///
    /// A `PathBuf` representing the absolute path to the recipe's log file.
    pub fn get_recipe_log_path(&self, recipe_name: &str) -> PathBuf {
        self.get_project_log_path()
            .join(format!("{}.log", recipe_name.replace(':', ".")))
    }

    /// Returns the path to the project's log directory (`.bake/logs`).
    fn get_project_log_path(&self) -> PathBuf {
        self.get_project_bake_path().join("logs")
    }

    /// Returns the path to the project's main Bake directory (`.bake`).
    /// This directory is used for storing Bake-specific files like cache and logs.
    pub fn get_project_bake_path(&self) -> PathBuf {
        self.root_path.join(".bake")
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, os::unix::prelude::PermissionsExt, path::PathBuf};

    use indexmap::IndexMap;
    use test_case::test_case;

    use crate::project::Recipe;

    fn config_path(path_str: &str) -> String {
        env!("CARGO_MANIFEST_DIR").to_owned() + "/resources/tests" + path_str
    }

    fn validate_project(project_result: anyhow::Result<super::BakeProject>) {
        let project = project_result.unwrap();
        assert_eq!(project.name, "test");
        assert_eq!(
            project.variables.get("bake_project_var"),
            Some(&"bar".to_string())
        );

        // Fetch all recipes and convert to a BTreeMap for easy lookup in tests
        let all_recipes_staged = project.get_recipes_for_execution(None).unwrap();
        let recipes_map: BTreeMap<String, Recipe> = all_recipes_staged
            .into_iter()
            .flatten()
            .map(|r| (r.full_name(), r))
            .collect();

        assert_eq!(
            recipes_map.get("foo:build").unwrap().variables["foo"],
            "build-bar"
        );
        assert_eq!(
            recipes_map.get("foo:build").unwrap().variables["baz"],
            "bar"
        );
        assert_eq!(
            recipes_map.get("foo:build").unwrap().run.trim(),
            format!("./build.sh build-bar test {}", project.root_path.display())
        );
        assert_eq!(
            recipes_map.get("foo:post-test").unwrap().variables["foo"],
            "bar"
        );
        // assert_eq!(recipes_map.len(), 7); // Update this count based on your valid project
        // assert_eq!(recipes_map["foo:build"].name, "build");
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
        super::BakeProject::from(&PathBuf::from(path_str), IndexMap::new())
    }

    #[test]
    fn invalid_permission() {
        let path = config_path("/invalid/permission/bake.yml");
        let mut perms = std::fs::metadata(&path).unwrap().permissions();
        let mode = perms.mode();
        perms.set_mode(0o200);
        std::fs::set_permissions(&path, perms.clone()).unwrap();
        let project = super::BakeProject::from(
            &PathBuf::from(config_path("/invalid/permission")),
            IndexMap::new(),
        );
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
