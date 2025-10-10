use console::style;

use crate::project::Recipe;

/// Display the full execution plan with all details
pub fn display_full_execution_plan(execution_plan: &[Vec<Recipe>]) -> anyhow::Result<()> {
    if execution_plan.is_empty() {
        return Ok(());
    }

    // Header
    println!("\n{}", style("🍰 Execution Plan").bold().cyan());
    println!("{}", style("━".repeat(50)).cyan());

    // Summary
    let total_recipes: usize = execution_plan.iter().map(|level| level.len()).sum();
    let max_parallel = execution_plan
        .iter()
        .map(|level| level.len())
        .max()
        .unwrap_or(0);

    println!("\n{}", style("📋 Summary:").bold().blue());
    println!("  • Total recipes: {total_recipes}");
    println!("  • Execution levels: {}", execution_plan.len());
    println!("  • Max parallel recipes: {max_parallel}");

    // Execution Order
    println!("\n{}", style("🔄 Execution Order:").bold().green());

    display_tree_execution_order(execution_plan)?;
    println!();

    Ok(())
}

/// Display execution order as a tree where levels are properly indented
fn display_tree_execution_order(execution_plan: &[Vec<Recipe>]) -> anyhow::Result<()> {
    if execution_plan.is_empty() {
        return Ok(());
    }

    for (level_idx, level) in execution_plan.iter().enumerate() {
        // Calculate indentation based on level
        let recipe_indent = if level_idx == 0 {
            "".to_string()
        } else {
            " ".repeat(level_idx * 3)
        };

        // Print recipes for this level
        for (recipe_idx, recipe) in level.iter().enumerate() {
            let is_last_in_level = recipe_idx == level.len() - 1;

            // Tree connector: ├─ for non-last, └─ for last
            let connector = if is_last_in_level {
                "└─ "
            } else {
                "├─ "
            };

            println!("{}{}{}", recipe_indent, connector, recipe.full_name());
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::Recipe;
    use indexmap::IndexMap;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    fn create_test_recipe(cookbook: &str, name: &str) -> Recipe {
        Recipe {
            name: name.to_string(),
            cookbook: cookbook.to_string(),
            project_root: PathBuf::from("/test"),
            description: Some(format!("Test recipe {name}")),
            tags: vec![],
            dependencies: None,
            cache: Default::default(),
            environment: vec![],
            variables: IndexMap::new(),
            overrides: BTreeMap::new(),
            processed_variables: IndexMap::new(),
            run: format!("echo 'Running {name}'"),
            run_status: Default::default(),
            config_path: PathBuf::from(format!("{cookbook}.yml")),
            template: None,
            parameters: BTreeMap::new(),
        }
    }

    #[test]
    fn test_display_empty_execution_plan() {
        let empty_plan: Vec<Vec<Recipe>> = vec![];
        let result = display_full_execution_plan(&empty_plan);
        assert!(result.is_ok());
    }

    #[test]
    fn test_display_single_recipe_plan() {
        let plan = vec![vec![create_test_recipe("app", "build")]];
        let result = display_full_execution_plan(&plan);
        assert!(result.is_ok());
    }

    #[test]
    fn test_display_multi_level_plan() {
        let plan = vec![
            vec![create_test_recipe("app", "install")],
            vec![
                create_test_recipe("app", "build"),
                create_test_recipe("app", "lint"),
            ],
            vec![create_test_recipe("app", "test")],
        ];
        let result = display_full_execution_plan(&plan);
        assert!(result.is_ok());
    }

    #[test]
    fn test_display_parallel_recipes() {
        let plan = vec![vec![
            create_test_recipe("frontend", "build"),
            create_test_recipe("backend", "build"),
            create_test_recipe("docs", "build"),
        ]];
        let result = display_full_execution_plan(&plan);
        assert!(result.is_ok());
    }

    #[test]
    fn test_tree_display_empty_plan() {
        let empty_plan: Vec<Vec<Recipe>> = vec![];
        let result = display_tree_execution_order(&empty_plan);
        assert!(result.is_ok());
    }

    #[test]
    fn test_execution_plan_statistics() {
        let plan = [
            vec![create_test_recipe("app", "install")],
            vec![
                create_test_recipe("app", "build"),
                create_test_recipe("app", "lint"),
            ],
            vec![create_test_recipe("app", "test")],
        ];

        // Test total recipe count
        let total_recipes: usize = plan.iter().map(|level| level.len()).sum();
        assert_eq!(total_recipes, 4);

        // Test max parallel count
        let max_parallel = plan.iter().map(|level| level.len()).max().unwrap_or(0);
        assert_eq!(max_parallel, 2);

        // Test level count
        assert_eq!(plan.len(), 3);
    }

    #[test]
    fn test_complex_execution_plan() {
        let plan = vec![
            // Level 0: Initial setup
            vec![
                create_test_recipe("shared", "setup"),
                create_test_recipe("db", "migrate"),
            ],
            // Level 1: Parallel builds
            vec![
                create_test_recipe("frontend", "build"),
                create_test_recipe("backend", "build"),
                create_test_recipe("api", "build"),
            ],
            // Level 2: Tests
            vec![
                create_test_recipe("frontend", "test"),
                create_test_recipe("backend", "test"),
            ],
            // Level 3: Integration
            vec![create_test_recipe("integration", "test")],
        ];

        let result = display_full_execution_plan(&plan);
        assert!(result.is_ok());

        // Verify statistics
        let total_recipes: usize = plan.iter().map(|level| level.len()).sum();
        assert_eq!(total_recipes, 8);

        let max_parallel = plan.iter().map(|level| level.len()).max().unwrap_or(0);
        assert_eq!(max_parallel, 3);
    }
}
