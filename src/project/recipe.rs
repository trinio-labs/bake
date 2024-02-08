use std::{collections::BTreeMap, io::Read, path::PathBuf};

use ignore::{overrides::OverrideBuilder, WalkBuilder};
use log::warn;
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

#[derive(Serialize)]
struct RecipeHashData {
    file_hashes: BTreeMap<PathBuf, String>,
    run: String,
}

impl Recipe {
    pub fn full_name(&self) -> String {
        format!("{}:{}", self.cookbook, self.name)
    }

    pub fn get_recipe_hash(&self) -> Result<String, String> {
        let mut walk_builder = WalkBuilder::new(self.config_path.clone());
        if let Some(inputs) = &self.inputs {
            let mut overrides_builder = OverrideBuilder::new(self.config_path.clone());
            for input in inputs {
                if let Err(err) = overrides_builder.add(input) {
                    return Err(format!(
                        "Failed to get hash for recipe {:?}. Error adding input: {:?}",
                        self.name, err
                    ));
                }
            }
            match overrides_builder.build() {
                Ok(overrides) => {
                    walk_builder.overrides(overrides);
                }
                Err(err) => {
                    return Err(format!(
                        "Failed to get hash for recipe {:?}. Error building overrides: {:?}",
                        self.name, err
                    ))
                }
            }
        };

        let walker = walk_builder.hidden(false).build();
        let mut file_hashes = BTreeMap::<PathBuf, String>::new();
        for result in walker {
            match result {
                Ok(entry) => {
                    if entry.file_type().unwrap().is_file() {
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
                            .strip_prefix(self.config_path.clone())
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

        let hash_data = RecipeHashData {
            file_hashes,
            run: self.run.clone(),
        };

        let mut hasher = blake3::Hasher::new();
        hasher.update(serde_json::to_string(&hash_data).unwrap().as_bytes());
        let hash = hasher.finalize();
        Ok(hash.to_string())
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_hash() {
        let recipe = Recipe {
            name: String::from("test"),
            cookbook: String::from("test"),
            config_path: PathBuf::from("/Users/theoribeiro/Dev/trinio/trinio/node/"),
            description: None,
            dependencies: None,
            run: String::from("test"),
            recipe_hash: String::from("test"),
            inputs: None,
            outputs: None,
            run_status: RunStatus::default(),
        };
        let hash = recipe.get_recipe_hash();
        assert!(hash.is_ok());
    }
}
