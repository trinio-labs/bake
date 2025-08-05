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
