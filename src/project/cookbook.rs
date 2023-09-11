use std::{collections::BTreeMap, path::PathBuf};

use crate::project::Recipe;
use ignore::WalkBuilder;
use log::debug;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Cookbook {
    pub name: String,
    pub variables: Option<Vec<String>>,
    pub recipes: BTreeMap<String, Recipe>,

    #[serde(skip)]
    pub config_path: PathBuf,
}
impl Cookbook {
    /// Creates a cookbook config from a path to a cookbook file or directory
    ///
    /// # Arguments
    /// * `path` - Path to a cookbook file or directory containing a cookbook.ya?ml file
    ///
    pub fn from(path: &PathBuf) -> Result<Self, String> {
        let config: Cookbook;

        let config_str = match std::fs::read_to_string(path) {
            Ok(contents) => contents,
            Err(_) => return Err(format!("Could not read config file: {}", path.display())),
        };

        match serde_yaml::from_str::<Self>(&config_str) {
            Ok(mut parsed) => {
                parsed.config_path = path.to_path_buf();
                parsed.recipes.iter_mut().for_each(|(name, recipe)| {
                    recipe.name = name.clone();
                    recipe.cookbook = parsed.name.clone();
                    recipe.config_path = path.to_path_buf();
                    if let Some(dependencies) = recipe.dependencies.as_ref() {
                        let new_deps = dependencies.iter().map(|dep| {
                            if !dep.contains(':') {
                                recipe.cookbook.clone() + ":" + dep
                            } else {
                                dep.clone()
                            }
                        });
                        recipe.dependencies = Some(new_deps.collect());
                    }
                });
                config = parsed;
            }
            Err(err) => return Err(format!("Could not parse cookbook file: {}", err)),
        }

        Ok(config)
    }

    /// Gets all cookbooks recursively in a directory
    ///
    /// map_from recursively searches for all cookbooks in a directory respecting .gitignore and
    /// .bakeignore files
    ///
    /// # Arguments
    /// * `path` - Path to a directory
    ///
    pub fn map_from(path: &PathBuf) -> Result<BTreeMap<String, Self>, String> {
        let all_files = WalkBuilder::new(path)
            .add_custom_ignore_filename(".bakeignore")
            .build();
        all_files
            .filter_map(|x| match x {
                Ok(file) => {
                    let filename = file.file_name().to_str().unwrap();
                    if filename.contains("cookbook.yaml") || filename.contains("cookbook.yml") {
                        match Self::from(&file.into_path()) {
                            Ok(cookbook) => Some(Ok((cookbook.name.clone(), cookbook))),
                            Err(err) => Some(Err(err)),
                        }
                    } else {
                        None
                    }
                }
                Err(_) => {
                    debug!("Ignored file: {}", x.unwrap_err());
                    None
                }
            })
            .collect()
    }
}
#[cfg(test)]
mod test {
    use std::{collections::BTreeMap, path::PathBuf};

    use test_case::test_case;

    fn config_path(path_str: &str) -> String {
        env!("CARGO_MANIFEST_DIR").to_owned() + "/resources/tests" + path_str
    }

    fn validate_cookbook_foo(actual: Result<super::Cookbook, String>) {
        assert_eq!(actual.unwrap().name, "foo")
    }

    fn validate_cookbook_vec(actual: Result<BTreeMap<String, super::Cookbook>, String>) {
        assert_eq!(actual.unwrap().len(), 2)
    }

    #[test_case(config_path("/valid/foo/cookbook.yml") => using validate_cookbook_foo; "Valid cookbook file")]
    #[test_case(config_path("/invalid/config/cookbook.yml") => matches Err(_); "Invalid cookbook file")]
    #[test_case(config_path("/invalid/config") => matches Err(_); "Cant read directory")]
    fn read_cookbook(path_str: String) -> Result<super::Cookbook, String> {
        super::Cookbook::from(&PathBuf::from(path_str))
    }

    #[test_case(config_path("/valid/") => using validate_cookbook_vec; "Root dir")]
    #[test_case(config_path("/invalid/config") => matches Err(_); "Invalid dir")]
    fn read_all_cookbooks(path_str: String) -> Result<BTreeMap<String, super::Cookbook>, String> {
        super::Cookbook::map_from(&PathBuf::from(path_str))
    }
}
