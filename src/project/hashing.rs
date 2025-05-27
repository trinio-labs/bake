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
}
