mod cookbook;
mod recipe;

pub use cookbook::*;
pub use recipe::*;
use regex::Regex;

use std::{
    collections::{BTreeMap, HashSet},
    path::{Path, PathBuf},
};

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct ToolConfig {
    #[serde(default = "max_parallel_default")]
    pub max_parallel: usize,

    #[serde(default)]
    pub fast_fail: bool,

    #[serde(default)]
    pub verbose: bool,
}

impl Default for ToolConfig {
    fn default() -> Self {
        Self {
            max_parallel: max_parallel_default(),
            fast_fail: true,
            verbose: false,
        }
    }
}

fn max_parallel_default() -> usize {
    std::thread::available_parallelism().unwrap().get() - 1
}

#[derive(Debug, Deserialize)]
pub struct BakeProject {
    pub name: String,

    #[serde(skip)]
    pub cookbooks: BTreeMap<String, Cookbook>,
    pub description: Option<String>,

    #[serde(default)]
    pub config: ToolConfig,

    #[serde(skip)]
    pub root_path: PathBuf,
}

impl BakeProject {
    /// Creates a bake project from a path to a bake.yml file or a directory in a bake project
    ///
    /// # Arguments
    /// * `path` - Path to either a config file or a directory. If a directory is passed,
    /// load_config will search for a bake.ya?ml file in that directory and in parent directories.
    ///
    pub fn from(path: &PathBuf) -> Result<Self, String> {
        let file_path: PathBuf;
        let mut project: Self;

        if !path.exists() {
            return Err(format!("Path does not exist: {}", path.display()));
        }

        if path.is_dir() {
            file_path = Self::find_config_file_in_dir(path)?;
        } else if path.is_file() {
            file_path = path.clone();
        } else {
            return Err("Invalid path".to_owned());
        }

        let config_str = match std::fs::read_to_string(&file_path) {
            Ok(contents) => contents,
            Err(_) => {
                return Err(format!(
                    "Could not read config file: {}",
                    file_path.display()
                ));
            }
        };

        match serde_yaml::from_str::<Self>(&config_str) {
            Ok(mut parsed) => {
                parsed.root_path = file_path.parent().unwrap().to_path_buf();
                project = parsed;
            }
            Err(err) => return Err(format!("Could not parse config file: {}", err)),
        }

        project.cookbooks = Cookbook::map_from(path)?;

        let all_recipes = project.recipes(None, None);
        // Validate if all recipe dependencies exist
        let err_msg = all_recipes.iter().fold("".to_owned(), |msg, recipe| {
            let mut missing_deps: Vec<String> = Vec::new();
            if let Some(dependencies) = recipe.dependencies.as_ref() {
                dependencies.iter().for_each(|dep| {
                    if project.get_recipe_by_name(dep).is_err() {
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
            return Err(format!(
                "{}:\n{}",
                console::style("Recipe dependencies not found").bold(),
                err_msg
            ));
        }

        // Validate if project doesn't have dependencies
        let circular_dependency = project.get_circular_dependencies();
        if !circular_dependency.is_empty() {
            let message = circular_dependency.iter().fold("".to_owned(), |acc, x| {
                format!("{}\n{}", acc, x.join(" => "))
            });
            return Err(format!("Circular dependencies detected:\n{:}", message));
        }

        Ok(project)
    }

    /// Recursively find a config file in a directory or its parent up until /
    /// or until the git repo root.
    fn find_config_file_in_dir(dir: &Path) -> Result<PathBuf, String> {
        let file_yml = dir.join("bake.yml");
        let file_yaml = dir.join("bake.yaml");

        if file_yml.exists() {
            Ok(file_yml)
        } else if file_yaml.exists() {
            return Ok(file_yaml);
        } else {
            let parent = dir.parent();

            // Stop if directory is root in the file system or in a git repository
            if let Some(parent) = parent && !dir.join(".git").is_dir() {
                return Self::find_config_file_in_dir(&PathBuf::from(parent));
            } else {
                return Err("Could not find bake.yml".to_owned());
            }
        }
    }

    /// Filter recipes by full recipe name e.g. "foo:build", "foo:" (all recipes in cookbook foo),
    /// ":build" (all recipes called build in all cookbooks)
    pub fn recipes_by_name(&self, filter: &str) -> Vec<&Recipe> {
        let re = Regex::new(r"(?P<cookbook>[\w.\-]*):(?P<recipe>[\w.\-]*)").unwrap();
        if let Some(caps) = re.captures(filter) {
            let cookbook = caps.name("cookbook").unwrap().as_str();
            let cookbook = if cookbook.is_empty() {
                None
            } else {
                Some(cookbook)
            };

            let recipe = caps.name("recipe").unwrap().as_str();
            let recipe = if recipe.is_empty() {
                None
            } else {
                Some(recipe)
            };

            return self.recipes(cookbook, recipe);
        } else {
            Vec::new()
        }
    }

    pub fn get_recipe_by_name(&self, name: &str) -> Result<&Recipe, String> {
        let recipes = self.recipes_by_name(name);
        if recipes.is_empty() {
            return Err(format!("Recipe not found: {}", name));
        }
        if recipes.len() != 1 {
            return Err(format!("Multiple recipes found: {}", name));
        }
        Ok(recipes[0])
    }

    /// Get a list of recipes given a cookbook name and/or recipe name.
    ///
    /// # Arguments
    /// * `cookbook_name` - Cookbook name
    /// * `recipe_name` - Recipe name
    ///
    /// Returns a list of recipes filtered by cookbook name and/or recipe name unless both are
    /// None, in which case all recipes are returned.
    pub fn recipes(&self, cookbook_name: Option<&str>, recipe_name: Option<&str>) -> Vec<&Recipe> {
        let mut recipes: Vec<&Recipe> = Vec::new();
        if let Some(cookbook_name) = cookbook_name {
            if let Some(cookbook) = self.cookbooks.get(cookbook_name) {
                if let Some(recipe) = cookbook.recipes.get(recipe_name.unwrap_or("")) {
                    recipes.push(recipe);
                } else {
                    recipes = cookbook.recipes.values().collect();
                }
            }
        } else if let Some(recipe_name) = recipe_name {
            recipes = self
                .cookbooks
                .iter()
                .flat_map(|(_, c)| {
                    c.recipes
                        .iter()
                        .filter_map(|(name, r)| if name == recipe_name { Some(r) } else { None })
                        .collect::<Vec<&Recipe>>()
                })
                .collect();
        } else {
            recipes = self
                .cookbooks
                .iter()
                .flat_map(|(_, c)| c.recipes.values())
                .collect();
        }
        recipes
    }

    /// Returns a list of all circular dependencies found in cookbooks
    ///
    /// If there are no circular dependencies, an empty vector is returned
    fn get_circular_dependencies(&self) -> Vec<Vec<String>> {
        struct Context<'a> {
            // recipes: &'a HashMap<String, Recipe>,
            project: &'a BakeProject,
            visited: HashSet<String>,
            cur_path: Vec<String>,
            result: Vec<Vec<String>>,
        }

        let mut ctx = Context {
            // recipes: &self.recipes(None, None),
            project: self,
            visited: HashSet::new(),
            cur_path: Vec::new(),
            result: Vec::new(),
        };

        for recipe in self.recipes(None, None) {
            if !ctx.visited.contains(&recipe.name) {
                ctx.cur_path = Vec::new();
                check_cycle(&recipe.full_name(), &mut ctx);
            }
        }

        fn check_cycle(cur_node_name: &str, ctx: &mut Context) {
            ctx.visited.insert(cur_node_name.to_string());
            ctx.cur_path.push(cur_node_name.to_string());
            if let Some(dependencies) = ctx
                .project
                .get_recipe_by_name(cur_node_name)
                .unwrap()
                .dependencies
                .as_ref()
            {
                dependencies.iter().for_each(|dep_name| {
                    if ctx.cur_path.contains(dep_name) {
                        let mut path = ctx.cur_path.clone();
                        path.push(dep_name.to_string());
                        ctx.result.push(path);
                    }
                    if !ctx.visited.contains(dep_name) {
                        check_cycle(dep_name, ctx);
                    }
                })
            }
        }

        ctx.result
    }
}

#[cfg(test)]
mod tests {
    use std::{os::unix::prelude::PermissionsExt, path::PathBuf};

    use test_case::test_case;

    fn config_path(path_str: &str) -> String {
        env!("CARGO_MANIFEST_DIR").to_owned() + "/resources/tests" + path_str
    }

    fn validate_project(project: Result<super::BakeProject, String>) {
        let project = project.unwrap();
        assert_eq!(project.name, "test");
        // assert_eq!(project.recipes.len(), 5);
        // assert_eq!(project.recipes["foo:build"].name, "build");
    }

    #[test]
    fn get_circular_dependencies() {
        let project = super::BakeProject::from(&PathBuf::from(config_path("/invalid/circular")));

        assert!(project.unwrap_err().contains("Circular dependencies"));
    }

    #[test_case(config_path("/valid/foo") => using validate_project; "Valid subdir")]
    #[test_case(config_path("/valid") => using validate_project; "Root dir")]
    #[test_case(config_path("/valid/bake.yml") => using validate_project; "Existing file")]
    #[test_case(config_path("/invalid/asdf") => matches Err(_); "Invalid subdir")]
    #[test_case(config_path("/invalid/circular") => matches Err(_); "Circular dependencies")]
    #[test_case(config_path("/invalid/recipes") => matches Err(_); "Inexistent recipes")]
    #[test_case(config_path("/invalid/config") => matches Err(_); "Invalid config")]
    #[test_case(config_path("/invalid/nobake/internal") => matches Err(_); "No bake file with .git root")]
    fn read_config(path_str: String) -> Result<super::BakeProject, String> {
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

    #[test]
    fn recipes_by_name() {
        let project = super::BakeProject::from(&PathBuf::from(config_path("/valid/"))).unwrap();

        // Should return empty when not specifying format "<cookbook>:<recipe>"
        let recipes = project.recipes_by_name("foo");
        assert_eq!(recipes.len(), 0);

        let recipes = project.recipes_by_name("foo:build");
        assert_eq!(recipes.len(), 1);
        assert_eq!(recipes[0].name, "build");

        let recipes = project.recipes_by_name("foo:");
        assert_eq!(recipes.len(), 2);

        let recipes = project.recipes_by_name(":build");
        assert_eq!(recipes.len(), 2);
    }
}
