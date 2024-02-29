use std::{collections::BTreeMap, path::PathBuf};

use crate::{
    project::Recipe,
    template::{parse_template, parse_variable_list},
};
use anyhow::bail;
use ignore::WalkBuilder;
use indexmap::IndexMap;
use log::debug;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Cookbook {
    pub name: String,

    #[serde(default)]
    pub environment: Vec<String>,

    #[serde(default)]
    pub variables: IndexMap<String, String>,

    pub recipes: BTreeMap<String, Recipe>,

    #[serde(skip)]
    pub config_path: PathBuf,
}
impl Cookbook {
    /// Creates a cookbook config from a path to a cookbook file
    ///
    /// # Arguments
    /// * `path` - Path to a cookbook file
    ///
    pub fn from(
        path: &PathBuf,
        project_environment: &[String],
        project_variables: &IndexMap<String, String>,
        project_constants: &IndexMap<String, String>,
        override_variables: &IndexMap<String, String>,
    ) -> anyhow::Result<Self> {
        let config: Cookbook;

        let config_str = match std::fs::read_to_string(path) {
            Ok(contents) => contents,
            Err(_) => bail!("Could not read config file: {}", path.display()),
        };

        match serde_yaml::from_str::<Self>(&config_str) {
            Ok(mut parsed) => {
                parsed.config_path = path.to_path_buf();

                // Inherit environment and variables from project
                let mut cookbook_environment = project_environment.to_owned();
                cookbook_environment.extend(parsed.environment.iter().cloned());
                parsed.environment = cookbook_environment;

                let mut cookbook_variables = project_variables.clone();
                cookbook_variables.extend(parsed.variables.clone());

                let mut cookbook_constants =
                    IndexMap::from([("project".to_owned(), project_constants.clone())]);
                cookbook_constants.insert(
                    "cookbook".to_owned(),
                    IndexMap::from([(
                        "root".to_owned(),
                        path.parent().unwrap().display().to_string(),
                    )]),
                );

                parsed.variables = parse_variable_list(
                    &parsed.environment,
                    &cookbook_variables,
                    &cookbook_constants,
                    override_variables,
                )?;

                parsed.recipes.iter_mut().try_for_each(|(name, recipe)| {
                    recipe.name = name.clone();
                    recipe.cookbook = parsed.name.clone();
                    recipe.config_path = path.to_path_buf();

                    // Inherit environment and variables from cookbook
                    let mut recipe_environment = parsed.environment.clone();
                    recipe_environment.extend(recipe.environment.iter().cloned());
                    recipe.environment = recipe_environment;

                    let mut recipe_variables = parsed.variables.clone();
                    recipe_variables.extend(recipe.variables.clone());
                    if let Ok(variables) = parse_variable_list(
                        recipe.environment.as_slice(),
                        &recipe_variables,
                        &cookbook_constants,
                        override_variables,
                    ) {
                        recipe.variables = variables;
                    } else {
                        bail!("Could not parse recipe variables: {}", recipe.name)
                    }

                    recipe.run = parse_template(
                        &recipe.run,
                        &recipe.environment,
                        &recipe.variables,
                        &cookbook_constants,
                    )?;

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

                    Ok(())
                })?;
                config = parsed;
            }
            Err(err) => bail!("Could not parse cookbook file: {}", err),
        }

        Ok(config)
    }

    /// Gets all cookbooks recursively in a directory
    ///
    /// map_from recursively searches for all cookbooks in a directory respecting `.gitignore` and
    /// `.bakeignore` files
    ///
    /// # Arguments
    /// * `path` - Path to a directory
    ///
    pub fn map_from(
        path: &PathBuf,
        project_environment: &[String],
        project_variables: &IndexMap<String, String>,
        project_constants: &IndexMap<String, String>,
        override_variables: &IndexMap<String, String>,
    ) -> anyhow::Result<BTreeMap<String, Self>> {
        let all_files = WalkBuilder::new(path)
            .add_custom_ignore_filename(".bakeignore")
            .build();
        all_files
            .filter_map(|x| match x {
                Ok(file) => {
                    let filename = file.file_name().to_str().unwrap();
                    if filename.contains("cookbook.yaml") || filename.contains("cookbook.yml") {
                        match Self::from(
                            &file.into_path(),
                            project_environment,
                            project_variables,
                            project_constants,
                            override_variables,
                        ) {
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

    use indexmap::IndexMap;
    use test_case::test_case;

    fn config_path(path_str: &str) -> String {
        env!("CARGO_MANIFEST_DIR").to_owned() + "/resources/tests" + path_str
    }

    fn validate_cookbook_foo(actual: anyhow::Result<super::Cookbook>) {
        assert_eq!(actual.as_ref().unwrap().name, "foo");
    }

    fn validate_cookbook_vec(actual: anyhow::Result<BTreeMap<String, super::Cookbook>>) {
        assert_eq!(actual.unwrap().len(), 2)
    }

    #[test_case(config_path("/valid/foo/cookbook.yml") => using validate_cookbook_foo; "Valid cookbook file")]
    #[test_case(config_path("/invalid/config/cookbook.yml") => matches Err(_); "Invalid cookbook file")]
    #[test_case(config_path("/invalid/config") => matches Err(_); "Cant read directory")]
    fn read_cookbook(path_str: String) -> anyhow::Result<super::Cookbook> {
        super::Cookbook::from(
            &PathBuf::from(path_str),
            &[],
            &IndexMap::new(),
            &IndexMap::new(),
            &IndexMap::new(),
        )
    }

    #[test_case(config_path("/valid/") => using validate_cookbook_vec; "Root dir")]
    #[test_case(config_path("/invalid/config") => matches Err(_); "Invalid dir")]
    fn read_all_cookbooks(path_str: String) -> anyhow::Result<BTreeMap<String, super::Cookbook>> {
        super::Cookbook::map_from(
            &PathBuf::from(path_str),
            &[],
            &IndexMap::new(),
            &IndexMap::new(),
            &IndexMap::new(),
        )
    }
}
