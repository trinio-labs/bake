use std::{collections::BTreeMap, io::Read, path::PathBuf};

use anyhow::bail;
use globset::{GlobBuilder, GlobSetBuilder};
use ignore::WalkBuilder;
use indexmap::IndexMap;
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

#[derive(Debug, PartialOrd, Ord, Deserialize, Clone, PartialEq, Eq, Hash, Default)]
pub struct RecipeCacheConfig {
    #[serde(default)]
    pub inputs: Vec<String>,

    #[serde(default)]
    pub outputs: Vec<String>,
}

#[derive(Debug, Deserialize, Clone, PartialEq, Eq, Default)]
pub struct Recipe {
    #[serde(skip)]
    pub name: String,

    #[serde(skip)]
    pub cookbook: String,

    #[serde(skip)]
    pub config_path: PathBuf,

    #[serde(default)]
    pub cache: Option<RecipeCacheConfig>,

    pub description: Option<String>,

    #[serde(default)]
    pub variables: IndexMap<String, String>,

    #[serde(default)]
    pub environment: Vec<String>,

    pub dependencies: Option<Vec<String>>,
    pub run: String,

    #[serde(skip)]
    pub run_status: RunStatus,
}

#[derive(Serialize, Debug)]
struct RecipeHashData {
    environment: BTreeMap<String, String>,
    file_hashes: BTreeMap<PathBuf, String>,
    run: String,
    variables: BTreeMap<String, String>,
}

impl Recipe {
    pub fn full_name(&self) -> String {
        format!("{}:{}", self.cookbook, self.name)
    }

    /// Gets the hash of the recipe's intrinsic properties (command, vars, env, inputs).
    pub fn get_self_hash(&self) -> anyhow::Result<String> {
        debug!("Getting hash for recipe: {}", self.name);
        let mut walk_builder = WalkBuilder::new(self.config_path.clone().parent().unwrap());
        let mut globset_builder = GlobSetBuilder::new();
        let mut file_hashes = BTreeMap::<PathBuf, String>::new();

        if let Some(cache) = &self.cache {
            for input in &cache.inputs {
                debug!("Adding input: {input}");
                match GlobBuilder::new(input).literal_separator(true).build() {
                    Ok(glob) => globset_builder.add(glob),
                    Err(err) => {
                        bail!(
                            "Failed to get hash for recipe {:?}. Error adding input: {:?}",
                            self.name,
                            err
                        );
                    }
                };
            }

            let globset = match globset_builder.build() {
                Ok(globset) => globset,
                Err(err) => {
                    bail!(
                        "Failed to get hash for recipe {:?}. Error building globset: {:?}",
                        self.name,
                        err
                    );
                }
            };

            // Hash all input files
            let walker = walk_builder.hidden(false).build();
            for result in walker {
                match result {
                    Ok(entry) => {
                        let path = entry.path();
                        let relative_path = path
                            .strip_prefix(self.config_path.clone().parent().unwrap())
                            .unwrap()
                            .to_path_buf();
                        if entry.file_type().unwrap().is_file() && globset.is_match(&relative_path)
                        {
                            debug!("Hashing file: {:?}", entry.path());
                            let mut hasher = blake3::Hasher::new();
                            let mut file = std::fs::File::open(path).unwrap();
                            let mut buf = Vec::new();
                            if let Err(err) = file.read_to_end(&mut buf) {
                                warn!("Error reading file: {err:?}");
                            }
                            hasher.update(buf.as_slice());
                            let hash = hasher.finalize();
                            file_hashes.insert(relative_path, hash.to_string());
                        }
                    }
                    Err(err) => {
                        warn!("Error reading file: {err:?}");
                    }
                }
            }
        }

        // Add environment variables
        let environment = self
            .environment
            .iter()
            .map(|env| (env.clone(), std::env::var(env).unwrap_or_default()))
            .collect::<BTreeMap<String, String>>();

        // We need to sort the hashes so that the hash is always the same independently of the order which they are declared
        let variables = BTreeMap::from_iter(self.variables.clone());

        // Create hash data structure and hash it
        let hash_data = RecipeHashData {
            file_hashes,
            environment,
            variables,
            run: self.run.clone(),
        };

        debug!("Hash data: {hash_data:?}");

        let mut hasher = blake3::Hasher::new();
        hasher.update(serde_json::to_string(&hash_data).unwrap().as_bytes());
        let hash = hasher.finalize();
        Ok(hash.to_string())
    }
}

#[cfg(test)]
mod tests {

    use std::collections::HashSet;

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
            environment: vec!["FOO".to_owned()],
            variables: IndexMap::new(),
            run: String::from("test"),
            cache: Some(RecipeCacheConfig {
                inputs: vec![String::from("build.sh")],
                ..Default::default()
            }),
            run_status: RunStatus::default(),
        };
        std::env::set_var("FOO", "bar");
        let hash1 = recipe.get_self_hash().unwrap();

        recipe.run = "test2".to_owned();
        let hash2 = recipe.get_self_hash().unwrap();
        assert_ne!(hash1, hash2);

        recipe.cache.as_mut().unwrap().inputs = vec![];
        let hash3 = recipe.get_self_hash().unwrap();

        recipe.variables = IndexMap::from([("FOO".to_owned(), "bar".to_owned())]);
        let hash4 = recipe.get_self_hash().unwrap();

        std::env::set_var("FOO", "not_bar");
        let hash5 = recipe.get_self_hash().unwrap();

        // All hashes should be unique
        let mut set = HashSet::new();
        assert!(set.insert(hash1));
        assert!(set.insert(hash2));
        assert!(set.insert(hash3));
        assert!(set.insert(hash4));
        assert!(set.insert(hash5));
    }
}
