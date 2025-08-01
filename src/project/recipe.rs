use std::{collections::BTreeMap, io::Read, path::PathBuf};

use anyhow::bail;
use globset::{GlobBuilder, GlobSetBuilder};
use ignore::WalkBuilder;
use indexmap::IndexMap;
use log::{debug, warn};
use pathdiff::diff_paths;
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

#[derive(Debug, PartialOrd, Ord, Deserialize, Serialize, Clone, PartialEq, Eq, Hash, Default)]
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

    #[serde(skip)]
    pub project_root: PathBuf,

    #[serde(default)]
    pub cache: Option<RecipeCacheConfig>,

    pub description: Option<String>,

    #[serde(default)]
    pub variables: IndexMap<String, String>,

    #[serde(default)]
    pub environment: Vec<String>,

    pub dependencies: Option<Vec<String>>,
    pub run: String,

    /// Template to use for this recipe (alternative to inline definition)
    pub template: Option<String>,

    /// Parameters to pass to the template
    #[serde(default)]
    pub parameters: std::collections::BTreeMap<String, serde_yaml::Value>,

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
        let cookbook_dir = self.config_path.parent().unwrap();
        let mut file_hashes = BTreeMap::<PathBuf, String>::new();

        if let Some(cache) = &self.cache {
            if !cache.inputs.is_empty() {
                // Build globset from all input patterns
                let mut globset_builder = GlobSetBuilder::new();
                for input in &cache.inputs {
                    debug!("Adding input pattern: {input}");
                    match GlobBuilder::new(input).literal_separator(true).build() {
                        Ok(glob) => globset_builder.add(glob),
                        Err(err) => {
                            bail!(
                                "Recipe Hash ('{}'): Failed to build glob for input pattern '{}': {:?}",
                                self.full_name(),
                                input,
                                err
                            );
                        }
                    };
                }

                let globset = match globset_builder.build() {
                    Ok(globset) => globset,
                    Err(err) => {
                        bail!(
                            "Recipe Hash ('{}'): Failed to build glob set from input patterns: {:?}",
                            self.full_name(),
                            err
                        );
                    }
                };

                // Find the root directory to walk from by looking at all patterns
                let walk_root = self.find_walk_root(&cache.inputs, cookbook_dir)?;

                // Walk files from the root directory
                let mut walk_builder = WalkBuilder::new(&walk_root);
                let walker = walk_builder.hidden(false).build();

                for result in walker {
                    match result {
                        Ok(entry) => {
                            let path = entry.path();
                            if !entry.file_type().unwrap().is_file() {
                                continue;
                            }

                            // Calculate the relative path from cookbook directory for glob matching
                            let relative_from_cookbook =
                                if let Some(rel) = diff_paths(path, cookbook_dir) {
                                    rel
                                } else {
                                    // If we can't get a relative path, try stripping the cookbook prefix
                                    match path.strip_prefix(cookbook_dir) {
                                        Ok(rel) => rel.to_path_buf(),
                                        Err(_) => {
                                            // For files outside cookbook dir, use the full path for matching
                                            path.to_path_buf()
                                        }
                                    }
                                };

                            if globset.is_match(&relative_from_cookbook) {
                                debug!(
                                    "Hashing file: {path:?} (matched as {relative_from_cookbook:?})"
                                );
                                let mut hasher = blake3::Hasher::new();
                                let mut file = match std::fs::File::open(path) {
                                    Ok(file) => file,
                                    Err(err) => {
                                        warn!("Error opening file {path:?}: {err}");
                                        continue;
                                    }
                                };
                                let mut buf = Vec::new();
                                if let Err(err) = file.read_to_end(&mut buf) {
                                    warn!("Error reading file {path:?}: {err}");
                                    continue;
                                }
                                hasher.update(buf.as_slice());
                                let hash = hasher.finalize();
                                file_hashes.insert(relative_from_cookbook, hash.to_string());
                            }
                        }
                        Err(err) => {
                            warn!("Error reading file during walk: {err:?}");
                        }
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

    /// Finds the optimal root directory to walk from based on input patterns.
    /// This ensures we walk the minimum necessary directory tree.
    fn find_walk_root(
        &self,
        inputs: &[String],
        cookbook_dir: &std::path::Path,
    ) -> anyhow::Result<PathBuf> {
        let mut root_candidates = Vec::new();

        for input in inputs {
            let pattern_path = PathBuf::from(input);
            let base_dir = if pattern_path.is_absolute() {
                pattern_path.parent().unwrap_or(&pattern_path).to_path_buf()
            } else {
                // For relative patterns, resolve from cookbook directory
                let resolved = cookbook_dir.join(&pattern_path);
                resolved.parent().unwrap_or(&resolved).to_path_buf()
            };

            // Canonicalize to handle .. properly
            if let Ok(canonical) = base_dir.canonicalize() {
                root_candidates.push(canonical);
            }
        }

        // If no valid directories found, default to cookbook directory
        if root_candidates.is_empty() {
            return Ok(cookbook_dir.to_path_buf());
        }

        // Find the common ancestor of all directories
        let mut common_root = root_candidates[0].clone();
        for candidate in &root_candidates[1..] {
            common_root = find_common_ancestor(&common_root, candidate);
        }

        Ok(common_root)
    }
}

/// Finds the common ancestor directory of two paths.
fn find_common_ancestor(path1: &std::path::Path, path2: &std::path::Path) -> PathBuf {
    let components1: Vec<_> = path1.components().collect();
    let components2: Vec<_> = path2.components().collect();

    let mut common_path = PathBuf::new();
    let min_len = components1.len().min(components2.len());

    for i in 0..min_len {
        if components1[i] == components2[i] {
            common_path.push(components1[i]);
        } else {
            break;
        }
    }

    // If no common path found, return root
    if common_path.as_os_str().is_empty() {
        PathBuf::from("/")
    } else {
        common_path
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
            project_root: PathBuf::from(config_path("/valid/")),
            config_path: PathBuf::from(config_path("/valid/foo/cookbook.yml")),
            description: None,
            dependencies: None,
            environment: vec!["FOO".to_owned()],
            variables: IndexMap::new(),
            run: String::from("test"),
            cache: Some(RecipeCacheConfig {
                inputs: vec![String::from("build.sh"), String::from("../*.txt")],
                ..Default::default()
            }),
            run_status: RunStatus::default(),
        };
        std::env::set_var("FOO", "bar");
        let hash1 = recipe.get_self_hash().unwrap();

        recipe.run = "test2".to_owned();
        let hash2 = recipe.get_self_hash().unwrap();
        assert_ne!(hash1, hash2);

        recipe.cache.as_mut().unwrap().inputs.pop();
        let hash3 = recipe.get_self_hash().unwrap();

        recipe.cache.as_mut().unwrap().inputs.pop();
        let hash4 = recipe.get_self_hash().unwrap();

        recipe.variables = IndexMap::from([("FOO".to_owned(), "bar".to_owned())]);
        let hash5 = recipe.get_self_hash().unwrap();

        std::env::set_var("FOO", "not_bar");
        let hash6 = recipe.get_self_hash().unwrap();

        // All hashes should be unique
        let mut set = HashSet::new();
        assert!(set.insert(hash1));
        assert!(set.insert(hash2));
        assert!(set.insert(hash3));
        assert!(set.insert(hash4));
        assert!(set.insert(hash5));
        assert!(set.insert(hash6));
    }
}
