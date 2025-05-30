use std::collections::BTreeMap;

use anyhow::{anyhow, Result};
use petgraph::Direction;

use crate::project::BakeProject;

/// Recursively calculates the combined hash of a recipe and its dependencies.
///
/// This function traverses the dependency graph of a given recipe, computes
/// the hash of each dependency, and combines these hashes with the self-hash
/// of the target recipe. Memoization is used to avoid redundant calculations
/// for already processed recipes.
///
/// # Arguments
///
/// * `recipe_fqn`: The fully qualified name (FQN) of the recipe (e.g., "cookbook_name:recipe_name").
/// * `project`: A reference to the `BakeProject` containing the recipes and dependency graph.
/// * `memoized_hashes`: A mutable reference to a `BTreeMap` used for memoizing calculated hashes.
///
/// # Returns
///
/// A `Result` containing the combined hash as a `String`, or an `anyhow::Error` if
/// an error occurs (e.g., recipe not found, hashing error).
fn calculate_combined_hash_recursive(
    recipe_fqn: &str,
    project: &BakeProject,
    memoized_hashes: &mut BTreeMap<String, String>,
) -> Result<String> {
    if let Some(cached_hash) = memoized_hashes.get(recipe_fqn) {
        return Ok(cached_hash.clone());
    }

    let recipe = project
        .get_recipe_by_fqn(recipe_fqn)
        .ok_or_else(|| anyhow!("Recipe '{}' not found for hashing.", recipe_fqn))?;

    let self_hash = recipe.get_self_hash()?;

    let mut dep_hashes: Vec<String> = Vec::new();

    if let Some(source_node_index) = project
        .recipe_dependency_graph
        .fqn_to_node_index
        .get(recipe_fqn)
    {
        for dep_node_index in project
            .recipe_dependency_graph
            .graph
            .neighbors_directed(*source_node_index, Direction::Outgoing)
        {
            let dep_fqn_str = &project.recipe_dependency_graph.graph[dep_node_index];
            let dep_combined_hash =
                calculate_combined_hash_recursive(dep_fqn_str, project, memoized_hashes)?;
            dep_hashes.push(dep_combined_hash);
        }
    } else {
        return Err(anyhow!(
            "NodeIndex not found for recipe FQN: {}",
            recipe_fqn
        ));
    }

    dep_hashes.sort();

    let mut combined_data = self_hash;
    for dep_hash in dep_hashes {
        combined_data.push_str(&dep_hash);
    }

    let mut hasher = blake3::Hasher::new();
    hasher.update(combined_data.as_bytes());
    let final_hash = hasher.finalize().to_string();

    memoized_hashes.insert(recipe_fqn.to_string(), final_hash.clone());
    Ok(final_hash)
}

/// Calculates the combined hash for a specific recipe within a Bake project.
///
/// This function serves as a public entry point to the hashing logic. It initializes
/// the memoization map and calls the recursive helper function to compute the
/// combined hash, which includes the recipe's own content hash and the hashes
/// of all its dependencies.
///
/// # Arguments
///
/// * `recipe_fqn`: The fully qualified name (FQN) of the recipe for which to calculate the hash.
/// * `project`: A reference to the `BakeProject` containing the recipe and its context.
///
/// # Returns
///
/// A `Result` containing the combined hash as a `String`, or an `anyhow::Error`
/// if the calculation fails.
pub fn calculate_combined_hash_for_recipe(
    recipe_fqn: &str,
    project: &BakeProject,
) -> Result<String> {
    let mut memoized_hashes = BTreeMap::new();
    calculate_combined_hash_recursive(recipe_fqn, project, &mut memoized_hashes)
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

        let combined_hash = calculate_combined_hash_for_recipe(recipe_a_fqn, &project)?;

        assert!(
            !combined_hash.is_empty(),
            "Combined hash should not be empty"
        );

        let recipe_a_obj = project.get_recipe_by_fqn(recipe_a_fqn).unwrap();
        let expected_self_hash = recipe_a_obj.get_self_hash()?;

        let mut hasher = blake3::Hasher::new();
        hasher.update(expected_self_hash.as_bytes());
        let expected_combined_hash = hasher.finalize().to_string();

        assert_eq!(combined_hash, expected_combined_hash);
        Ok(())
    }

    #[test]
    fn test_recipe_with_one_dependency() -> Result<()> {
        let project = TestProjectBuilder::new()
            .with_cookbook("cookbook1", &["recipe_a", "recipe_b"])
            .with_dependency("cookbook1:recipe_a", "cookbook1:recipe_b")
            .build();

        let recipe_a_fqn = "cookbook1:recipe_a";
        let recipe_b_fqn = "cookbook1:recipe_b";

        // Calculate expected hash
        let recipe_a_obj = project.get_recipe_by_fqn(recipe_a_fqn).unwrap();
        let recipe_b_obj = project.get_recipe_by_fqn(recipe_b_fqn).unwrap();

        let recipe_a_self_hash = recipe_a_obj.get_self_hash()?;
        let recipe_b_self_hash = recipe_b_obj.get_self_hash()?;

        // recipe_b's combined hash (no dependencies)
        let mut hasher_b = blake3::Hasher::new();
        hasher_b.update(recipe_b_self_hash.as_bytes());
        let recipe_b_combined_hash = hasher_b.finalize().to_string();

        // recipe_a's combined hash (depends on recipe_b)
        let mut combined_data_a = recipe_a_self_hash;
        combined_data_a.push_str(&recipe_b_combined_hash);

        let mut hasher_a = blake3::Hasher::new();
        hasher_a.update(combined_data_a.as_bytes());
        let expected_combined_hash = hasher_a.finalize().to_string();

        let actual_combined_hash = calculate_combined_hash_for_recipe(recipe_a_fqn, &project)?;

        assert_eq!(actual_combined_hash, expected_combined_hash);
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
        let mut hasher_b = blake3::Hasher::new();
        hasher_b.update(recipe_b_self_hash.as_bytes());
        let recipe_b_combined_hash = hasher_b.finalize().to_string();

        // recipe_c's combined hash (no dependencies)
        let mut hasher_c = blake3::Hasher::new();
        hasher_c.update(recipe_c_self_hash.as_bytes());
        let recipe_c_combined_hash = hasher_c.finalize().to_string();

        // recipe_a's combined hash (depends on recipe_b and recipe_c)
        // Ensure consistent ordering of dependency hashes
        let mut dep_hashes = vec![recipe_b_combined_hash, recipe_c_combined_hash];
        dep_hashes.sort();

        let mut combined_data_a = recipe_a_self_hash;
        for dep_hash in dep_hashes {
            combined_data_a.push_str(&dep_hash);
        }

        let mut hasher_a = blake3::Hasher::new();
        hasher_a.update(combined_data_a.as_bytes());
        let expected_combined_hash = hasher_a.finalize().to_string();

        let actual_combined_hash = calculate_combined_hash_for_recipe(recipe_a_fqn, &project)?;

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
        let mut hasher_c = blake3::Hasher::new();
        hasher_c.update(recipe_c_self_hash.as_bytes());
        let recipe_c_combined_hash = hasher_c.finalize().to_string();

        // recipe_b's combined hash (depends on recipe_c)
        let mut combined_data_b = recipe_b_self_hash;
        combined_data_b.push_str(&recipe_c_combined_hash);
        let mut hasher_b = blake3::Hasher::new();
        hasher_b.update(combined_data_b.as_bytes());
        let recipe_b_combined_hash = hasher_b.finalize().to_string();

        // recipe_a's combined hash (depends on recipe_b)
        let mut combined_data_a = recipe_a_self_hash;
        combined_data_a.push_str(&recipe_b_combined_hash);

        let mut hasher_a = blake3::Hasher::new();
        hasher_a.update(combined_data_a.as_bytes());
        let expected_combined_hash = hasher_a.finalize().to_string();

        let actual_combined_hash = calculate_combined_hash_for_recipe(recipe_a_fqn, &project)?;

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
        let mut hasher_d = blake3::Hasher::new();
        hasher_d.update(recipe_d_self_hash.as_bytes());
        let recipe_d_combined_hash = hasher_d.finalize().to_string();

        // recipe_b's combined hash (depends on recipe_d)
        let mut combined_data_b = recipe_b_self_hash;
        combined_data_b.push_str(&recipe_d_combined_hash);
        let mut hasher_b = blake3::Hasher::new();
        hasher_b.update(combined_data_b.as_bytes());
        let recipe_b_combined_hash = hasher_b.finalize().to_string();

        // recipe_c's combined hash (depends on recipe_d)
        let mut combined_data_c = recipe_c_self_hash;
        combined_data_c.push_str(&recipe_d_combined_hash);
        let mut hasher_c = blake3::Hasher::new();
        hasher_c.update(combined_data_c.as_bytes());
        let recipe_c_combined_hash = hasher_c.finalize().to_string();

        // recipe_a's combined hash (depends on recipe_b and recipe_c)
        // Ensure consistent ordering of dependency hashes
        let mut dep_hashes_a = vec![recipe_b_combined_hash, recipe_c_combined_hash];
        dep_hashes_a.sort();

        let mut combined_data_a = recipe_a_self_hash;
        for dep_hash in dep_hashes_a {
            combined_data_a.push_str(&dep_hash);
        }

        let mut hasher_a = blake3::Hasher::new();
        hasher_a.update(combined_data_a.as_bytes());
        let expected_combined_hash = hasher_a.finalize().to_string();

        let actual_combined_hash = calculate_combined_hash_for_recipe(recipe_a_fqn, &project)?;

        assert_eq!(actual_combined_hash, expected_combined_hash);
        Ok(())
    }
}
