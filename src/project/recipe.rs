use std::{collections::BTreeMap, io::Read, path::PathBuf};

use anyhow::bail;
use ignore::{overrides::OverrideBuilder, WalkBuilder};
use log::{debug, warn};
use serde::{Deserialize, Serialize};

#[derive(Debug, PartialOrd, Ord, Deserialize, Clone, PartialEq, Eq, Hash, Default)]
pub enum Status {
    Done,
    Error,
    #[default]
    Idle,
    Running,
}

#[derive(Debug, PartialOrd, Ord, Deserialize, Clone, PartialEq, Eq, Hash, Default)]
pub struct RunStatus {
    pub status: Status,
    pub output: String,
}

#[derive(Debug, PartialOrd, Ord, Deserialize, Clone, PartialEq, Eq, Hash)]
pub struct Recipe {
    #[serde(skip)]
    pub name: String,

    #[serde(skip)]
    pub cookbook: String,

    #[serde(skip)]
    pub config_path: PathBuf,
    pub description: Option<String>,
    pub dependencies: Option<Vec<String>>,
    pub run: String,
    pub inputs: Option<Vec<String>>,
    pub outputs: Option<Vec<String>>,

    #[serde(skip)]
    pub recipe_hash: String,

    #[serde(skip)]
    pub run_status: RunStatus,
}

#[derive(Serialize, Debug)]
struct RecipeHashData {
    file_hashes: BTreeMap<PathBuf, String>,
    run: String,
}

impl Recipe {
    pub fn full_name(&self) -> String {
        format!("{}:{}", self.cookbook, self.name)
    }

    /// Gets the hash of the recipes fields, not including its dependencies
    pub fn get_recipe_hash(&self) -> anyhow::Result<String> {
        debug!("Getting hash for recipe: {}", self.name);
        let mut walk_builder = WalkBuilder::new(self.config_path.clone().parent().unwrap());
        let mut overrides_builder =
            OverrideBuilder::new(self.config_path.clone().parent().unwrap());

        // Add an ignore all rule so that only globs listed as inputs are hashed
        if let Err(err) = overrides_builder.add("!**/*") {
            bail!(
                "Failed to get hash for recipe {:?}. Error adding default ignore: {:?}",
                self.name,
                err
            );
        }

        // For each input, add it to the overrides list
        if let Some(inputs) = &self.inputs {
            for input in inputs {
                debug!("Adding input: {}", input);
                if let Err(err) = overrides_builder.add(input) {
                    bail!(
                        "Failed to get hash for recipe {:?}. Error adding input: {:?}",
                        self.name,
                        err
                    );
                }
            }
        };

        match overrides_builder.build() {
            Ok(overrides) => {
                debug!("Num ignores: {}", overrides.num_ignores());
                walk_builder.overrides(overrides);
            }
            Err(err) => {
                bail!(
                    "Failed to get hash for recipe {:?}. Error building overrides: {:?}",
                    self.name,
                    err
                )
            }
        }

        // Hash all input files
        let walker = walk_builder.hidden(false).build();
        let mut file_hashes = BTreeMap::<PathBuf, String>::new();
        for result in walker {
            match result {
                Ok(entry) => {
                    if entry.file_type().unwrap().is_file() {
                        debug!("Hashing file: {:?}", entry.path());
                        let path = entry.into_path();
                        let mut hasher = blake3::Hasher::new();
                        let mut file = std::fs::File::open(&path).unwrap();
                        let mut buf = Vec::new();
                        if let Err(err) = file.read_to_end(&mut buf) {
                            warn!("Error reading file: {:?}", err);
                        }
                        hasher.update(buf.as_slice());
                        let hash = hasher.finalize();
                        let relative_path = path
                            .strip_prefix(self.config_path.clone().parent().unwrap())
                            .unwrap()
                            .to_path_buf();
                        file_hashes.insert(relative_path, hash.to_string());
                    }
                }
                Err(err) => {
                    warn!("Error reading file: {:?}", err);
                }
            }
        }

        // Create hash data structure and hash it
        let hash_data = RecipeHashData {
            file_hashes,
            run: self.run.clone(),
        };

        debug!("Hash data: {:?}", hash_data);

        let mut hasher = blake3::Hasher::new();
        hasher.update(serde_json::to_string(&hash_data).unwrap().as_bytes());
        let hash = hasher.finalize();
        Ok(hash.to_string())
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    fn config_path(path_str: &str) -> String {
        env!("CARGO_MANIFEST_DIR").to_owned() + "/resources/tests" + path_str
    }

    #[test]
    fn test_hash() {
        let mut recipe = Recipe {
            name: String::from("test"),
            cookbook: String::from("test"),
            config_path: PathBuf::from(config_path("/valid/foo/bake.yml")),
            description: None,
            dependencies: None,
            run: String::from("test"),
            recipe_hash: String::from("test"),
            inputs: Some(vec![String::from("build.sh")]),
            outputs: None,
            run_status: RunStatus::default(),
        };
        let hash1 = recipe.get_recipe_hash().unwrap();

        recipe.run = "test2".to_owned();
        let hash2 = recipe.get_recipe_hash().unwrap();
        assert_ne!(hash1, hash2);

        recipe.inputs = None;
        let hash3 = recipe.get_recipe_hash().unwrap();
        assert_ne!(hash1, hash3);
        assert_ne!(hash2, hash3);
    }
}
