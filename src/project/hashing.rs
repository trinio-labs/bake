use std::collections::BTreeMap;

use anyhow::{Result, anyhow};
use petgraph::Direction;

use crate::project::BakeProject;

/// Helper function for Blake3 hashing
fn blake3_hash(data: &str) -> String {
    blake3::hash(data.as_bytes()).to_string()
}

/// Computes a combined hash from a recipe's self hash and its dependency hashes.
fn compute_combined_hash(self_hash: String, mut dependency_hashes: Vec<String>) -> String {
    dependency_hashes.sort();
    let combined_data = format!("{}{}", self_hash, dependency_hashes.join(""));
    blake3_hash(&combined_data)
}

/// RecipeHasher efficiently computes and memoizes combined hashes for recipes and their dependencies.
pub struct RecipeHasher<'a> {
    project: &'a BakeProject,
    memoized_hashes: BTreeMap<String, String>,
}

impl<'a> RecipeHasher<'a> {
    /// Create a new RecipeHasher for the given project.
    pub fn new(project: &'a BakeProject) -> Self {
        Self {
            project,
            memoized_hashes: BTreeMap::new(),
        }
    }

    /// Returns the combined hash for a given recipe FQN, including all dependencies.
    pub fn hash_for(&mut self, recipe_fqn: &str) -> Result<String> {
        if let Some(cached_hash) = self.memoized_hashes.get(recipe_fqn) {
            return Ok(cached_hash.clone());
        }

        let recipe = self
            .project
            .get_recipe_by_fqn(recipe_fqn)
            .ok_or_else(|| anyhow!("Recipe '{}' not found for hashing.", recipe_fqn))?;

        let self_hash = recipe.get_self_hash()?;
        let dep_hashes = self.collect_dependency_hashes(recipe_fqn)?;
        let final_hash = compute_combined_hash(self_hash, dep_hashes);

        self.memoized_hashes
            .insert(recipe_fqn.to_string(), final_hash.clone());
        Ok(final_hash)
    }

    /// Collects dependency hashes for a given recipe
    fn collect_dependency_hashes(&mut self, recipe_fqn: &str) -> Result<Vec<String>> {
        let node_index = self
            .project
            .recipe_dependency_graph
            .fqn_to_node_index
            .get(recipe_fqn)
            .ok_or_else(|| anyhow!("NodeIndex not found for recipe FQN: {}", recipe_fqn))?;

        self.project
            .recipe_dependency_graph
            .graph
            .neighbors_directed(*node_index, Direction::Outgoing)
            .map(|dep_node_index| {
                let dep_fqn = &self.project.recipe_dependency_graph.graph[dep_node_index];
                self.hash_for(dep_fqn)
            })
            .collect()
    }

    /// Returns the memoized map, useful for batch hash collection.
    pub fn into_memoized_hashes(self) -> BTreeMap<String, String> {
        self.memoized_hashes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_blake3_hash_consistent() {
        let data = "test data";
        let hash1 = blake3_hash(data);
        let hash2 = blake3_hash(data);
        assert_eq!(hash1, hash2);
        assert!(!hash1.is_empty());
    }

    #[test]
    fn test_blake3_hash_different_inputs() {
        let hash1 = blake3_hash("input1");
        let hash2 = blake3_hash("input2");
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_blake3_hash_empty_string() {
        let hash = blake3_hash("");
        assert!(!hash.is_empty());
        assert_eq!(hash.len(), 64); // Blake3 produces 256-bit hash = 64 hex chars
    }

    #[test]
    fn test_compute_combined_hash() {
        let self_hash = "self".to_string();
        let deps = vec!["dep1".to_string(), "dep2".to_string()];

        let combined = compute_combined_hash(self_hash.clone(), deps.clone());
        assert!(!combined.is_empty());
        assert_ne!(combined, self_hash);
        assert_eq!(combined.len(), 64); // Blake3 hash length
    }

    #[test]
    fn test_compute_combined_hash_sorts_dependencies() {
        let self_hash = "self".to_string();
        let deps1 = vec!["b".to_string(), "a".to_string()];
        let deps2 = vec!["a".to_string(), "b".to_string()];

        let hash1 = compute_combined_hash(self_hash.clone(), deps1);
        let hash2 = compute_combined_hash(self_hash.clone(), deps2);
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_compute_combined_hash_with_empty_deps() {
        let self_hash = "self".to_string();
        let deps = vec![];

        let combined = compute_combined_hash(self_hash.clone(), deps);
        let expected = blake3_hash("self");
        assert_eq!(combined, expected);
    }

    #[test]
    fn test_compute_combined_hash_different_self_hash() {
        let deps = vec!["dep1".to_string()];

        let hash1 = compute_combined_hash("self1".to_string(), deps.clone());
        let hash2 = compute_combined_hash("self2".to_string(), deps.clone());
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_compute_combined_hash_different_deps() {
        let self_hash = "self".to_string();

        let hash1 = compute_combined_hash(self_hash.clone(), vec!["dep1".to_string()]);
        let hash2 = compute_combined_hash(self_hash.clone(), vec!["dep2".to_string()]);
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_compute_combined_hash_order_independence() {
        let self_hash = "test".to_string();
        let deps1 = vec!["z".to_string(), "a".to_string(), "m".to_string()];
        let deps2 = vec!["a".to_string(), "m".to_string(), "z".to_string()];
        let deps3 = vec!["m".to_string(), "z".to_string(), "a".to_string()];

        let hash1 = compute_combined_hash(self_hash.clone(), deps1);
        let hash2 = compute_combined_hash(self_hash.clone(), deps2);
        let hash3 = compute_combined_hash(self_hash.clone(), deps3);

        assert_eq!(hash1, hash2);
        assert_eq!(hash2, hash3);
    }

    #[test]
    fn test_compute_combined_hash_duplicate_dependencies() {
        let self_hash = "test".to_string();
        let deps = vec!["dep1".to_string(), "dep1".to_string(), "dep2".to_string()];

        let combined = compute_combined_hash(self_hash, deps);
        assert!(!combined.is_empty());
        assert_eq!(combined.len(), 64);
    }
}
