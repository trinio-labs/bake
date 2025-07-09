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

use crate::template::VariableContext;

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
        let mut context = VariableContext::builder()
            .environment(self.environment.clone())
            .variables(self.variables.clone())
            .overrides(override_variables.clone())
            .build();

        // Add project constants
        context.merge(&VariableContext::with_project_constants(&self.root_path));

        self.variables = context.process_variables()?;
        Ok(())
    }

    /// Loads cookbooks for the project.
    fn load_project_cookbooks(
        &mut self,
        override_variables: &IndexMap<String, String>,
    ) -> anyhow::Result<()> {
        let context = VariableContext::builder()
            .environment(self.environment.clone())
            .variables(self.variables.clone())
            .overrides(override_variables.clone())
            .build();

        self.cookbooks = Cookbook::map_from(&self.root_path, &context)?;
        Ok(())
    }

    /// Populates the recipe dependency graph.
    fn populate_dependency_graph(&mut self) -> anyhow::Result<()> {
        self.recipe_dependency_graph = RecipeDependencyGraph::new();
        self.recipe_dependency_graph
            .populate_from_cookbooks(&self.cookbooks)?;
        Ok(())
    }

    /// Validates the minimum bake version required by this project configuration.
    /// Validates project version compatibility assuming backward compatibility.
    /// Only prevents running if the project requires a newer version than current.
    fn validate_min_version(&self, force_version_override: bool) -> anyhow::Result<()> {
        let current_version = env!("CARGO_PKG_VERSION");

        if let Some(project_version) = &self.config.min_version {
            if project_version != current_version {
                // Parse versions to compare
                let current_parts: Vec<u32> = current_version
                    .split('.')
                    .filter_map(|s| s.parse().ok())
                    .collect();
                let project_parts: Vec<u32> = project_version
                    .split('.')
                    .filter_map(|s| s.parse().ok())
                    .collect();

                // Compare version tuples (major, minor, patch)
                let cmp = project_parts.cmp(&current_parts);
                if cmp == std::cmp::Ordering::Greater {
                    // Project requires newer version than current
                    if !force_version_override {
                        anyhow::bail!(
                            "❌ This project requires bake v{} but you're running v{}.\n   Please upgrade your bake installation to match or exceed the project version, or use --force-version-override to bypass this check.",
                            project_version, current_version
                        );
                    } else {
                        eprintln!(
                            "⚠️  Forced override: This project requires bake v{project_version} but you're running v{current_version}. Proceeding with force override.",
                        );
                    }
                } else if cmp == std::cmp::Ordering::Less {
                    // Project version is older than current - assume backward compatibility
                    // Only show deprecation warnings if configuration uses deprecated features
                    self.check_deprecated_configuration(project_version, current_version);
                }
                // If versions are equal, no action needed
            }
        } else {
            // No version specified - this is an older project
            eprintln!(
                "ℹ️  Info: This project doesn't specify a minimum bake version (created with bake v{current_version})",
            );
        }

        Ok(())
    }

    /// Checks for deprecated configuration options and shows appropriate warnings.
    /// This method is called when the project version is older than the current bake version.
    fn check_deprecated_configuration(&self, project_version: &str, current_version: &str) {
        let warnings: Vec<&str> = Vec::new();

        // Check for deprecated configuration patterns
        // TODO: Add specific deprecation checks as features are deprecated

        // Example deprecation check structure:
        // if self.has_deprecated_config_option() {
        //     warnings.push("Configuration option 'old_option' is deprecated since v1.2.0. Use 'new_option' instead.");
        // }

        // Show warnings if any deprecated features are found
        if !warnings.is_empty() {
            eprintln!("⚠️  Deprecated configuration detected in project (v{project_version} → v{current_version}):");
            for warning in warnings {
                eprintln!("   • {warning}");
            }
            eprintln!(
                "   Consider updating your project configuration with: bake --update-version"
            );
        }
    }

    /// Updates the minimum bake version to the current version.
    /// This should be called when the project configuration is modified.
    pub fn update_min_version(&mut self) {
        self.config.min_version = Some(env!("CARGO_PKG_VERSION").to_string());
    }

    /// Saves the project configuration back to the original file.
    /// This is useful for updating the bake version or other configuration changes.
    pub fn save_configuration(&self) -> anyhow::Result<()> {
        let config_path = self.root_path.join("bake.yml");

        // Create a temporary struct for serialization (excluding skip fields)
        #[derive(serde::Serialize)]
        struct BakeProjectConfig<'a> {
            name: &'a str,
            description: &'a Option<String>,
            variables: &'a IndexMap<String, String>,
            environment: &'a Vec<String>,
            config: &'a ToolConfig,
        }

        let config = BakeProjectConfig {
            name: &self.name,
            description: &self.description,
            variables: &self.variables,
            environment: &self.environment,
            config: &self.config,
        };

        let yaml = serde_yaml::to_string(&config)?;
        std::fs::write(&config_path, yaml)?;

        Ok(())
    }

    pub fn from(
        path: &Path,
        override_variables: IndexMap<String, String>,
        force_version_override: bool,
    ) -> anyhow::Result<Self> {
        // Find and load the configuration file content.
        let (file_path, config_str) = Self::find_and_load_config_str(path)?;

        // Parse the configuration string, validate, and set the root path.
        let mut project = Self::parse_and_validate_project(&file_path, &config_str)?;

        // Validate bake version compatibility
        project.validate_min_version(force_version_override)?;

        // Initialize project-level variables.
        project.initialize_project_variables(&override_variables)?;

        // Load cookbooks for the project.
        project.load_project_cookbooks(&override_variables)?;

        // Populate the recipe dependency graph.
        project.populate_dependency_graph()?;

        Ok(project)
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
    /// * `pattern` - An optional string pattern in the format:
    ///   - `cookbook:recipe` - Execute a specific recipe from a specific cookbook
    ///   - `cookbook:` - Execute all recipes in a cookbook
    ///   - `:recipe` - Execute all recipes with that name across all cookbooks
    ///     Both cookbook and recipe parts support regex patterns.
    ///     If `None`, all recipes are considered initial targets.
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
    ///   - Invalid regex pattern is provided.
    ///   - Pattern does not contain required ':' separator.
    ///
    /// If no recipes match the pattern or if the project has no recipes, an empty
    /// vector `Vec::new()` is returned successfully.
    pub fn get_recipes_for_execution(
        &self,
        pattern: Option<&str>,
        use_regex: bool,
    ) -> anyhow::Result<Vec<Vec<Recipe>>> {
        let initial_target_fqns: HashSet<String> = if let Some(p_str) = pattern {
            self.filter_recipes_by_pattern(p_str, use_regex)?
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

    /// Filters recipes based on a colon-separated pattern with regex support.
    ///
    /// # Arguments
    ///
    /// * `pattern` - The pattern in format:
    ///   - `cookbook:recipe` - Execute a specific recipe from a specific cookbook
    ///   - `cookbook:` - Execute all recipes in a cookbook
    ///   - `:recipe` - Execute all recipes with that name across all cookbooks
    ///     Both cookbook and recipe parts support regex patterns.
    ///
    /// # Returns
    ///
    /// A `Result<HashSet<String>, anyhow::Error>` containing the FQNs of matching recipes.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The pattern does not contain a ':' separator
    /// - The regex pattern is invalid
    fn filter_recipes_by_pattern(
        &self,
        pattern: &str,
        use_regex: bool,
    ) -> anyhow::Result<HashSet<String>> {
        // Require ':' separator
        if !pattern.contains(':') {
            bail!(
                "Command Error: Pattern '{}' must contain ':' separator. Use:\n  \
                 <cookbook>:<recipe> - for a specific recipe\n  \
                 <cookbook>: - for all recipes in a cookbook\n  \
                 :<recipe> - for all recipes with that name across all cookbooks",
                pattern
            );
        }

        let (cookbook_pattern, recipe_pattern) = pattern.split_once(':').unwrap();

        let mut matching_fqns = HashSet::new();

        if use_regex {
            // Compile regex patterns
            let cookbook_regex = if cookbook_pattern.is_empty() {
                None
            } else {
                Some(regex::Regex::new(cookbook_pattern).map_err(|e| {
                    anyhow::anyhow!(
                        "Command Error: Invalid regex pattern for cookbook '{}': {}",
                        cookbook_pattern,
                        e
                    )
                })?)
            };

            let recipe_regex = if recipe_pattern.is_empty() {
                None
            } else {
                Some(regex::Regex::new(recipe_pattern).map_err(|e| {
                    anyhow::anyhow!(
                        "Command Error: Invalid regex pattern for recipe '{}': {}",
                        recipe_pattern,
                        e
                    )
                })?)
            };

            // Filter recipes based on regex patterns
            for fqn in self.recipe_dependency_graph.fqn_to_node_index.keys() {
                if let Some((cookbook_name, recipe_name)) = fqn.split_once(':') {
                    let cookbook_matches = cookbook_regex
                        .as_ref()
                        .map(|re| re.is_match(cookbook_name))
                        .unwrap_or(true);

                    let recipe_matches = recipe_regex
                        .as_ref()
                        .map(|re| re.is_match(recipe_name))
                        .unwrap_or(true);

                    if cookbook_matches && recipe_matches {
                        matching_fqns.insert(fqn.clone());
                    }
                }
            }
        } else {
            // Use exact string matching
            for fqn in self.recipe_dependency_graph.fqn_to_node_index.keys() {
                if let Some((cookbook_name, recipe_name)) = fqn.split_once(':') {
                    let cookbook_matches = if cookbook_pattern.is_empty() {
                        true
                    } else {
                        cookbook_name == cookbook_pattern
                    };

                    let recipe_matches = if recipe_pattern.is_empty() {
                        true
                    } else {
                        recipe_name == recipe_pattern
                    };

                    if cookbook_matches && recipe_matches {
                        matching_fqns.insert(fqn.clone());
                    }
                }
            }
        }

        Ok(matching_fqns)
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
        let all_recipes_staged = project.get_recipes_for_execution(None, false).unwrap();
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
        super::BakeProject::from(&PathBuf::from(path_str), IndexMap::new(), false)
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
            false,
        );
        assert!(project.is_err());
        perms.set_mode(mode);
        std::fs::set_permissions(&path, perms.clone()).unwrap();
    }

    #[test]
    fn test_min_version_validation() {
        use std::fs;
        use tempfile::tempdir;

        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("bake.yml");

        // Create a test configuration with a specific minimum version
        let config_content = r#"
name: test_project
config:
  minVersion: "0.4.0"
variables:
  test_var: "test_value"
"#;

        fs::write(&config_path, config_content).unwrap();

        // Test that version validation works
        let result = super::BakeProject::from(temp_dir.path(), IndexMap::new(), false);
        assert!(result.is_ok());

        let project = result.unwrap();
        assert_eq!(project.config.min_version, Some("0.4.0".to_string()));
    }

    #[test]
    fn test_update_min_version() {
        use std::fs;
        use tempfile::tempdir;

        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("bake.yml");

        // Create a test configuration without minimum version
        let config_content = r#"
name: test_project
variables:
  test_var: "test_value"
"#;

        fs::write(&config_path, config_content).unwrap();

        // Load project and update version
        let mut project =
            super::BakeProject::from(temp_dir.path(), IndexMap::new(), false).unwrap();
        project.update_min_version();

        // Save configuration
        project.save_configuration().unwrap();

        // Verify the version was updated
        let updated_content = fs::read_to_string(&config_path).unwrap();
        assert!(updated_content.contains("minVersion:"));
        assert!(updated_content.contains(env!("CARGO_PKG_VERSION")));
    }

    fn get_test_project() -> super::BakeProject {
        std::env::set_var("TEST_BAKE_VAR", "test");
        super::BakeProject::from(
            &PathBuf::from(config_path("/valid")),
            IndexMap::new(),
            false,
        )
        .unwrap()
    }

    #[test_case("foo:build"; "Exact cookbook and recipe match")]
    #[test_case("foo:"; "Exact cookbook match")]
    #[test_case(":build"; "Exact recipe match")]
    #[test_case("bar:unique-recipe"; "Exact match for unique recipe")]
    fn test_filter_recipes_by_pattern_exact_matching(pattern: &str) {
        let project = get_test_project();
        let result = project.filter_recipes_by_pattern(pattern, false).unwrap();
        assert!(!result.is_empty());

        // Verify exact matching behavior
        for fqn in &result {
            if let Some((cookbook_name, recipe_name)) = fqn.split_once(':') {
                let (cookbook_pattern, recipe_pattern) = pattern.split_once(':').unwrap();

                if !cookbook_pattern.is_empty() {
                    assert_eq!(
                        cookbook_name, cookbook_pattern,
                        "Cookbook name should match exactly"
                    );
                }
                if !recipe_pattern.is_empty() {
                    assert_eq!(
                        recipe_name, recipe_pattern,
                        "Recipe name should match exactly"
                    );
                }
            }
        }
    }

    #[test_case("foo_something:build"; "Non-existent cookbook with similar name")]
    #[test_case("foo:build_something"; "Non-existent recipe with similar name")]
    #[test_case("my_cookbook:build"; "Partial cookbook name should not match")]
    fn test_filter_recipes_by_pattern_exact_no_matches(pattern: &str) {
        let project = get_test_project();
        let result = project.filter_recipes_by_pattern(pattern, false).unwrap();
        assert!(
            result.is_empty(),
            "Exact matching should not match similar names"
        );
    }

    #[test_case("build" => matches Err(_); "Missing colon separator")]
    #[test_case("build-test" => matches Err(_); "Missing colon separator with dash")]
    #[test_case("^[unclosed:" => matches Err(_); "Invalid regex in cookbook")]
    #[test_case(":^[unclosed" => matches Err(_); "Invalid regex in recipe")]
    fn test_filter_recipes_by_pattern_errors(
        pattern: &str,
    ) -> anyhow::Result<std::collections::HashSet<String>> {
        let project = get_test_project();
        project.filter_recipes_by_pattern(pattern, true)
    }

    #[test_case("foo:"; "Cookbook only")]
    #[test_case(":test"; "Recipe only")]
    #[test_case("foo:build"; "Specific recipe")]
    #[test_case("^f.*:"; "Regex cookbook pattern")]
    #[test_case(":^build"; "Regex recipe pattern")]
    #[test_case("^f.*:^build"; "Regex both patterns")]
    fn test_filter_recipes_by_pattern_success(pattern: &str) {
        let project = get_test_project();
        let result = project.filter_recipes_by_pattern(pattern, true).unwrap();
        assert!(!result.is_empty());
    }

    #[test_case("nonexistent:recipe"; "Nonexistent cookbook and recipe")]
    #[test_case("foo:nonexistent"; "Existing cookbook, nonexistent recipe")]
    #[test_case("nonexistent:"; "Nonexistent cookbook")]
    #[test_case(":nonexistent"; "Nonexistent recipe")]
    fn test_filter_recipes_by_pattern_no_matches(pattern: &str) {
        let project = get_test_project();
        let result = project.filter_recipes_by_pattern(pattern, true).unwrap();
        assert!(result.is_empty());
    }

    #[test_case("foo:"; "Cookbook filter")]
    #[test_case(":test"; "Recipe filter")]
    #[test_case(":"; "Match all")]
    fn test_filter_recipes_by_pattern_validation(pattern: &str) {
        let project = get_test_project();
        let result = project.filter_recipes_by_pattern(pattern, true).unwrap();
        assert!(!result.is_empty());

        match pattern {
            p if p.starts_with(':') && !p.ends_with(':') => {
                // Recipe only pattern (:recipe)
                let recipe_name = &p[1..];
                for fqn in &result {
                    let fqn_recipe = fqn.split(':').nth(1).unwrap();
                    // Current implementation uses regex matching, so "test" matches "post-test"
                    let re = regex::Regex::new(recipe_name).unwrap();
                    assert!(re.is_match(fqn_recipe));
                }
            }
            p if p.ends_with(':') && !p.starts_with(':') => {
                // Cookbook only pattern (cookbook:)
                let cookbook_name = &p[..p.len() - 1];
                for fqn in &result {
                    if cookbook_name.contains('^') || cookbook_name.contains('*') {
                        // Regex pattern - just check it matches something from expected cookbook
                        let cookbook = fqn.split(':').next().unwrap();
                        let re = regex::Regex::new(cookbook_name).unwrap();
                        assert!(re.is_match(cookbook));
                    } else {
                        // Exact match for cookbook
                        assert!(fqn.starts_with(&format!("{cookbook_name}:")));
                    }
                }
            }
            ":" => {
                // Match all - just verify we got results
                assert!(!result.is_empty());
            }
            _ => {
                // Specific recipe pattern (cookbook:recipe)
                if pattern.contains('^') || pattern.contains('*') {
                    // Contains regex - verify each part matches
                    let parts: Vec<&str> = pattern.split(':').collect();
                    for fqn in &result {
                        let fqn_parts: Vec<&str> = fqn.split(':').collect();
                        if !parts[0].is_empty() {
                            let cookbook_re = regex::Regex::new(parts[0]).unwrap();
                            assert!(cookbook_re.is_match(fqn_parts[0]));
                        }
                        if !parts[1].is_empty() {
                            let recipe_re = regex::Regex::new(parts[1]).unwrap();
                            assert!(recipe_re.is_match(fqn_parts[1]));
                        }
                    }
                } else {
                    // Exact match
                    assert!(result.contains(pattern));
                }
            }
        }
    }

    #[test_case("foo:"; "Valid cookbook pattern")]
    #[test_case(":test"; "Valid recipe pattern")]
    #[test_case("foo:build"; "Valid specific pattern")]
    fn test_get_recipes_for_execution_with_patterns(pattern: &str) {
        let project = get_test_project();
        let result = project.get_recipes_for_execution(Some(pattern), true);
        assert!(result.is_ok());
        assert!(!result.unwrap().is_empty());
    }

    #[test_case("foo:build"; "Exact match pattern")]
    #[test_case("foo:"; "Exact cookbook pattern")]
    #[test_case(":build"; "Exact recipe pattern")]
    fn test_get_recipes_for_execution_exact_matching(pattern: &str) {
        let project = get_test_project();
        let result = project.get_recipes_for_execution(Some(pattern), false);
        assert!(result.is_ok());
        assert!(!result.unwrap().is_empty());
    }

    #[test_case("foo_something:build"; "Similar cookbook name should not match")]
    #[test_case("foo:build_something"; "Similar recipe name should not match")]
    fn test_get_recipes_for_execution_exact_no_matches(pattern: &str) {
        let project = get_test_project();
        let result = project.get_recipes_for_execution(Some(pattern), false);
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test_case("^f.*:build"; "Regex cookbook pattern")]
    #[test_case("foo:^build"; "Regex recipe pattern")]
    fn test_get_recipes_for_execution_regex_patterns(pattern: &str) {
        let project = get_test_project();
        let result = project.get_recipes_for_execution(Some(pattern), true);
        assert!(result.is_ok());
        assert!(!result.unwrap().is_empty());
    }

    #[test_case("build" => matches Err(_); "Missing colon in execution")]
    #[test_case("^[invalid" => matches Err(_); "Invalid regex in execution")]
    fn test_get_recipes_for_execution_errors(
        pattern: &str,
    ) -> anyhow::Result<Vec<Vec<super::Recipe>>> {
        let project = get_test_project();
        project.get_recipes_for_execution(Some(pattern), true)
    }
}
