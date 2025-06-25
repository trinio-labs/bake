use std::collections::BTreeMap;

use anyhow::{anyhow, Result};
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
    use crate::test_utils::TestProjectBuilder;

    #[test]
    fn test_single_recipe_no_dependencies() -> Result<()> {
        let project = TestProjectBuilder::new()
            .with_cookbook("cookbook1", &["recipe_a"])
            .build();

        let recipe_a_fqn = "cookbook1:recipe_a";

        let mut hasher = RecipeHasher::new(&project);
        let combined_hash = hasher.hash_for(recipe_a_fqn)?;

        assert!(
            !combined_hash.is_empty(),
            "Combined hash should not be empty"
        );

        // For a recipe with no dependencies, combined hash equals compute_combined_hash(self_hash, [])
        let recipe_a_obj = project.get_recipe_by_fqn(recipe_a_fqn).unwrap();
        let expected_self_hash = recipe_a_obj.get_self_hash()?;
        let expected_combined_hash = compute_combined_hash(expected_self_hash, vec![]);

        assert_eq!(combined_hash, expected_combined_hash);
        Ok(())
    }

    #[test]
    fn test_recipe_with_one_dependency() -> Result<()> {
        let project = TestProjectBuilder::new()
            .with_cookbook("cookbook1", &["recipe_a", "recipe_b"])
            .with_dependency("cookbook1:recipe_a", "cookbook1:recipe_b")
            .build();

        let mut hasher = RecipeHasher::new(&project);

        // Get both hashes
        let recipe_a_hash = hasher.hash_for("cookbook1:recipe_a")?;
        let recipe_b_hash = hasher.hash_for("cookbook1:recipe_b")?;

        // Basic assertions
        assert!(
            !recipe_a_hash.is_empty(),
            "recipe_a hash should not be empty"
        );
        assert!(
            !recipe_b_hash.is_empty(),
            "recipe_b hash should not be empty"
        );
        assert_ne!(recipe_a_hash, recipe_b_hash, "Hashes should be different");

        // Test memoization: calling again should return same result
        let recipe_a_hash_2 = hasher.hash_for("cookbook1:recipe_a")?;
        assert_eq!(recipe_a_hash, recipe_a_hash_2, "Memoization should work");

        Ok(())
    }

    #[test]
    fn test_recipe_with_multiple_dependencies() -> Result<()> {
        let project = TestProjectBuilder::new()
            .with_cookbook("cookbook1", &["recipe_a", "recipe_b", "recipe_c"])
            .with_dependency("cookbook1:recipe_a", "cookbook1:recipe_b")
            .with_dependency("cookbook1:recipe_a", "cookbook1:recipe_c")
            .build();

        let recipe_a_fqn = "cookbook1:recipe_a";
        let recipe_b_fqn = "cookbook1:recipe_b";
        let recipe_c_fqn = "cookbook1:recipe_c";

        // Calculate expected hash
        let recipe_a_obj = project.get_recipe_by_fqn(recipe_a_fqn).unwrap();
        let recipe_b_obj = project.get_recipe_by_fqn(recipe_b_fqn).unwrap();
        let recipe_c_obj = project.get_recipe_by_fqn(recipe_c_fqn).unwrap();

        let recipe_a_self_hash = recipe_a_obj.get_self_hash()?;
        let recipe_b_self_hash = recipe_b_obj.get_self_hash()?;
        let recipe_c_self_hash = recipe_c_obj.get_self_hash()?;

        // recipe_b's combined hash (no dependencies)
        let recipe_b_combined_hash = compute_combined_hash(recipe_b_self_hash, vec![]);

        // recipe_c's combined hash (no dependencies)
        let recipe_c_combined_hash = compute_combined_hash(recipe_c_self_hash, vec![]);

        // recipe_a's combined hash (depends on recipe_b and recipe_c)
        let expected_combined_hash = compute_combined_hash(
            recipe_a_self_hash,
            vec![recipe_b_combined_hash, recipe_c_combined_hash],
        );

        let mut hasher = RecipeHasher::new(&project);
        let actual_combined_hash = hasher.hash_for(recipe_a_fqn)?;

        assert_eq!(actual_combined_hash, expected_combined_hash);
        Ok(())
    }

    #[test]
    fn test_recipe_with_transitive_dependencies() -> Result<()> {
        let project = TestProjectBuilder::new()
            .with_cookbook("cookbook1", &["recipe_a", "recipe_b", "recipe_c"])
            .with_dependency("cookbook1:recipe_a", "cookbook1:recipe_b")
            .with_dependency("cookbook1:recipe_b", "cookbook1:recipe_c")
            .build();

        let recipe_a_fqn = "cookbook1:recipe_a";
        let recipe_b_fqn = "cookbook1:recipe_b";
        let recipe_c_fqn = "cookbook1:recipe_c";

        // Calculate expected hash
        let recipe_a_obj = project.get_recipe_by_fqn(recipe_a_fqn).unwrap();
        let recipe_b_obj = project.get_recipe_by_fqn(recipe_b_fqn).unwrap();
        let recipe_c_obj = project.get_recipe_by_fqn(recipe_c_fqn).unwrap();

        let recipe_a_self_hash = recipe_a_obj.get_self_hash()?;
        let recipe_b_self_hash = recipe_b_obj.get_self_hash()?;
        let recipe_c_self_hash = recipe_c_obj.get_self_hash()?;

        // recipe_c's combined hash (no dependencies)
        let recipe_c_combined_hash = compute_combined_hash(recipe_c_self_hash, vec![]);

        // recipe_b's combined hash (depends on recipe_c)
        let recipe_b_combined_hash =
            compute_combined_hash(recipe_b_self_hash, vec![recipe_c_combined_hash]);

        // recipe_a's combined hash (depends on recipe_b)
        let expected_combined_hash =
            compute_combined_hash(recipe_a_self_hash, vec![recipe_b_combined_hash]);

        let mut hasher = RecipeHasher::new(&project);
        let actual_combined_hash = hasher.hash_for(recipe_a_fqn)?;

        assert_eq!(actual_combined_hash, expected_combined_hash);
        Ok(())
    }

    #[test]
    fn test_recipe_with_diamond_dependency() -> Result<()> {
        let project = TestProjectBuilder::new()
            .with_cookbook(
                "cookbook1",
                &["recipe_a", "recipe_b", "recipe_c", "recipe_d"],
            )
            .with_dependency("cookbook1:recipe_a", "cookbook1:recipe_b")
            .with_dependency("cookbook1:recipe_a", "cookbook1:recipe_c")
            .with_dependency("cookbook1:recipe_b", "cookbook1:recipe_d")
            .with_dependency("cookbook1:recipe_c", "cookbook1:recipe_d")
            .build();

        let recipe_a_fqn = "cookbook1:recipe_a";
        let recipe_b_fqn = "cookbook1:recipe_b";
        let recipe_c_fqn = "cookbook1:recipe_c";
        let recipe_d_fqn = "cookbook1:recipe_d";

        // Calculate expected hash
        let recipe_a_obj = project.get_recipe_by_fqn(recipe_a_fqn).unwrap();
        let recipe_b_obj = project.get_recipe_by_fqn(recipe_b_fqn).unwrap();
        let recipe_c_obj = project.get_recipe_by_fqn(recipe_c_fqn).unwrap();
        let recipe_d_obj = project.get_recipe_by_fqn(recipe_d_fqn).unwrap();

        let recipe_a_self_hash = recipe_a_obj.get_self_hash()?;
        let recipe_b_self_hash = recipe_b_obj.get_self_hash()?;
        let recipe_c_self_hash = recipe_c_obj.get_self_hash()?;
        let recipe_d_self_hash = recipe_d_obj.get_self_hash()?;

        // recipe_d's combined hash (no dependencies)
        let recipe_d_combined_hash = compute_combined_hash(recipe_d_self_hash, vec![]);

        // recipe_b's combined hash (depends on recipe_d)
        let recipe_b_combined_hash =
            compute_combined_hash(recipe_b_self_hash, vec![recipe_d_combined_hash.clone()]);

        // recipe_c's combined hash (depends on recipe_d)
        let recipe_c_combined_hash =
            compute_combined_hash(recipe_c_self_hash, vec![recipe_d_combined_hash]);

        // recipe_a's combined hash (depends on recipe_b and recipe_c)
        let expected_combined_hash = compute_combined_hash(
            recipe_a_self_hash,
            vec![recipe_b_combined_hash, recipe_c_combined_hash],
        );

        let mut hasher = RecipeHasher::new(&project);
        let actual_combined_hash = hasher.hash_for(recipe_a_fqn)?;

        assert_eq!(actual_combined_hash, expected_combined_hash);
        Ok(())
    }
}
