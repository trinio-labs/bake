use std::collections::{BTreeMap, HashSet, VecDeque};

use anyhow::bail;
use petgraph::{
    Direction, // Added for explicit use of Direction::Incoming
    algo::{is_cyclic_directed, tarjan_scc},
    graph::{Graph, NodeIndex},
    visit::Dfs,
};

use crate::project::Cookbook;

/// Represents a dependency graph of recipes.
///
/// The graph stores recipes as nodes, identified by their fully qualified names (FQNs),
/// and dependencies as directed edges. It provides functionalities to populate the graph
/// from cookbook definitions, validate dependencies, detect circular dependencies,
/// and query for transitive dependencies of a given recipe.
#[derive(Debug, Clone, Default)]
pub struct RecipeDependencyGraph {
    /// The petgraph `Graph` instance where nodes are recipe FQNs (String) and edges represent dependencies.
    pub graph: Graph<String, ()>,
    /// A map from recipe FQN (String) to its `NodeIndex` in the graph for quick lookups.
    pub fqn_to_node_index: BTreeMap<String, NodeIndex>,
}

impl RecipeDependencyGraph {
    /// Creates a new, empty `RecipeDependencyGraph`.
    pub fn new() -> Self {
        Default::default()
    }

    /// Populates the dependency graph from a collection of cookbooks.
    ///
    /// This method performs several key operations:
    /// 1. Clears any existing graph data.
    /// 2. Adds a node for each recipe found in the provided cookbooks, using the recipe's FQN.
    /// 3. Creates an edge for each declared dependency between recipes.
    /// 4. Validates that all declared dependencies refer to existing recipes. If not, it returns an error
    ///    listing all missing dependencies.
    /// 5. Checks for circular dependencies within the graph. If cycles are detected, it returns an error
    ///    listing the recipes involved in the cycles.
    ///
    /// # Arguments
    ///
    /// * `cookbooks`: A `BTreeMap` where keys are cookbook names and values are `Cookbook` instances.
    ///
    /// # Errors
    ///
    /// Returns `anyhow::Error` if:
    /// * There are missing dependencies (a recipe depends on a non-existent recipe).
    /// * Circular dependencies are detected in the graph.
    pub fn populate_from_cookbooks(
        &mut self,
        cookbooks: &BTreeMap<String, Cookbook>,
    ) -> anyhow::Result<()> {
        self.clear_graph_data();
        self.add_recipe_nodes_from_cookbooks(cookbooks);
        self.add_dependency_edges_and_validate_dependencies(cookbooks)?;
        self.ensure_no_circular_dependencies()?;
        Ok(())
    }

    /// Clears all nodes and edges from the graph and resets the FQN to NodeIndex map.
    fn clear_graph_data(&mut self) {
        self.graph.clear();
        self.fqn_to_node_index.clear();
    }

    /// Adds a node to the graph for each recipe found in the provided cookbooks.
    /// Nodes are identified by the recipe's FQN.
    fn add_recipe_nodes_from_cookbooks(&mut self, cookbooks: &BTreeMap<String, Cookbook>) {
        cookbooks
            .values()
            .flat_map(|cookbook| cookbook.recipes.values())
            .for_each(|recipe| {
                let fqn = recipe.full_name();
                let node_index = self.graph.add_node(fqn.clone());
                self.fqn_to_node_index.insert(fqn, node_index);
            });
    }

    /// Adds edges to the graph for each declared dependency between recipes and validates them.
    ///
    /// # Errors
    ///
    /// Returns `anyhow::Error` if any declared dependency refers to a non-existent recipe.
    fn add_dependency_edges_and_validate_dependencies(
        &mut self,
        cookbooks: &BTreeMap<String, Cookbook>,
    ) -> anyhow::Result<()> {
        let missing_deps_messages: Vec<String> = cookbooks
            .values()
            .flat_map(|cookbook| cookbook.recipes.values())
            .flat_map(|recipe| {
                let source_fqn = recipe.full_name();
                let source_node_index = match self.fqn_to_node_index.get(&source_fqn) {
                    Some(index) => *index,
                    None => {
                        // This should not happen in normal operation, but we'll handle it gracefully
                        // by skipping this recipe and logging an error
                        eprintln!("Internal graph inconsistency: FQN '{source_fqn}' not found in node map");
                        return Vec::new();
                    }
                };

                recipe.dependencies.as_ref().map_or_else(
                    Vec::new, // No dependencies, so no messages.
                    |deps| {
                        deps.iter()
                            .filter_map(|dep_fqn| {
                                if let Some(target_node_index) = self.fqn_to_node_index.get(dep_fqn)
                                {
                                    self.graph
                                        .add_edge(source_node_index, *target_node_index, ());
                                    None // Dependency found, no error message.
                                } else {
                                    // Dependency FQN not found in the graph.
                                    Some(format!(
                                        "  - Recipe '{}' (defined in {}) depends on '{}', which was not found.",
                                        recipe.name,
                                        recipe.config_path.display(),
                                        dep_fqn
                                    ))
                                }
                            })
                            .collect::<Vec<_>>()
                    },
                )
            })
            .collect();

        if !missing_deps_messages.is_empty() {
            bail!(
                "Dependency Graph: Recipe dependency errors found during graph population:\n{}",
                missing_deps_messages.join("\n")
            );
        }
        Ok(())
    }

    /// Checks for circular dependencies within the graph.
    ///
    /// # Errors
    ///
    /// Returns `anyhow::Error` if cycles are detected, listing the recipes involved.
    fn ensure_no_circular_dependencies(&self) -> anyhow::Result<()> {
        if is_cyclic_directed(&self.graph) {
            let mut cycles_messages = Vec::new();
            let sccs = tarjan_scc(&self.graph); // Tarjan's SCC algorithm identifies strongly connected components.
            for scc_node_indices in sccs {
                // An SCC represents a cycle if it contains more than one node,
                // or if it contains one node that has an edge to itself.
                if scc_node_indices.len() > 1
                    || (scc_node_indices.len() == 1
                        && self
                            .graph
                            .find_edge(scc_node_indices[0], scc_node_indices[0])
                            .is_some())
                {
                    let cycle_path: Vec<String> = scc_node_indices
                        .iter()
                        .map(|node_idx| self.graph[*node_idx].clone()) // Get FQN from NodeIndex
                        .collect();
                    cycles_messages
                        .push(format!("  - Cycle detected: {}", cycle_path.join(" -> ")));
                }
            }

            if cycles_messages.is_empty() {
                // This case should ideally not be reached if is_cyclic_directed is true
                // and tarjan_scc is working correctly, but it's a safeguard.
                bail!(
                    "Dependency Graph: Circular dependencies detected during graph population. (SCC analysis did not pinpoint specific cycle paths, but the graph is cyclic. This might indicate a complex cycle structure or an issue with the cycle detection algorithm under certain graph conditions.)"
                );
            } else {
                bail!(
                    "Dependency Graph: Circular dependencies detected during graph population:\n{}",
                    cycles_messages.join("\n")
                );
            }
        }
        Ok(())
    }

    /// Returns a set of all transitive dependency FQNs for a given recipe FQN.
    ///
    /// This method performs a Depth-First Search (DFS) starting from the given recipe
    /// to find all recipes it directly or indirectly depends on. The FQN of the starting
    /// recipe itself is not included in the result.
    ///
    /// # Arguments
    ///
    /// * `recipe_fqn`: The fully qualified name of the recipe for which to find dependencies.
    ///
    /// # Returns
    ///
    /// A `HashSet<String>` containing the FQNs of all transitive dependencies.
    /// Returns an empty set if the recipe FQN is not found or has no dependencies.
    pub fn get_all_dependencies_for(&self, recipe_fqn: &str) -> HashSet<String> {
        let mut dependencies = HashSet::new();
        if let Some(start_node_idx) = self.fqn_to_node_index.get(recipe_fqn) {
            let mut dfs = Dfs::new(&self.graph, *start_node_idx);
            // The first node returned by DFS is the starting node itself. We skip it
            // as we are interested in its dependencies, not the node itself.
            dfs.next(&self.graph);

            while let Some(nx_idx) = dfs.next(&self.graph) {
                dependencies.insert(self.graph[nx_idx].clone());
            }
        }
        dependencies
    }

    /// Calculates the complete execution plan (all transitive dependencies and execution order)
    /// for a given set of initial target recipe FQNs.
    ///
    /// This method performs two main steps:
    /// 1. Expands the `initial_target_fqns` to include all their transitive dependencies.
    ///    It ensures all FQNs in `initial_target_fqns` exist in the graph.
    /// 2. Performs a topological sort (Kahn's algorithm) on this complete set of FQNs
    ///    to determine the execution levels. Recipes within the same level can be
    ///    executed in parallel.
    ///
    /// # Arguments
    ///
    /// * `initial_target_fqns`: A `HashSet<String>` containing the fully qualified names of the
    ///   recipes the user initially wants to execute.
    ///
    /// # Returns
    ///
    /// A `Result<Vec<Vec<String>>, anyhow::Error>` where:
    /// - `Ok(Vec<Vec<String>>)`: A vector of vectors of FQNs. Each inner vector
    ///   represents a level of recipes that can be executed in parallel. Levels are
    ///   ordered according to their dependencies.
    /// - `Err(anyhow::Error)`: An error if:
    ///   - Any FQN in `initial_target_fqns` is not found in the graph.
    ///   - A cycle is detected among the recipes (including dependencies).
    ///
    /// If `initial_target_fqns` is empty, an empty `Vec::new()` is returned successfully.
    pub fn get_execution_plan_for_initial_targets(
        &self,
        initial_target_fqns: &HashSet<String>,
    ) -> anyhow::Result<Vec<Vec<String>>> {
        if initial_target_fqns.is_empty() {
            return Ok(Vec::new());
        }

        let mut all_required_fqns: HashSet<String> = HashSet::new();

        // Validate initial targets and expand to include all dependencies.
        for fqn in initial_target_fqns {
            if !self.fqn_to_node_index.contains_key(fqn) {
                bail!(
                    "Execution Plan: Initial target recipe FQN '{}' not found in the dependency graph. Ensure the recipe exists and is correctly defined in a cookbook.",
                    fqn
                );
            }
            all_required_fqns.insert(fqn.clone());
            let dependencies = self.get_all_dependencies_for(fqn);
            all_required_fqns.extend(dependencies);
        }

        // Now that we have the complete set of FQNs, get their execution order.
        self.get_execution_order_for_targets(&all_required_fqns)
    }

    /// Determines the execution order for a given set of target recipe FQNs.
    ///
    /// This method takes a `HashSet` of FQNs, representing the recipes that need to be
    /// executed (including all their transitive dependencies). It then performs a
    /// topological sort (Kahn's algorithm) on the subgraph formed by these target FQNs
    /// to determine the execution levels. Recipes within the same level can be
    /// executed in parallel.
    ///
    /// # Arguments
    ///
    /// * `target_fqns`: A `HashSet<String>` containing the fully qualified names of all
    ///   recipes that are part of the current execution plan.
    ///
    /// # Returns
    ///
    /// A `Result<Vec<Vec<String>>, anyhow::Error>` where:
    /// - `Ok(Vec<Vec<String>>)`: A vector of vectors of FQNs. Each inner vector
    ///   represents a level of recipes that can be executed in parallel. Levels are
    ///   ordered according to their dependencies.
    /// - `Err(anyhow::Error)`: An error if a cycle is detected within the `target_fqns`
    ///   subset of the graph, or if an FQN in `target_fqns` is not found in the graph.
    ///
    /// If `target_fqns` is empty, an empty `Vec::new()` is returned successfully.
    pub fn get_execution_order_for_targets(
        &self,
        target_fqns: &HashSet<String>,
    ) -> anyhow::Result<Vec<Vec<String>>> {
        if target_fqns.is_empty() {
            return Ok(Vec::new());
        }

        // Build a subgraph representation for Kahn's algorithm, focusing only on target_fqns.
        // `subgraph_adj_rev` stores reverse edges: for a recipe, lists recipes that depend on it.
        let mut subgraph_adj_rev: BTreeMap<String, Vec<String>> = BTreeMap::new();
        // `subgraph_in_degree` stores the number of dependencies for each recipe within the subgraph.
        let mut subgraph_in_degree: BTreeMap<String, usize> = BTreeMap::new();

        for fqn in target_fqns {
            if !self.fqn_to_node_index.contains_key(fqn) {
                bail!(
                    "Execution Order: Recipe FQN '{}' targeted for execution not found in the dependency graph. This recipe was expected to be part of the graph but is missing.",
                    fqn
                );
            }
            subgraph_adj_rev.insert(fqn.clone(), Vec::new());
            subgraph_in_degree.insert(fqn.clone(), 0);
        }

        // Populate the adjacency list (reverse) and in-degrees for the subgraph.
        // Note: The main graph stores edges from dependent to dependency (e.g., recipe -> its_dependency).
        // For Kahn's algorithm, we often think about it as:
        // - In-degree: number of direct dependencies a recipe has.
        // - When a recipe is "processed", we "remove" it and decrement the in-degree of recipes that depended on it.

        // Iterate through each FQN in the target set. This FQN is a potential *dependency*.
        for dependency_fqn_str in target_fqns {
            let dependency_node_idx = self.fqn_to_node_index[dependency_fqn_str];

            // Find all recipes in the main graph that *depend on* `dependency_fqn_str`.
            // These are incoming neighbors to `dependency_node_idx` in the main graph
            // if we consider edges as (dependent_recipe_node, dependency_recipe_node).
            // However, petgraph's `neighbors_directed` with `Direction::Incoming` gives nodes `u` such that `(u, v)` is an edge.
            // If our graph edges are (source_recipe, its_dependency_recipe), then:
            // - `source_recipe` is `u`
            // - `its_dependency_recipe` is `v` (which is `dependency_node_idx` here)
            // So, we are looking for recipes `u` that have `dependency_fqn_str` as one of their dependencies.
            for dependent_node_idx in self
                .graph
                .neighbors_directed(dependency_node_idx, Direction::Incoming)
            {
                let dependent_fqn_str = &self.graph[dependent_node_idx];
                if target_fqns.contains(dependent_fqn_str) {
                    // Both `dependent_fqn_str` and `dependency_fqn_str` are in our target set.
                    // `dependent_fqn_str` depends on `dependency_fqn_str`.

                    // Add `dependent_fqn_str` to the list of recipes that are "children" of `dependency_fqn_str`
                    // in the sense that they will be processed after `dependency_fqn_str`.
                    subgraph_adj_rev
                        .get_mut(dependency_fqn_str) // Key is the dependency
                        .unwrap() // Should exist as all target_fqns were added
                        .push(dependent_fqn_str.clone()); // Value is the recipe that depends on it

                    // Increment in-degree of the `dependent_fqn_str` because it has `dependency_fqn_str` as a dependency.
                    *subgraph_in_degree.get_mut(dependent_fqn_str).unwrap() += 1;
                }
            }
        }

        // Kahn's algorithm for topological sorting.
        let mut queue: VecDeque<String> = VecDeque::new();
        for (fqn, degree) in &subgraph_in_degree {
            if *degree == 0 {
                // Recipes with no dependencies *within the subgraph* are starting points.
                queue.push_back(fqn.clone());
            }
        }

        let mut result_levels_fqns: Vec<Vec<String>> = Vec::new();
        let mut processed_count = 0;

        while !queue.is_empty() {
            let mut current_level_fqns: Vec<String> = queue.drain(..).collect();
            // Sort FQNs at the current level for deterministic output order.
            current_level_fqns.sort();

            if !current_level_fqns.is_empty() {
                result_levels_fqns.push(current_level_fqns.clone()); // Store the FQNs for this level

                for fqn_processed in current_level_fqns {
                    processed_count += 1;

                    // For each recipe (`dependent_fqn`) that depended on `fqn_processed`,
                    // decrement its in-degree because `fqn_processed` is now "executed".
                    if let Some(dependents) = subgraph_adj_rev.get(&fqn_processed) {
                        for dependent_fqn in dependents {
                            // `dependent_fqn` is a recipe that has `fqn_processed` as a dependency.
                            // All FQNs in `subgraph_adj_rev` are already confirmed to be in `target_fqns`.
                            let degree = subgraph_in_degree.get_mut(dependent_fqn).unwrap();
                            *degree -= 1;
                            if *degree == 0 {
                                // If all dependencies of `dependent_fqn` (within the subgraph) are processed,
                                // add it to the queue for the next level.
                                queue.push_back(dependent_fqn.clone());
                            }
                        }
                    }
                }
            }
        }

        // Final check for cycles or unprocessed recipes.
        if processed_count != target_fqns.len() {
            let mut remaining_with_deps: Vec<String> = Vec::new();
            for (fqn, degree) in subgraph_in_degree {
                if degree > 0 {
                    remaining_with_deps.push(fqn);
                }
            }
            bail!(
                "Execution Order: Could not determine a valid execution order for all targeted recipes (within the graph component). \\
                Processed FQNs: {}, Expected FQNs: {}. This usually indicates a circular dependency \\
                among the following FQNs (or their dependencies within the target set): {:?}. \\
                Please check your recipe dependencies.",
                processed_count,
                target_fqns.len(),
                remaining_with_deps
            );
        }

        Ok(result_levels_fqns)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{Cookbook, Recipe};
    use std::collections::BTreeMap;

    /// Helper function to set up a RecipeDependencyGraph and a BTreeMap of cookbooks
    /// with a single cookbook named "test_cookbook" containing the provided recipes.
    fn setup_single_cookbook_test_environment(
        recipes_for_test_cookbook: Vec<Recipe>,
    ) -> (RecipeDependencyGraph, BTreeMap<String, Cookbook>) {
        let graph_data = RecipeDependencyGraph::new();
        let mut cookbooks = BTreeMap::new();

        if !recipes_for_test_cookbook.is_empty() {
            let (cookbook_name, cookbook) =
                create_cookbook("test_cookbook", recipes_for_test_cookbook);
            cookbooks.insert(cookbook_name, cookbook);
        }
        (graph_data, cookbooks)
    }

    /// Helper function to create a dummy `Recipe` for testing.
    ///
    /// # Arguments
    ///
    /// * `name`: The name of the recipe.
    /// * `dependencies`: A vector of FQNs that this recipe depends on.
    ///
    /// # Returns
    ///
    /// A `Recipe` instance. The FQN for the recipe is constructed as "test_cookbook:{name}".
    fn create_recipe(name: &str, dependencies: Vec<&str>) -> Recipe {
        Recipe {
            name: name.to_string(),
            // Assuming full_name() would produce something like "cookbook_name.recipe_name".
            // For simplicity in tests, we'll use "test_cookbook" as the cookbook name.
            cookbook: "test_cookbook".to_string(),
            dependencies: Some(dependencies.into_iter().map(|s| s.to_string()).collect()),
            config_path: format!("dummy_path/{name}.yaml").into(),
            ..Default::default()
        }
    }

    /// Helper function to create a dummy `Cookbook` for testing.
    ///
    /// # Arguments
    ///
    /// * `name`: The name of the cookbook.
    /// * `recipes`: A vector of `Recipe` instances to include in this cookbook.
    ///
    /// # Returns
    ///
    /// A tuple containing the cookbook name (String) and the `Cookbook` instance.
    fn create_cookbook(name: &str, recipes: Vec<Recipe>) -> (String, Cookbook) {
        let mut recipe_map = BTreeMap::new();
        for recipe in recipes {
            recipe_map.insert(recipe.name.clone(), recipe);
        }
        (
            name.to_string(),
            Cookbook {
                name: name.to_string(),
                recipes: recipe_map,
                // Other Cookbook fields can be defaulted if not relevant to graph tests.
                ..Default::default()
            },
        )
    }

    /// Tests that a newly created `RecipeDependencyGraph` is empty.
    #[test]
    fn test_new_graph_is_empty() {
        let graph_data = RecipeDependencyGraph::new();
        assert_eq!(
            graph_data.graph.node_count(),
            0,
            "New graph should have 0 nodes."
        );
        assert_eq!(
            graph_data.graph.edge_count(),
            0,
            "New graph should have 0 edges."
        );
        assert!(
            graph_data.fqn_to_node_index.is_empty(),
            "New graph's FQN map should be empty."
        );
    }

    /// Tests `populate_from_cookbooks` with a simple Directed Acyclic Graph (DAG).
    /// Verifies correct node and edge counts, and FQN to NodeIndex mapping.
    #[test]
    fn test_populate_from_cookbooks_simple_dag() {
        let recipes = vec![
            create_recipe("a", vec![]),
            create_recipe("b", vec!["test_cookbook:a"]),
            create_recipe("c", vec!["test_cookbook:a", "test_cookbook:b"]),
        ];
        let (mut graph_data, cookbooks) = setup_single_cookbook_test_environment(recipes);

        let result = graph_data.populate_from_cookbooks(&cookbooks);
        assert!(result.is_ok(), "Population should succeed for a valid DAG.");

        assert_eq!(
            graph_data.graph.node_count(),
            3,
            "Should have 3 nodes (a, b, c)."
        );
        assert_eq!(
            graph_data.graph.edge_count(),
            3,
            "Should have 3 edges (b->a, c->a, c->b)."
        );

        assert!(graph_data.fqn_to_node_index.contains_key("test_cookbook:a"));
        assert!(graph_data.fqn_to_node_index.contains_key("test_cookbook:b"));
        assert!(graph_data.fqn_to_node_index.contains_key("test_cookbook:c"));

        let idx_a = graph_data.fqn_to_node_index["test_cookbook:a"];
        let idx_b = graph_data.fqn_to_node_index["test_cookbook:b"];
        let idx_c = graph_data.fqn_to_node_index["test_cookbook:c"];

        assert!(
            graph_data.graph.contains_edge(idx_b, idx_a),
            "Edge b -> a should exist."
        );
        assert!(
            graph_data.graph.contains_edge(idx_c, idx_a),
            "Edge c -> a should exist."
        );
        assert!(
            graph_data.graph.contains_edge(idx_c, idx_b),
            "Edge c -> b should exist."
        );
    }

    /// Tests that `populate_from_cookbooks` correctly detects and reports a missing dependency.
    #[test]
    fn test_populate_detects_missing_dependency() {
        let recipes = vec![create_recipe("a", vec!["test_cookbook:non_existent"])];
        let (mut graph_data, cookbooks) = setup_single_cookbook_test_environment(recipes);

        let result = graph_data.populate_from_cookbooks(&cookbooks);
        assert!(
            result.is_err(),
            "Population should fail due to missing dependency."
        );
        let error_message = result.err().unwrap().to_string();
        assert!(
            error_message.contains("Recipe dependency errors found"),
            "Error message should indicate dependency errors."
        );
        assert!(
            error_message.contains("depends on 'test_cookbook:non_existent', which was not found"),
            "Error message should detail the missing dependency."
        );
    }

    /// Tests that `populate_from_cookbooks` correctly detects a direct cycle (e.g., a -> b, b -> a).
    #[test]
    fn test_populate_detects_direct_cycle() {
        let recipes = vec![
            create_recipe("a", vec!["test_cookbook:b"]),
            create_recipe("b", vec!["test_cookbook:a"]),
        ];
        let (mut graph_data, cookbooks) = setup_single_cookbook_test_environment(recipes);

        let result = graph_data.populate_from_cookbooks(&cookbooks);
        assert!(
            result.is_err(),
            "Population should fail due to a direct cycle."
        );
        let error_message = result.err().unwrap().to_string();
        assert!(
            error_message.contains("Circular dependencies detected"),
            "Error message should indicate circular dependencies."
        );
        // The order in the cycle path might vary, so check for both participants.
        assert!(
            error_message.contains("test_cookbook:a"),
            "Error message should mention recipe 'a'."
        );
        assert!(
            error_message.contains("test_cookbook:b"),
            "Error message should mention recipe 'b'."
        );
    }

    /// Tests that `populate_from_cookbooks` correctly detects an indirect cycle (e.g., a -> b -> c -> a).
    #[test]
    fn test_populate_detects_indirect_cycle() {
        let recipes = vec![
            create_recipe("a", vec!["test_cookbook:b"]),
            create_recipe("b", vec!["test_cookbook:c"]),
            create_recipe("c", vec!["test_cookbook:a"]),
        ];
        let (mut graph_data, cookbooks) = setup_single_cookbook_test_environment(recipes);

        let result = graph_data.populate_from_cookbooks(&cookbooks);
        assert!(
            result.is_err(),
            "Population should fail due to an indirect cycle."
        );
        let error_message = result.err().unwrap().to_string();
        assert!(
            error_message.contains("Circular dependencies detected"),
            "Error message should indicate circular dependencies."
        );
        assert!(
            error_message.contains("test_cookbook:a"),
            "Error message should mention recipe 'a'."
        );
        assert!(
            error_message.contains("test_cookbook:b"),
            "Error message should mention recipe 'b'."
        );
        assert!(
            error_message.contains("test_cookbook:c"),
            "Error message should mention recipe 'c'."
        );
    }

    /// Tests that `populate_from_cookbooks` correctly detects a self-dependency cycle (e.g., a -> a).
    #[test]
    fn test_populate_self_dependency_cycle() {
        let recipes = vec![create_recipe("a", vec!["test_cookbook:a"])];
        let (mut graph_data, cookbooks) = setup_single_cookbook_test_environment(recipes);

        let result = graph_data.populate_from_cookbooks(&cookbooks);
        assert!(
            result.is_err(),
            "Population should fail due to a self-dependency cycle."
        );
        let error_message = result.err().unwrap().to_string();
        assert!(
            error_message.contains("Circular dependencies detected"),
            "Error message should indicate circular dependencies."
        );
        assert!(
            error_message.contains("Cycle detected: test_cookbook:a"),
            "Error message should specify the self-cycle on 'a'."
        );
    }

    /// Tests `get_all_dependencies_for` a recipe with no dependencies.
    /// Expects an empty set of dependencies.
    #[test]
    fn test_get_all_dependencies_for_no_dependencies() {
        let recipes = vec![create_recipe("a", vec![])]; // 'a' has no dependencies
        let (mut graph_data, cookbooks) = setup_single_cookbook_test_environment(recipes);

        graph_data.populate_from_cookbooks(&cookbooks).unwrap();

        let deps = graph_data.get_all_dependencies_for("test_cookbook:a");
        assert!(deps.is_empty(), "Dependencies for 'a' should be empty.");
    }

    /// Tests `get_all_dependencies_for` a recipe with only direct dependencies.
    #[test]
    fn test_get_all_dependencies_for_direct_dependencies() {
        let recipes = vec![
            create_recipe("a", vec![]),
            create_recipe("b", vec!["test_cookbook:a"]), // 'b' depends on 'a'
        ];
        let (mut graph_data, cookbooks) = setup_single_cookbook_test_environment(recipes);

        graph_data.populate_from_cookbooks(&cookbooks).unwrap();

        let deps_b = graph_data.get_all_dependencies_for("test_cookbook:b");
        let expected_deps_b: HashSet<String> =
            ["test_cookbook:a"].iter().map(|s| s.to_string()).collect();
        assert_eq!(
            deps_b, expected_deps_b,
            "Dependencies for 'b' should be {{'test_cookbook:a'}}."
        );

        let deps_a = graph_data.get_all_dependencies_for("test_cookbook:a");
        assert!(deps_a.is_empty(), "Dependencies for 'a' should be empty.");
    }

    /// Tests `get_all_dependencies_for` a recipe with transitive dependencies.
    /// (e.g., c -> b -> a).
    #[test]
    fn test_get_all_dependencies_for_transitive_dependencies() {
        let recipes = vec![
            create_recipe("a", vec![]),
            create_recipe("b", vec!["test_cookbook:a"]), // b -> a
            create_recipe("c", vec!["test_cookbook:b"]), // c -> b
            create_recipe("d", vec!["test_cookbook:a"]), // d -> a
        ];
        let (mut graph_data, cookbooks) = setup_single_cookbook_test_environment(recipes);

        graph_data.populate_from_cookbooks(&cookbooks).unwrap();

        let deps_c = graph_data.get_all_dependencies_for("test_cookbook:c");
        let expected_deps_c: HashSet<String> = ["test_cookbook:a", "test_cookbook:b"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(
            deps_c, expected_deps_c,
            "Dependencies for 'c' should be {{'test_cookbook:a', 'test_cookbook:b'}}."
        );

        let deps_d = graph_data.get_all_dependencies_for("test_cookbook:d");
        let expected_deps_d: HashSet<String> =
            ["test_cookbook:a"].iter().map(|s| s.to_string()).collect();
        assert_eq!(
            deps_d, expected_deps_d,
            "Dependencies for 'd' should be {{'test_cookbook:a'}}."
        );
    }

    /// Tests `get_all_dependencies_for` a recipe FQN that does not exist in the graph.
    /// Expects an empty set of dependencies.
    #[test]
    fn test_get_all_dependencies_for_non_existent_recipe() {
        let graph_data = RecipeDependencyGraph::new();
        // No population needed as we are testing a non-existent FQN on an empty graph.
        let deps = graph_data.get_all_dependencies_for("test_cookbook:non_existent");
        assert!(
            deps.is_empty(),
            "Dependencies for a non-existent recipe should be empty."
        );
    }

    /// Tests `get_all_dependencies_for` when there are multiple paths to the same dependency.
    /// (e.g., d -> b -> a, d -> c -> a). Ensures the dependency ('a') is listed only once.
    #[test]
    fn test_get_all_dependencies_for_multiple_paths_to_same_dependency() {
        let recipes = vec![
            create_recipe("a", vec![]),
            create_recipe("b", vec!["test_cookbook:a"]), // b -> a
            create_recipe("c", vec!["test_cookbook:a"]), // c -> a
            create_recipe("d", vec!["test_cookbook:b", "test_cookbook:c"]), // d -> b, d -> c
        ];
        let (mut graph_data, cookbooks) = setup_single_cookbook_test_environment(recipes);

        graph_data.populate_from_cookbooks(&cookbooks).unwrap();

        let deps_d = graph_data.get_all_dependencies_for("test_cookbook:d");
        let expected_deps_d: HashSet<String> =
            ["test_cookbook:a", "test_cookbook:b", "test_cookbook:c"]
                .iter()
                .map(|s| s.to_string())
                .collect();
        assert_eq!(
            deps_d, expected_deps_d,
            "Dependencies for 'd' should include 'a', 'b', and 'c'."
        );
        assert_eq!(
            deps_d.len(),
            3,
            "Dependency 'a' should only be counted once."
        );
    }

    /// Tests `populate_from_cookbooks` with recipes from multiple cookbooks,
    /// including inter-cookbook dependencies.
    #[test]
    fn test_populate_from_cookbooks_multiple_cookbooks() {
        let mut graph_data = RecipeDependencyGraph::new();
        let mut cookbooks_map = BTreeMap::new();

        // Cookbook 1 (cb1)
        let recipe_cb1_a = Recipe {
            name: "a".to_string(),
            cookbook: "cb1".to_string(), // Explicitly set cookbook name
            dependencies: None,
            config_path: "cb1/a.yaml".into(),
            ..Default::default()
        };
        let recipe_cb1_b = Recipe {
            name: "b".to_string(),
            cookbook: "cb1".to_string(), // Explicitly set cookbook name
            dependencies: Some(vec!["cb1:a".to_string()]), // Depends on cb1:a
            config_path: "cb1/b.yaml".into(),
            ..Default::default()
        };
        let (cb1_name, cookbook1) =
            create_cookbook("cb1", vec![recipe_cb1_a.clone(), recipe_cb1_b.clone()]);
        cookbooks_map.insert(cb1_name, cookbook1);

        // Cookbook 2 (cb2)
        let recipe_cb2_c = Recipe {
            name: "c".to_string(),
            cookbook: "cb2".to_string(), // Explicitly set cookbook name
            dependencies: Some(vec!["cb1:b".to_string()]), // Depends on cb1:b
            config_path: "cb2/c.yaml".into(),
            ..Default::default()
        };
        let recipe_cb2_d = Recipe {
            name: "d".to_string(),
            cookbook: "cb2".to_string(), // Explicitly set cookbook name
            dependencies: None,
            config_path: "cb2/d.yaml".into(),
            ..Default::default()
        };
        let (cb2_name, cookbook2) =
            create_cookbook("cb2", vec![recipe_cb2_c.clone(), recipe_cb2_d.clone()]);
        cookbooks_map.insert(cb2_name, cookbook2);

        let result = graph_data.populate_from_cookbooks(&cookbooks_map);
        assert!(
            result.is_ok(),
            "Population should succeed with multiple cookbooks."
        );

        assert_eq!(
            graph_data.graph.node_count(),
            4,
            "Should have 4 nodes (cb1:a, cb1:b, cb2:c, cb2:d)."
        );
        // Edges: cb1:b -> cb1:a, cb2:c -> cb1:b
        assert_eq!(graph_data.graph.edge_count(), 2, "Should have 2 edges.");

        assert!(graph_data.fqn_to_node_index.contains_key("cb1:a"));
        assert!(graph_data.fqn_to_node_index.contains_key("cb1:b"));
        assert!(graph_data.fqn_to_node_index.contains_key("cb2:c"));
        assert!(graph_data.fqn_to_node_index.contains_key("cb2:d"));

        let idx_cb1_a = graph_data.fqn_to_node_index["cb1:a"];
        let idx_cb1_b = graph_data.fqn_to_node_index["cb1:b"];
        let idx_cb2_c = graph_data.fqn_to_node_index["cb2:c"];

        assert!(
            graph_data.graph.contains_edge(idx_cb1_b, idx_cb1_a),
            "Edge cb1:b -> cb1:a should exist."
        );
        assert!(
            graph_data.graph.contains_edge(idx_cb2_c, idx_cb1_b),
            "Edge cb2:c -> cb1:b should exist."
        );

        // Test transitive dependencies for cb2:c (cb2:c -> cb1:b -> cb1:a)
        let deps_cb2_c = graph_data.get_all_dependencies_for("cb2:c");
        let expected_deps_cb2_c: HashSet<String> =
            ["cb1:a", "cb1:b"].iter().map(|s| s.to_string()).collect();
        assert_eq!(
            deps_cb2_c, expected_deps_cb2_c,
            "Dependencies for 'cb2:c' should be {{'cb1:a', 'cb1:b'}}."
        );
    }

    // Helper to create a graph for get_execution_order_for_targets tests.
    // Edges are (source_fqn, dependency_fqn).
    fn create_graph_for_exec_order_tests(
        nodes: Vec<&str>,
        edges: Vec<(&str, &str)>,
    ) -> RecipeDependencyGraph {
        let mut graph_data = RecipeDependencyGraph::new();
        for node_fqn in nodes {
            let node_index = graph_data.graph.add_node(node_fqn.to_string());
            graph_data
                .fqn_to_node_index
                .insert(node_fqn.to_string(), node_index);
        }
        for (source_fqn, dep_fqn) in edges {
            let source_idx = graph_data.fqn_to_node_index[source_fqn];
            let dep_idx = graph_data.fqn_to_node_index[dep_fqn];
            graph_data.graph.add_edge(source_idx, dep_idx, ());
        }
        graph_data
    }

    #[test]
    fn test_exec_order_empty_targets() {
        let graph = create_graph_for_exec_order_tests(vec!["a"], vec![]);
        let targets = HashSet::new();
        let result = graph.get_execution_order_for_targets(&targets).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_exec_order_single_target_no_deps() {
        let graph = create_graph_for_exec_order_tests(vec!["a"], vec![]);
        let targets = ["a".to_string()].iter().cloned().collect();
        let result = graph.get_execution_order_for_targets(&targets).unwrap();
        assert_eq!(result, vec![vec!["a".to_string()]]);
    }

    #[test]
    fn test_exec_order_multiple_targets_no_deps_among_them() {
        let graph = create_graph_for_exec_order_tests(vec!["a", "b", "c"], vec![]);
        let targets = ["a".to_string(), "b".to_string(), "c".to_string()]
            .iter()
            .cloned()
            .collect();
        let mut result = graph.get_execution_order_for_targets(&targets).unwrap();
        // Result is Vec<Vec<String>>, inner vecs are sorted, outer vec order depends on initial queue population (BTreeMap based)
        // For no dependencies, all should be in the first level.
        assert_eq!(result.len(), 1);
        result[0].sort(); // Ensure consistent order for assertion
        assert_eq!(
            result[0],
            vec!["a".to_string(), "b".to_string(), "c".to_string()]
        );
    }

    #[test]
    fn test_exec_order_simple_linear_dependency() {
        // a -> b (a depends on b)
        let graph = create_graph_for_exec_order_tests(vec!["a", "b"], vec![("a", "b")]);
        let targets = ["a".to_string(), "b".to_string()].iter().cloned().collect();
        let result = graph.get_execution_order_for_targets(&targets).unwrap();
        assert_eq!(result, vec![vec!["b".to_string()], vec!["a".to_string()]]);
    }

    #[test]
    fn test_exec_order_longer_linear_dependency() {
        // a -> b, b -> c (a depends on b, b depends on c)
        let graph =
            create_graph_for_exec_order_tests(vec!["a", "b", "c"], vec![("a", "b"), ("b", "c")]);
        let targets = ["a".to_string(), "b".to_string(), "c".to_string()]
            .iter()
            .cloned()
            .collect();
        let result = graph.get_execution_order_for_targets(&targets).unwrap();
        assert_eq!(
            result,
            vec![
                vec!["c".to_string()],
                vec!["b".to_string()],
                vec!["a".to_string()]
            ]
        );
    }

    #[test]
    fn test_exec_order_dag_multiple_paths() {
        // d -> b, b -> a
        // d -> c, c -> a
        let graph = create_graph_for_exec_order_tests(
            vec!["a", "b", "c", "d"],
            vec![("d", "b"), ("b", "a"), ("d", "c"), ("c", "a")],
        );
        let targets = ["a", "b", "c", "d"].iter().map(|s| s.to_string()).collect();
        let result = graph.get_execution_order_for_targets(&targets).unwrap();
        // Expected: [[a], [b, c], [d]] (sorted within levels)
        assert_eq!(result.len(), 3);
        assert_eq!(result[0], vec!["a".to_string()]);
        assert_eq!(result[1], vec!["b".to_string(), "c".to_string()]); // b,c sorted
        assert_eq!(result[2], vec!["d".to_string()]);
    }

    #[test]
    fn test_exec_order_complex_dag() {
        // e -> b, e -> d
        // d -> c
        // c -> a
        // b -> a
        // f (no deps, no dependents within targets)
        let graph = create_graph_for_exec_order_tests(
            vec!["a", "b", "c", "d", "e", "f"],
            vec![("e", "b"), ("e", "d"), ("d", "c"), ("c", "a"), ("b", "a")],
        );
        let targets = ["a", "b", "c", "d", "e", "f"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let result = graph.get_execution_order_for_targets(&targets).unwrap();
        // Expected: [[a, f], [b, c], [d], [e]] (sorted within levels)
        assert_eq!(result.len(), 4);
        assert_eq!(result[0], vec!["a".to_string(), "f".to_string()]);
        assert_eq!(result[1], vec!["b".to_string(), "c".to_string()]);
        assert_eq!(result[2], vec!["d".to_string()]);
        assert_eq!(result[3], vec!["e".to_string()]);
    }

    #[test]
    fn test_exec_order_target_fqn_not_in_graph() {
        let graph = create_graph_for_exec_order_tests(vec!["a"], vec![]);
        let targets = ["a".to_string(), "non_existent".to_string()]
            .iter()
            .cloned()
            .collect();
        let result = graph.get_execution_order_for_targets(&targets);
        assert!(result.is_err());
        assert!(
            result
                .err()
                .unwrap()
                .to_string()
                .contains("Recipe FQN 'non_existent' targeted for execution not found")
        );
    }

    #[test]
    fn test_exec_order_cycle_direct() {
        // a -> b, b -> a
        let graph = create_graph_for_exec_order_tests(vec!["a", "b"], vec![("a", "b"), ("b", "a")]);
        let targets = ["a".to_string(), "b".to_string()].iter().cloned().collect();
        let result = graph.get_execution_order_for_targets(&targets);
        assert!(result.is_err());
        let err_msg = result.err().unwrap().to_string();
        assert!(err_msg.contains("circular dependency"));
        // Check that both a and b are mentioned as part of the remaining items with dependencies
        assert!(err_msg.contains("\"a\""));
        assert!(err_msg.contains("\"b\""));
    }

    #[test]
    fn test_exec_order_cycle_indirect() {
        // a -> b, b -> c, c -> a
        let graph = create_graph_for_exec_order_tests(
            vec!["a", "b", "c"],
            vec![("a", "b"), ("b", "c"), ("c", "a")],
        );
        let targets = ["a".to_string(), "b".to_string(), "c".to_string()]
            .iter()
            .cloned()
            .collect();
        let result = graph.get_execution_order_for_targets(&targets);
        assert!(result.is_err());
        let err_msg = result.err().unwrap().to_string();
        assert!(err_msg.contains("circular dependency"));
        assert!(err_msg.contains("\"a\""));
        assert!(err_msg.contains("\"b\""));
        assert!(err_msg.contains("\"c\""));
    }

    #[test]
    fn test_exec_order_self_cycle_within_targets() {
        // a -> a
        let graph = create_graph_for_exec_order_tests(vec!["a"], vec![("a", "a")]);
        let targets = ["a".to_string()].iter().cloned().collect();
        let result = graph.get_execution_order_for_targets(&targets);
        assert!(result.is_err());
        let err_msg = result.err().unwrap().to_string();
        assert!(err_msg.contains("circular dependency"));
        assert!(err_msg.contains("\"a\""));
    }

    #[test]
    fn test_exec_order_disconnected_components_in_targets() {
        // Component 1: a -> b
        // Component 2: c -> d
        // Component 3: e (isolated)
        let graph = create_graph_for_exec_order_tests(
            vec!["a", "b", "c", "d", "e"],
            vec![("a", "b"), ("c", "d")],
        );
        let targets = ["a", "b", "c", "d", "e"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let result = graph.get_execution_order_for_targets(&targets).unwrap();
        // Expected: [[b, d, e], [a, c]] (sorted within levels)
        assert_eq!(result.len(), 2);
        assert_eq!(
            result[0],
            vec!["b".to_string(), "d".to_string(), "e".to_string()]
        );
        assert_eq!(result[1], vec!["a".to_string(), "c".to_string()]);
    }

    #[test]
    fn test_exec_order_targets_are_subset_of_graph() {
        // Full graph: a -> b, b -> c, c -> d
        // Targets: b, c
        // Expected for targets: [[c], [b]]
        let graph = create_graph_for_exec_order_tests(
            vec!["a", "b", "c", "d"],
            vec![("a", "b"), ("b", "c"), ("c", "d")],
        );

        // Test 1: Targets {b, c, d} where b->c, c->d
        // Expected: [[d], [c], [b]]
        let targets_bcd = ["b".to_string(), "c".to_string(), "d".to_string()]
            .iter()
            .cloned()
            .collect();
        let result_bcd = graph.get_execution_order_for_targets(&targets_bcd).unwrap();
        assert_eq!(
            result_bcd,
            vec![
                vec!["d".to_string()],
                vec!["c".to_string()],
                vec!["b".to_string()]
            ]
        );

        // Test 2: Targets {b, c} where b->c, and c depends on d (d is NOT in targets)
        // In this scenario, 'c' has no dependencies *within the target set {b,c}*.
        // 'b' depends on 'c' (which is in the target set).
        // Expected: [[c], [b]]
        let targets_bc = ["b".to_string(), "c".to_string()].iter().cloned().collect();
        let result_bc = graph.get_execution_order_for_targets(&targets_bc).unwrap();
        assert_eq!(
            result_bc,
            vec![vec!["c".to_string()], vec!["b".to_string()]]
        );

        // Test 3: Targets {a, b} where a->b, and b depends on c (c is NOT in targets)
        // 'b' has no dependencies *within the target set {a,b}*.
        // 'a' depends on 'b' (which is in the target set).
        // Expected: [[b], [a]]
        let targets_ab = ["a".to_string(), "b".to_string()].iter().cloned().collect();
        let result_ab = graph.get_execution_order_for_targets(&targets_ab).unwrap();
        assert_eq!(
            result_ab,
            vec![vec!["b".to_string()], vec!["a".to_string()]]
        );
    }

    #[test]
    fn test_exec_order_diamond_dependency() {
        // a -> b, a -> c, b -> d, c -> d
        let graph = create_graph_for_exec_order_tests(
            vec!["a", "b", "c", "d"],
            vec![("a", "b"), ("a", "c"), ("b", "d"), ("c", "d")],
        );
        let targets = ["a", "b", "c", "d"].iter().map(|s| s.to_string()).collect();
        let result = graph.get_execution_order_for_targets(&targets).unwrap();
        // Expected: [[d], [b, c], [a]] (sorted within levels)
        assert_eq!(result.len(), 3);
        assert_eq!(result[0], vec!["d".to_string()]);
        assert_eq!(result[1], vec!["b".to_string(), "c".to_string()]);
        assert_eq!(result[2], vec!["a".to_string()]);
    }

    // --- Tests for get_execution_plan_for_initial_targets ---

    #[test]
    fn test_plan_empty_initial_targets() {
        let graph = create_graph_for_exec_order_tests(vec!["a"], vec![]);
        let initial_targets = HashSet::new();
        let result = graph
            .get_execution_plan_for_initial_targets(&initial_targets)
            .unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_plan_single_initial_target_no_deps() {
        let graph = create_graph_for_exec_order_tests(vec!["a"], vec![]);
        let initial_targets = ["a".to_string()].iter().cloned().collect();
        let result = graph
            .get_execution_plan_for_initial_targets(&initial_targets)
            .unwrap();
        assert_eq!(result, vec![vec!["a".to_string()]]);
    }

    #[test]
    fn test_plan_initial_target_with_direct_deps() {
        // Graph: b -> a, c -> a (b and c depend on a)
        // Initial: {"b"}
        // Expected plan: [[a], [b]]
        let graph =
            create_graph_for_exec_order_tests(vec!["a", "b", "c"], vec![("b", "a"), ("c", "a")]);
        let initial_targets = ["b".to_string()].iter().cloned().collect();
        let result = graph
            .get_execution_plan_for_initial_targets(&initial_targets)
            .unwrap();
        assert_eq!(result, vec![vec!["a".to_string()], vec!["b".to_string()]]);
    }

    #[test]
    fn test_plan_initial_target_with_transitive_deps() {
        // Graph: c -> b, b -> a
        // Initial: {"c"}
        // Expected plan: [[a], [b], [c]]
        let graph =
            create_graph_for_exec_order_tests(vec!["a", "b", "c"], vec![("c", "b"), ("b", "a")]);
        let initial_targets = ["c".to_string()].iter().cloned().collect();
        let result = graph
            .get_execution_plan_for_initial_targets(&initial_targets)
            .unwrap();
        assert_eq!(
            result,
            vec![
                vec!["a".to_string()],
                vec!["b".to_string()],
                vec!["c".to_string()]
            ]
        );
    }

    #[test]
    fn test_plan_multiple_initial_targets_shared_deps() {
        // Graph: b -> a, c -> a, d -> c
        // Initial: {"b", "d"}
        // Expected plan: [[a], [b, c], [d]] (b and c are on the same level after a)
        let graph = create_graph_for_exec_order_tests(
            vec!["a", "b", "c", "d"],
            vec![("b", "a"), ("c", "a"), ("d", "c")],
        );
        let initial_targets = ["b".to_string(), "d".to_string()].iter().cloned().collect();
        let result = graph
            .get_execution_plan_for_initial_targets(&initial_targets)
            .unwrap();
        assert_eq!(result.len(), 3);
        assert_eq!(result[0], vec!["a".to_string()]);
        assert_eq!(result[1], vec!["b".to_string(), "c".to_string()]); // Sorted
        assert_eq!(result[2], vec!["d".to_string()]);
    }

    #[test]
    fn test_plan_multiple_initial_targets_independent_and_dependent() {
        // Graph: b -> a, d -> c, e (isolated)
        // Initial: {"b", "d", "e"}
        // Expected plan: [[a, c, e], [b, d]] (sorted within levels)
        let graph = create_graph_for_exec_order_tests(
            vec!["a", "b", "c", "d", "e"],
            vec![("b", "a"), ("d", "c")],
        );
        let initial_targets = ["b".to_string(), "d".to_string(), "e".to_string()]
            .iter()
            .cloned()
            .collect();
        let result = graph
            .get_execution_plan_for_initial_targets(&initial_targets)
            .unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(
            result[0],
            vec!["a".to_string(), "c".to_string(), "e".to_string()] // Sorted
        );
        assert_eq!(result[1], vec!["b".to_string(), "d".to_string()]); // Sorted
    }

    #[test]
    fn test_plan_initial_target_not_in_graph() {
        let graph = create_graph_for_exec_order_tests(vec!["a"], vec![]);
        let initial_targets = ["non_existent".to_string()].iter().cloned().collect();
        let result = graph.get_execution_plan_for_initial_targets(&initial_targets);
        assert!(result.is_err());
        assert!(
            result
                .err()
                .unwrap()
                .to_string()
                .contains("Initial target recipe FQN 'non_existent' not found")
        );
    }

    #[test]
    fn test_plan_dependency_cycle_from_initial_target() {
        // Graph: a -> b, b -> c, c -> a (cycle)
        // Initial: {"a"}
        // Expected: Error due to cycle
        let graph = create_graph_for_exec_order_tests(
            vec!["a", "b", "c"],
            vec![("a", "b"), ("b", "c"), ("c", "a")],
        );
        let initial_targets = ["a".to_string()].iter().cloned().collect();
        let result = graph.get_execution_plan_for_initial_targets(&initial_targets);
        assert!(result.is_err());
        let err_msg = result.err().unwrap().to_string();
        assert!(err_msg.contains("circular dependency"));
    }

    #[test]
    fn test_plan_initial_target_is_part_of_cycle_but_not_entry() {
        // Graph: a -> b, b -> c, c -> a (cycle)
        // Initial: {"b"}
        // Expected: Error due to cycle (b's dependencies include a and c, which form the cycle)
        let graph = create_graph_for_exec_order_tests(
            vec!["a", "b", "c"],
            vec![("a", "b"), ("b", "c"), ("c", "a")],
        );
        let initial_targets = ["b".to_string()].iter().cloned().collect();
        let result = graph.get_execution_plan_for_initial_targets(&initial_targets);
        assert!(result.is_err());
        let err_msg = result.err().unwrap().to_string();
        assert!(err_msg.contains("circular dependency"));
    }

    #[test]
    fn test_plan_initial_target_with_dependency_not_in_graph_indirectly() {
        // Graph: a -> b, b -> non_existent_dep (This setup is tricky with current helpers)
        // The create_graph_for_exec_order_tests assumes all nodes in edges are pre-declared.
        // populate_from_cookbooks would catch b -> non_existent_dep earlier.
        // get_all_dependencies_for for 'b' would return an empty set for non_existent_dep if it's not in fqn_to_node_index.
        // So, if 'b' depends on 'non_existent_dep', and 'non_existent_dep' is not a node,
        // get_all_dependencies_for("b") will not include it.
        // This means get_execution_plan_for_initial_targets relies on populate_from_cookbooks
        // to have already validated all direct dependencies.

        // Let's test the scenario where an initial target is valid, but one of its *transitive* dependencies
        // was somehow not added to the graph as a node, which populate_from_cookbooks should prevent.
        // This test is more about the robustness of get_all_dependencies_for if the graph is inconsistent.

        // For this test, we manually construct a graph where 'b' exists as a node,
        // but its dependency 'non_existent_dep' does not.
        let mut graph = RecipeDependencyGraph::new();
        let idx_a = graph.graph.add_node("a".to_string());
        graph.fqn_to_node_index.insert("a".to_string(), idx_a);
        let idx_b = graph.graph.add_node("b".to_string());
        graph.fqn_to_node_index.insert("b".to_string(), idx_b);
        // Edge a -> b (a depends on b)
        graph.graph.add_edge(idx_a, idx_b, ());
        // 'b' is supposed to depend on 'non_existent_dep', but 'non_existent_dep' is not a node.
        // get_all_dependencies_for("a") will find {"b"}.
        // get_execution_order_for_targets({"a", "b"}) will then run.

        let initial_targets = ["a".to_string()].iter().cloned().collect();
        let result = graph
            .get_execution_plan_for_initial_targets(&initial_targets)
            .unwrap();
        // Expected: [[b], [a]] because 'non_existent_dep' is silently ignored by get_all_dependencies_for
        // if it's not in fqn_to_node_index.
        assert_eq!(result, vec![vec!["b".to_string()], vec!["a".to_string()]]);

        // A more realistic error for missing transitive deps would be caught by populate_from_cookbooks.
        // If populate_from_cookbooks allowed a recipe 'b' to list 'non_existent_dep' as a dependency,
        // it would error out there.
    }
}
