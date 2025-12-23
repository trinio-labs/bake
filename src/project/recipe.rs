use std::{
    collections::BTreeMap,
    io::Read,
    path::{Path, PathBuf},
};

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

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq, Default)]
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

    /// Tags for filtering recipes (e.g., "frontend", "backend", "api")
    #[serde(default)]
    pub tags: Vec<String>,

    /// Recipe-level variables
    #[serde(default)]
    pub variables: IndexMap<String, serde_yaml::Value>,

    /// Environment-specific variable overrides for this recipe
    #[serde(default)]
    pub overrides: BTreeMap<String, IndexMap<String, serde_yaml::Value>>,

    /// Processed variables for runtime use (combines variables + overrides)
    #[serde(skip)]
    pub processed_variables: IndexMap<String, serde_yaml::Value>,

    #[serde(default)]
    pub environment: Vec<String>,

    pub dependencies: Option<Vec<String>>,
    #[serde(default)]
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
    tags: Vec<String>,
    variables: BTreeMap<String, String>,
}

impl Recipe {
    pub fn full_name(&self) -> String {
        format!("{}:{}", self.cookbook, self.name)
    }

    /// Gets the hash of the recipe's intrinsic properties (command, vars, env, inputs).
    pub fn get_self_hash(&self) -> anyhow::Result<String> {
        debug!("Getting hash for recipe: {}", self.name);
        let cookbook_dir = self
            .config_path
            .parent()
            .unwrap()
            .canonicalize()
            .unwrap_or_else(|_| self.config_path.parent().unwrap().to_path_buf());
        let mut file_hashes = BTreeMap::<PathBuf, String>::new();

        if let Some(cache) = &self.cache {
            if !cache.inputs.is_empty() {
                // Group patterns by their walk roots (performance optimization)
                let pattern_groups =
                    self.group_patterns_by_walk_root(&cache.inputs, &cookbook_dir)?;

                // Process each group with simplified canonical path logic
                for (walk_root, patterns) in pattern_groups {
                    debug!("Walking from {walk_root:?} for patterns: {patterns:?}");
                    self.walk_and_hash(&walk_root, &patterns, &cookbook_dir, &mut file_hashes)?;
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
        // Convert YAML variables to strings for hashing
        let variables: BTreeMap<String, String> = self
            .variables
            .iter()
            .map(|(k, v)| {
                let string_value = match v {
                    serde_yaml::Value::String(s) => s.clone(),
                    _ => serde_yaml::to_string(v)
                        .unwrap_or_else(|_| "null".to_string())
                        .trim()
                        .to_string(),
                };
                (k.clone(), string_value)
            })
            .collect();

        // Create hash data structure and hash it
        let hash_data = RecipeHashData {
            file_hashes,
            environment,
            variables,
            tags: self.tags.clone(),
            run: self.run.clone(),
        };

        debug!("Hash data: {hash_data:?}");

        let mut hasher = blake3::Hasher::new();
        hasher.update(serde_json::to_string(&hash_data).unwrap().as_bytes());
        let hash = hasher.finalize();
        Ok(hash.to_string())
    }

    /// Walks files and hashes them using simplified canonical path logic.
    fn walk_and_hash(
        &self,
        walk_root: &Path,
        patterns: &[String],
        cookbook_dir: &Path,
        file_hashes: &mut BTreeMap<PathBuf, String>,
    ) -> anyhow::Result<()> {
        // Convert patterns to canonical absolute patterns for matching
        let canonical_patterns = self.resolve_patterns_to_canonical(patterns, cookbook_dir)?;

        // Build globset for matching
        let mut globset_builder = GlobSetBuilder::new();
        for pattern in &canonical_patterns {
            debug!("Adding canonical pattern: {pattern:?}");
            let pattern_str = pattern.to_string_lossy();
            match GlobBuilder::new(&pattern_str)
                .literal_separator(true)
                .build()
            {
                Ok(glob) => globset_builder.add(glob),
                Err(err) => {
                    bail!(
                        "Recipe Hash ('{}'): Failed to build glob for canonical pattern '{}': {:?}",
                        self.full_name(),
                        pattern_str,
                        err
                    );
                }
            };
        }

        let globset = globset_builder.build().map_err(|err| {
            anyhow::anyhow!(
                "Recipe Hash ('{}'): Failed to build glob set: {:?}",
                self.full_name(),
                err
            )
        })?;

        // Walk files from the root
        let mut walk_builder = WalkBuilder::new(walk_root);
        let walker = walk_builder.hidden(false).build();

        for result in walker {
            match result {
                Ok(entry) => {
                    let path = entry.path();
                    if !entry.file_type().unwrap().is_file() {
                        continue;
                    }

                    // Convert file path to canonical for matching
                    let canonical_file = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());

                    if globset.is_match(&canonical_file) {
                        // Calculate relative path from cookbook_dir for consistent hash keys
                        let relative_key =
                            if let Ok(rel) = canonical_file.strip_prefix(cookbook_dir) {
                                rel.to_path_buf()
                            } else if let Some(rel) = diff_paths(&canonical_file, cookbook_dir) {
                                rel
                            } else {
                                canonical_file.clone()
                            };

                        debug!("Hashing file: {canonical_file:?} (key: {relative_key:?})");

                        let mut hasher = blake3::Hasher::new();
                        let mut file = match std::fs::File::open(&canonical_file) {
                            Ok(file) => file,
                            Err(err) => {
                                warn!("Error opening file {canonical_file:?}: {err}");
                                continue;
                            }
                        };
                        let mut buf = Vec::new();
                        if let Err(err) = file.read_to_end(&mut buf) {
                            warn!("Error reading file {canonical_file:?}: {err}");
                            continue;
                        }
                        hasher.update(buf.as_slice());
                        let hash = hasher.finalize();
                        file_hashes.insert(relative_key, hash.to_string());
                    }
                }
                Err(err) => {
                    warn!("Error reading file during walk: {err:?}");
                }
            }
        }

        Ok(())
    }

    /// Converts input patterns to canonical absolute patterns for consistent matching.
    fn resolve_patterns_to_canonical(
        &self,
        patterns: &[String],
        cookbook_dir: &Path,
    ) -> anyhow::Result<Vec<PathBuf>> {
        let mut canonical_patterns = Vec::new();

        for pattern in patterns {
            // Strip surrounding quotes (both single and double) that might come from
            // YAML serialization, especially when helpers return quoted patterns
            let trimmed = pattern.trim();
            let unquoted = if (trimmed.starts_with('\'') && trimmed.ends_with('\''))
                || (trimmed.starts_with('"') && trimmed.ends_with('"'))
            {
                &trimmed[1..trimmed.len() - 1]
            } else {
                trimmed
            };

            let pattern_path = PathBuf::from(unquoted);
            let resolved_pattern = if pattern_path.is_absolute() {
                pattern_path
            } else {
                cookbook_dir.join(&pattern_path)
            };

            // For glob patterns, we need to resolve the non-glob part
            let canonical_pattern = if pattern.contains('*') || pattern.contains('?') {
                // Find the first glob character
                let pattern_str = resolved_pattern.to_string_lossy();
                if let Some(glob_pos) = pattern_str.find(['*', '?']) {
                    // Split at the last directory separator before the glob
                    let pre_glob = &pattern_str[..glob_pos];
                    if let Some(dir_pos) = pre_glob.rfind('/') {
                        let dir_part = &pattern_str[..dir_pos];
                        let glob_part = &pattern_str[dir_pos..];

                        // Canonicalize the directory part, keep the glob part
                        let canonical_dir = PathBuf::from(dir_part)
                            .canonicalize()
                            .unwrap_or_else(|_| PathBuf::from(dir_part));
                        PathBuf::from(format!("{}{}", canonical_dir.to_string_lossy(), glob_part))
                    } else {
                        resolved_pattern
                    }
                } else {
                    resolved_pattern
                }
            } else {
                // Non-glob pattern, canonicalize if it exists
                resolved_pattern.canonicalize().unwrap_or(resolved_pattern)
            };

            canonical_patterns.push(canonical_pattern);
        }

        Ok(canonical_patterns)
    }

    /// Groups input patterns by their optimal walk root directories.
    /// This allows us to run separate walks for patterns with different roots.
    fn group_patterns_by_walk_root(
        &self,
        inputs: &[String],
        cookbook_dir: &Path,
    ) -> anyhow::Result<Vec<(PathBuf, Vec<String>)>> {
        use std::collections::HashMap;

        let mut groups: HashMap<PathBuf, Vec<String>> = HashMap::new();

        for input in inputs {
            let walk_root = self.find_walk_root_for_pattern(input, cookbook_dir);
            groups.entry(walk_root).or_default().push(input.clone());
        }

        Ok(groups.into_iter().collect())
    }

    /// Finds the optimal walk root for a given pattern.
    fn find_walk_root_for_pattern(&self, pattern: &str, cookbook_dir: &Path) -> PathBuf {
        let pattern_path = PathBuf::from(pattern);

        let resolved = if pattern_path.is_absolute() {
            pattern_path
        } else {
            cookbook_dir.join(&pattern_path)
        };

        // Find the first existing parent directory for optimal walking
        let mut current = resolved.as_path();
        while let Some(parent) = current.parent() {
            if parent.exists() {
                return parent
                    .canonicalize()
                    .unwrap_or_else(|_| parent.to_path_buf());
            }
            current = parent;
        }

        // Fallback to cookbook directory
        cookbook_dir
            .canonicalize()
            .unwrap_or_else(|_| cookbook_dir.to_path_buf())
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
            tags: vec![],
            dependencies: None,
            environment: vec!["FOO".to_owned()],
            variables: IndexMap::new(),
            overrides: BTreeMap::new(),
            processed_variables: IndexMap::new(),
            run: String::from("test"),
            cache: Some(RecipeCacheConfig {
                inputs: vec![String::from("build.sh"), String::from("../*.txt")],
                ..Default::default()
            }),
            template: None,
            parameters: std::collections::BTreeMap::new(),
            run_status: RunStatus::default(),
        };
        // SAFETY: Test code - environment variable used only in this test
        unsafe { std::env::set_var("FOO", "bar") };
        let hash1 = recipe.get_self_hash().unwrap();

        recipe.run = "test2".to_owned();
        let hash2 = recipe.get_self_hash().unwrap();
        assert_ne!(hash1, hash2);

        recipe.cache.as_mut().unwrap().inputs.pop();
        let hash3 = recipe.get_self_hash().unwrap();

        recipe.cache.as_mut().unwrap().inputs.pop();
        let hash4 = recipe.get_self_hash().unwrap();

        recipe.variables = IndexMap::from([(
            "FOO".to_owned(),
            serde_yaml::Value::String("bar".to_owned()),
        )]);
        let hash5 = recipe.get_self_hash().unwrap();

        // SAFETY: Test code - environment variable used only in this test
        unsafe { std::env::set_var("FOO", "not_bar") };
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

    #[test]
    fn test_cache_inputs_with_quotes() {
        // Test that cache inputs handle quoted glob patterns correctly by stripping quotes
        // when resolving patterns for glob matching

        let yaml_normal = r#"
inputs:
  - "**/*.rs"
  - "normal/**/*.txt"
"#;

        let cache_normal: RecipeCacheConfig = serde_yaml::from_str(yaml_normal).unwrap();

        // Verify normal parsing
        assert_eq!(cache_normal.inputs.len(), 2);
        assert_eq!(cache_normal.inputs[0], "**/*.rs");
        assert_eq!(cache_normal.inputs[1], "normal/**/*.txt");

        // Test that YAML's triple-quote syntax (which it uses when re-serializing strings with quotes)
        // parses to the string WITH quotes, but glob matching should strip them
        let yaml_triple = r#"
inputs:
  - '''**/*.rs'''
  - '"src/**/*.txt"'
"#;
        let cache_triple: RecipeCacheConfig = serde_yaml::from_str(yaml_triple).unwrap();

        // YAML parses these WITH quotes (as literal characters in the string)
        assert_eq!(cache_triple.inputs[0], "'**/*.rs'");
        assert_eq!(cache_triple.inputs[1], "\"src/**/*.txt\"");

        // But when used for glob matching, resolve_patterns_to_canonical will strip the quotes
        // so both '**/*.rs' and **/*.rs will work as the same glob pattern
    }

    #[test]
    fn test_relative_glob_matching() {
        // Create a recipe with relative patterns to test glob matching
        let recipe = Recipe {
            name: String::from("test"),
            cookbook: String::from("test"),
            project_root: PathBuf::from(config_path("/valid/")),
            config_path: PathBuf::from(config_path("/valid/foo/cookbook.yml")),
            description: None,
            tags: vec![],
            dependencies: None,
            environment: vec![],
            variables: IndexMap::new(),
            overrides: BTreeMap::new(),
            processed_variables: IndexMap::new(),
            run: String::from("test"),
            cache: Some(RecipeCacheConfig {
                inputs: vec![
                    String::from("build.sh"), // should match foo/build.sh
                    String::from("../*.txt"), // should match valid/dependency.txt and foo/not_a_dependency.txt
                ],
                ..Default::default()
            }),
            template: None,
            parameters: std::collections::BTreeMap::new(),
            run_status: RunStatus::default(),
        };

        // This should not panic and should find the files
        let result = recipe.get_self_hash();
        assert!(
            result.is_ok(),
            "Hash calculation should succeed: {result:?}"
        );

        // The hash should be reproducible
        let hash1 = recipe.get_self_hash().unwrap();
        let hash2 = recipe.get_self_hash().unwrap();
        assert_eq!(hash1, hash2, "Hash should be reproducible");
    }

    #[test]
    fn test_complex_relative_path_matching() {
        // Test the complex multi-level relative path pattern like quasar project
        let recipe = Recipe {
            name: String::from("complex-build"),
            cookbook: String::from("complex-paths"),
            project_root: PathBuf::from(config_path("/valid/")),
            config_path: PathBuf::from(config_path("/valid/nested/apps/gateway/cmd/cookbook.yml")),
            description: None,
            tags: vec![],
            dependencies: None,
            environment: vec![],
            variables: IndexMap::new(),
            overrides: BTreeMap::new(),
            processed_variables: IndexMap::new(),
            run: String::from("echo 'test'"),
            cache: Some(RecipeCacheConfig {
                inputs: vec![
                    String::from("main.go"),                                 // simple relative
                    String::from("../../../../../libs/test_reader/**/*.go"), // complex multi-level pattern
                    String::from("../../shared.txt"),                        // intermediate level
                ],
                ..Default::default()
            }),
            template: None,
            parameters: std::collections::BTreeMap::new(),
            run_status: RunStatus::default(),
        };

        // This should not panic and should find the files including the complex path
        let result = recipe.get_self_hash();
        assert!(
            result.is_ok(),
            "Complex path hash calculation should succeed: {result:?}"
        );

        // The hash should be reproducible
        let hash1 = recipe.get_self_hash().unwrap();
        let hash2 = recipe.get_self_hash().unwrap();
        assert_eq!(hash1, hash2, "Complex path hash should be reproducible");
    }

    #[test]
    fn test_tags_default_empty() {
        // Test that tags default to empty vector
        let recipe = Recipe::default();
        assert_eq!(recipe.tags, Vec::<String>::new());
    }

    #[test]
    fn test_tags_serialization() {
        // Test that tags serialize and deserialize correctly
        let yaml = r#"
name: test
description: "Test recipe"
tags: ["frontend", "build"]
run: echo "test"
"#;
        let recipe: Recipe = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(recipe.tags, vec!["frontend", "build"]);
    }

    #[test]
    fn test_tags_affect_hash() {
        // Test that tags affect the recipe hash
        let mut recipe1 = Recipe {
            name: String::from("test"),
            cookbook: String::from("test"),
            project_root: PathBuf::from(config_path("/valid/")),
            config_path: PathBuf::from(config_path("/valid/foo/cookbook.yml")),
            description: None,
            tags: vec!["frontend".to_string()],
            dependencies: None,
            environment: vec![],
            variables: IndexMap::new(),
            overrides: BTreeMap::new(),
            processed_variables: IndexMap::new(),
            run: String::from("test"),
            cache: None,
            template: None,
            parameters: std::collections::BTreeMap::new(),
            run_status: RunStatus::default(),
        };

        let hash1 = recipe1.get_self_hash().unwrap();

        // Change tags
        recipe1.tags = vec!["backend".to_string()];
        let hash2 = recipe1.get_self_hash().unwrap();

        assert_ne!(
            hash1, hash2,
            "Different tags should produce different hashes"
        );
    }
}
