// #![feature(coverage_attribute)]

// Re-export all modules for external use
pub mod baker;
pub mod cache;
pub mod execution_plan;
pub mod project;
pub mod template;
pub mod update;

// Re-export commonly used types for convenience
pub use cache::CacheBuilder;
pub use project::BakeProject;
pub use update::{check_for_updates, perform_self_update};

use anyhow::{bail, Context};
use clap::Parser;
use console::Term;
use env_logger::Env;
use indexmap::IndexMap;
use std::sync::Arc;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const DEFAULT_LOG_LEVEL: &str = "warn";
const WELCOME_MSG: &str = "
┌───────────────────────────┐
│                           │
│     🍪 Let's Bake! 🍪     │
│         vxx.xx.xx         │
│                           │
└───────────────────────────┘
";

/// Bake is a build system that runs and caches tasks based on yaml configuration
/// files.
///
/// For more information and documentation visit: https://github.com/theoribeiro/bake
///
#[derive(Parser, Debug)]
#[command(version, about, long_about)]
pub struct Args {
    /// Recipe to bake. Use:{n}{n}
    /// <cookbook>:<recipe>  - for a cookbook's recipe{n}
    /// <cookbook>:          - for all recipes in a cookbook{n}
    /// :<recipe>            - for all recipes with that name across all cookbooks{n}
    /// By default, cookbook and recipe names are matched exactly.{n}
    /// Use --regex flag to enable regex pattern matching.{n}
    pub recipe: Option<String>,

    /// Path fo config file or directory containing a bake.yml file
    #[arg(short, long)]
    pub path: Option<String>,

    /// Show execution plan only (don't execute anything)
    #[arg(short = 'e', long, alias = "explain")]
    pub show_plan: bool,

    /// Clean outputs and caches for the selected recipes
    #[arg(short, long)]
    pub clean: bool,

    /// Verbose mode (-v, -vv, -vvv, etc.)
    #[arg(short, long, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Variables to override (key=value).
    /// Variables specified here override those defined at recipe, cookbook, or project level.
    #[arg(short = 'D', long = "define", value_parser = parse_key_val, action = clap::ArgAction::Append)]
    pub vars: Vec<(String, String)>,

    /// Enable regex pattern matching for recipe filters
    #[arg(long)]
    pub regex: bool,

    /// Dry run mode - just show what would be done
    #[arg(short = 'n', long)]
    pub dry_run: bool,

    /// Fail fast on first error instead of running all possible recipes
    #[arg(short, long)]
    pub fail_fast: bool,

    /// Maximum number of recipes to execute concurrently  
    #[arg(short, long)]
    pub jobs: Option<usize>,

    /// Check for updates  
    #[arg(long)]
    pub check_updates: bool,

    /// Self update bake to the latest version
    #[arg(long)]
    pub self_update: bool,

    /// Include prerelease versions when updating
    #[arg(long)]
    pub prerelease: bool,

    /// List available templates
    #[arg(long)]
    pub list_templates: bool,

    /// Validate all templates in project
    #[arg(long)]
    pub validate_templates: bool,

    /// Print rendered cookbooks with all variables and templates resolved
    #[arg(long)]
    pub render: bool,

    /// Skip using and saving to cache
    #[arg(long)]
    pub skip_cache: bool,
}

pub fn parse_key_val(s: &str) -> anyhow::Result<(String, String)> {
    let parts: Vec<&str> = s.splitn(2, '=').collect();
    if parts.len() != 2 {
        bail!("Invalid key=value pair: {}", s);
    }
    Ok((parts[0].to_string(), parts[1].to_string()))
}

pub fn parse_variables(vars: &[(String, String)]) -> IndexMap<String, String> {
    vars.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
}

pub async fn load_project_with_feedback(
    bake_path: &std::path::Path,
    variables: IndexMap<String, String>,
    verbose: bool,
    jobs: Option<usize>,
    fail_fast: bool,
) -> anyhow::Result<Arc<BakeProject>> {
    let term = Term::stderr();
    let loading_message = format!("Loading project from {}...", bake_path.display());

    if !verbose {
        term.write_line(&loading_message)?;
    }

    let mut project = match BakeProject::from(bake_path, Some("default"), variables, verbose) {
        Ok(p) => p,
        Err(e) => {
            if !verbose {
                term.clear_line()?;
                term.move_cursor_up(1)?;
                term.clear_line()?;
            }
            bail!("Failed to load project: {}", e);
        }
    };

    // Apply configuration overrides
    if let Some(jobs) = jobs {
        project.config.max_parallel = jobs;
    }
    project.config.fast_fail = fail_fast;
    project.config.verbose = verbose;

    if !verbose {
        term.clear_line()?;
        term.move_cursor_up(1)?;
        term.clear_line()?;
    }

    // Check for updates if configured
    if project.config.update.enabled {
        let update_config = crate::update::UpdateConfig {
            enabled: project.config.update.enabled,
            check_interval_days: project.config.update.check_interval_days,
            auto_update: project.config.update.auto_update,
            prerelease: project.config.update.prerelease,
        };
        let _ = check_for_updates(&update_config, false).await;
    }

    Ok(Arc::new(project))
}

pub fn resolve_bake_path(path_arg: &Option<String>) -> anyhow::Result<std::path::PathBuf> {
    match path_arg {
        Some(path) => Ok(std::path::PathBuf::from(path)),
        None => std::env::current_dir().with_context(|| "Failed to get current directory"),
    }
}

pub async fn handle_self_update(prerelease: bool) -> anyhow::Result<()> {
    println!("Checking for updates...");

    match perform_self_update(prerelease) {
        Ok(_) => println!("Update completed successfully!"),
        Err(e) => {
            eprintln!("Update failed: {e}");
            std::process::exit(1);
        }
    }
    Ok(())
}

pub async fn handle_check_updates(prerelease: bool) -> anyhow::Result<()> {
    let config = update::UpdateConfig {
        enabled: true,
        check_interval_days: 0, // Always check
        auto_update: false,
        prerelease,
    };

    match check_for_updates(&config, true).await? {
        Some(version) => {
            println!("New version available: {version}");
            println!("Run 'bake --self-update' to update");
        }
        None => {
            println!("You are using the latest version");
        }
    }
    Ok(())
}

pub async fn handle_list_templates(args: &Args) -> anyhow::Result<()> {
    let bake_path = resolve_bake_path(&args.path)?;
    let variables = parse_variables(&args.vars);
    let project =
        load_project_with_feedback(&bake_path, variables, args.verbose > 0, None, false).await?;

    if project.template_registry.is_empty() {
        println!("No templates found in this project.");
        return Ok(());
    }

    println!("Available templates:");
    println!();

    for (name, template) in &project.template_registry {
        println!("📋 {name}");

        if let Some(description) = &template.description {
            println!("   Description: {description}");
        }

        if !template.parameters.is_empty() {
            println!("   Parameters:");
            for (param_name, param_def) in &template.parameters {
                let param_type = match param_def.parameter_type {
                    crate::project::recipe_template::ParameterType::String => "string",
                    crate::project::recipe_template::ParameterType::Number => "number",
                    crate::project::recipe_template::ParameterType::Boolean => "boolean",
                    crate::project::recipe_template::ParameterType::Array => "array",
                    crate::project::recipe_template::ParameterType::Object => "object",
                };

                let required = if param_def.required {
                    " (required)"
                } else {
                    ""
                };
                let default = if let Some(ref default_val) = param_def.default {
                    format!(" [default: {}]", serde_yaml::to_string(default_val)?.trim())
                } else {
                    String::new()
                };

                println!("     {param_name} ({param_type}){required}{default}");

                if let Some(ref desc) = param_def.description {
                    println!("       {desc}");
                }
            }
        }

        println!();
    }

    Ok(())
}

pub async fn handle_validate_templates(args: &Args) -> anyhow::Result<()> {
    let bake_path = resolve_bake_path(&args.path)?;
    let variables = parse_variables(&args.vars);
    let project =
        load_project_with_feedback(&bake_path, variables, args.verbose > 0, None, false).await?;

    if project.template_registry.is_empty() {
        println!("No templates found in this project.");
        return Ok(());
    }

    println!(
        "Validating {} templates...",
        project.template_registry.len()
    );
    println!();

    let mut all_valid = true;

    for (name, template) in &project.template_registry {
        print!("📋 {name} ... ");

        // Basic validation - check if template has required content
        if template.template_content.trim().is_empty() {
            println!("❌ FAILED - Empty template content");
            all_valid = false;
            continue;
        }

        // Try to parse the template content as YAML to catch syntax errors
        match serde_yaml::from_str::<serde_yaml::Value>(&template.template_content) {
            Ok(_) => {
                // Additional validation could go here:
                // - Check if required parameters are used in template
                // - Validate Handlebars syntax
                // - Check for circular dependencies if template has extends
                println!("✅ VALID");
            }
            Err(e) => {
                println!("❌ FAILED - Invalid YAML: {e}");
                all_valid = false;
            }
        }
    }

    println!();
    if all_valid {
        println!("✅ All templates are valid");
    } else {
        println!("❌ Some templates have validation errors");
        std::process::exit(1);
    }

    Ok(())
}

pub async fn handle_render(args: &Args) -> anyhow::Result<()> {
    let bake_path = resolve_bake_path(&args.path)?;
    let variables = parse_variables(&args.vars);
    let project =
        load_project_with_feedback(&bake_path, variables, args.verbose > 0, None, false).await?;

    // Get the execution plan to determine which recipes to show
    let recipe_filter = args.recipe.as_deref();
    let execution_plan = project.get_recipes_for_execution(recipe_filter, args.regex)?;

    // Create a set of all recipe FQNs that should be included
    let included_recipes: std::collections::HashSet<String> = execution_plan
        .iter()
        .flatten()
        .map(|recipe| format!("{}:{}", recipe.cookbook, recipe.name))
        .collect();

    // Output header with pretty styling
    println!(
        "\n{}",
        console::style("🍰 Rendered Bake Configuration")
            .bold()
            .cyan()
    );
    println!(
        "{}",
        console::style("✨ All variables and templates resolved").dim()
    );
    if let Some(filter) = recipe_filter {
        println!(
            "{} {}",
            console::style("🎯 Filter:").bold().yellow(),
            console::style(filter).bright().white()
        );
    }
    println!("{}", "━".repeat(50));

    // Create serializable structures for cookbooks and recipes
    #[derive(serde::Serialize)]
    struct RenderedCookbook {
        name: String,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        environment: Vec<String>,
        #[serde(skip_serializing_if = "IndexMap::is_empty")]
        variables: IndexMap<String, serde_yaml::Value>,
        recipes: std::collections::BTreeMap<String, RenderedRecipe>,
    }

    #[derive(serde::Serialize)]
    struct RenderedRecipe {
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[serde(skip_serializing_if = "IndexMap::is_empty")]
        variables: IndexMap<String, serde_yaml::Value>,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        environment: Vec<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        dependencies: Option<Vec<String>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        cache: Option<crate::project::RecipeCacheConfig>,
        run: String,
    }

    // Convert project to renderable structure, filtering by included recipes
    let cookbooks: std::collections::BTreeMap<String, RenderedCookbook> = project
        .cookbooks
        .iter()
        .filter_map(|(name, cookbook)| {
            // Only include recipes that are in the execution plan
            let filtered_recipes: std::collections::BTreeMap<String, RenderedRecipe> = cookbook
                .recipes
                .iter()
                .filter(|(_recipe_name, recipe)| {
                    // If no filter specified, include all recipes
                    if included_recipes.is_empty() {
                        true
                    } else {
                        included_recipes.contains(&format!("{}:{}", recipe.cookbook, recipe.name))
                    }
                })
                .map(|(recipe_name, recipe)| {
                    (
                        recipe_name.clone(),
                        RenderedRecipe {
                            description: recipe.description.clone(),
                            variables: recipe.variables.clone(),
                            environment: recipe.environment.clone(),
                            dependencies: recipe.dependencies.clone(),
                            cache: recipe.cache.clone(),
                            run: recipe.run.clone(),
                        },
                    )
                })
                .collect();

            // Only include cookbooks that have at least one recipe to show
            if filtered_recipes.is_empty() {
                None
            } else {
                Some((
                    name.clone(),
                    RenderedCookbook {
                        name: cookbook.name.clone(),
                        environment: cookbook.environment.clone(),
                        variables: cookbook.variables.clone(),
                        recipes: filtered_recipes,
                    },
                ))
            }
        })
        .collect();

    // Display project information separately
    #[derive(serde::Serialize)]
    struct ProjectInfo {
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[serde(skip_serializing_if = "IndexMap::is_empty")]
        variables: IndexMap<String, serde_yaml::Value>,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        environment: Vec<String>,
    }

    let project_info = ProjectInfo {
        name: project.name.clone(),
        description: project.description.clone(),
        variables: project.variables.clone(),
        environment: project.environment.clone(),
    };

    // Print project information with pretty header
    println!(
        "\n{}",
        console::style("📋 Project Information").bold().blue()
    );
    println!(
        "{}{}",
        console::style("└─").blue(),
        console::style("─".repeat(25)).blue()
    );
    match serde_yaml::to_string(&project_info) {
        Ok(yaml) => {
            println!("{}", yaml.trim());
        }
        Err(err) => {
            eprintln!("Error serializing project info to YAML: {err}");
            return Err(err.into());
        }
    }

    // Display each cookbook separately with pretty headers
    for (cookbook_name, cookbook) in &cookbooks {
        println!(
            "\n\n{} {}",
            console::style("📚").green(),
            console::style(&format!("Cookbook: {cookbook_name}"))
                .bold()
                .green()
        );
        println!(
            "{}{}",
            console::style("└─").green(),
            console::style("─".repeat(12 + cookbook_name.len())).green()
        );

        match serde_yaml::to_string(cookbook) {
            Ok(yaml) => {
                println!("{}", yaml.trim());
            }
            Err(err) => {
                eprintln!("Error serializing cookbook '{cookbook_name}' to YAML: {err}");
                return Err(err.into());
            }
        }
    }

    Ok(())
}

pub async fn run_bake(args: Args) -> anyhow::Result<()> {
    let bake_path = resolve_bake_path(&args.path)?;
    let variables = parse_variables(&args.vars);
    let project = load_project_with_feedback(
        &bake_path,
        variables,
        args.verbose > 0,
        args.jobs,
        args.fail_fast,
    )
    .await?;

    // Handle clean command
    if args.clean {
        let execution_plan =
            project.get_recipes_for_execution(args.recipe.as_deref(), args.regex)?;

        if execution_plan.is_empty() {
            println!("No recipes found to clean");
            return Ok(());
        }

        println!(
            "Cleaning outputs and caches for {} recipes...",
            execution_plan
                .iter()
                .map(|level| level.len())
                .sum::<usize>()
        );

        // Clean cache entries for all recipes
        let all_recipes: Vec<String> = execution_plan
            .iter()
            .flatten()
            .map(|recipe| format!("{}:{}", recipe.cookbook, recipe.name))
            .collect();

        let _cache = CacheBuilder::new(project.clone())
            .default_strategies()
            .build_for_recipes(&all_recipes)
            .await?;

        // Clean outputs and cache entries
        for recipe in execution_plan.iter().flatten() {
            let recipe_fqn = format!("{}:{}", recipe.cookbook, recipe.name);

            // Clean outputs if they exist
            if let Some(ref cache_config) = recipe.cache {
                for output in &cache_config.outputs {
                    let output_path = recipe.config_path.parent().unwrap().join(output);

                    if output_path.exists() {
                        if output_path.is_dir() {
                            std::fs::remove_dir_all(&output_path).with_context(|| {
                                format!("Failed to remove directory {}", output_path.display())
                            })?;
                        } else {
                            std::fs::remove_file(&output_path).with_context(|| {
                                format!("Failed to remove file {}", output_path.display())
                            })?;
                        }
                        println!("Cleaned: {}", output_path.display());
                    }
                }
            }

            println!("Cleaned cache for: {recipe_fqn}");
        }

        return Ok(());
    }

    // Get execution plan
    let execution_plan = project.get_recipes_for_execution(args.recipe.as_deref(), args.regex)?;

    if execution_plan.is_empty() {
        println!("No recipes to bake in the project.");
        return Ok(());
    }

    // Show execution plan if requested
    if args.show_plan {
        // Both tree and show_plan use the same display function
        execution_plan::display_full_execution_plan(&execution_plan)?;
        return Ok(());
    }

    // Dry run mode
    if args.dry_run {
        println!("Dry run mode - showing what would be executed:");
        execution_plan::display_full_execution_plan(&execution_plan)?;
        return Ok(());
    }

    // Handle skip cache option
    if args.skip_cache {
        println!("Skipping cache...");
        // Modify the project's cache configuration in place
        unsafe {
            let project_ptr = Arc::as_ptr(&project) as *mut BakeProject;
            (*project_ptr).config.cache.local.enabled = false;
            (*project_ptr).config.cache.remotes = None;
        }
    }

    // Build cache for recipes
    let all_recipes: Vec<String> = execution_plan
        .iter()
        .flatten()
        .map(|recipe| format!("{}:{}", recipe.cookbook, recipe.name))
        .collect();

    let cache = CacheBuilder::new(project.clone())
        .default_strategies()
        .build_for_recipes(&all_recipes)
        .await?;

    // Execute recipes
    baker::bake(project, cache, execution_plan, false).await
}

/// Main entry point for the library - initializes logging and runs the application
pub async fn run() -> anyhow::Result<()> {
    env_logger::Builder::from_env(Env::default().default_filter_or(DEFAULT_LOG_LEVEL)).init();

    let term = Term::stdout();
    let padded_version = format!("{VERSION:<8}");
    term.set_title("Bake");
    println!("{}", WELCOME_MSG.replace("xx.xx.xx", &padded_version));

    let args = Args::parse();

    if args.self_update {
        return handle_self_update(args.prerelease).await;
    }

    if args.check_updates {
        return handle_check_updates(args.prerelease).await;
    }

    // Handle template-specific commands
    if args.list_templates {
        return handle_list_templates(&args).await;
    }

    if args.validate_templates {
        return handle_validate_templates(&args).await;
    }

    if args.render {
        return handle_render(&args).await;
    }

    // Main baking logic
    run_bake(args).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_parse_key_val_valid() {
        let result = parse_key_val("key=value").unwrap();
        assert_eq!(result, ("key".to_string(), "value".to_string()));
    }

    #[test]
    fn test_parse_key_val_with_equals_in_value() {
        let result = parse_key_val("key=value=with=equals").unwrap();
        assert_eq!(result, ("key".to_string(), "value=with=equals".to_string()));
    }

    #[test]
    fn test_parse_key_val_empty_value() {
        let result = parse_key_val("key=").unwrap();
        assert_eq!(result, ("key".to_string(), "".to_string()));
    }

    #[test]
    fn test_parse_key_val_invalid_no_equals() {
        let result = parse_key_val("keyvalue");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Invalid key=value pair"));
    }

    #[test]
    fn test_parse_key_val_invalid_only_key() {
        let result = parse_key_val("key");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_variables_empty() {
        let vars = vec![];
        let result = parse_variables(&vars);
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_variables_single() {
        let vars = vec![("key1".to_string(), "value1".to_string())];
        let result = parse_variables(&vars);
        assert_eq!(result.len(), 1);
        assert_eq!(result.get("key1"), Some(&"value1".to_string()));
    }

    #[test]
    fn test_parse_variables_multiple() {
        let vars = vec![
            ("key1".to_string(), "value1".to_string()),
            ("key2".to_string(), "value2".to_string()),
        ];
        let result = parse_variables(&vars);
        assert_eq!(result.len(), 2);
        assert_eq!(result.get("key1"), Some(&"value1".to_string()));
        assert_eq!(result.get("key2"), Some(&"value2".to_string()));
    }

    #[test]
    fn test_resolve_bake_path_with_path() {
        let path_arg = Some("/custom/path".to_string());
        let result = resolve_bake_path(&path_arg).unwrap();
        assert_eq!(result, std::path::PathBuf::from("/custom/path"));
    }

    #[test]
    fn test_resolve_bake_path_none() {
        let path_arg = None;
        let result = resolve_bake_path(&path_arg).unwrap();
        // Should return current directory
        assert_eq!(result, std::env::current_dir().unwrap());
    }

    #[tokio::test]
    async fn test_handle_list_templates_no_templates() {
        let temp_dir = tempdir().unwrap();

        // Create a minimal bake.yml
        let bake_config = r#"
name: test_project
"#;
        fs::write(temp_dir.path().join("bake.yml"), bake_config).unwrap();

        let args = Args {
            recipe: None,
            path: Some(temp_dir.path().to_string_lossy().to_string()),
            show_plan: false,
            clean: false,
            verbose: 0,
            vars: vec![],
            regex: false,
            dry_run: false,
            fail_fast: false,
            jobs: None,
            check_updates: false,
            self_update: false,
            prerelease: false,
            list_templates: true,
            validate_templates: false,
            render: false,
            skip_cache: false,
        };

        // This should succeed and print "No templates found"
        let result = handle_list_templates(&args).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_handle_validate_templates_no_templates() {
        let temp_dir = tempdir().unwrap();

        // Create a minimal bake.yml
        let bake_config = r#"
name: test_project
"#;
        fs::write(temp_dir.path().join("bake.yml"), bake_config).unwrap();

        let args = Args {
            recipe: None,
            path: Some(temp_dir.path().to_string_lossy().to_string()),
            show_plan: false,
            clean: false,
            verbose: 0,
            vars: vec![],
            regex: false,
            dry_run: false,
            fail_fast: false,
            jobs: None,
            check_updates: false,
            self_update: false,
            prerelease: false,
            list_templates: false,
            validate_templates: true,
            render: false,
            skip_cache: false,
        };

        // This should succeed and print "No templates found"
        let result = handle_validate_templates(&args).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_run_bake_no_recipes() {
        let temp_dir = tempdir().unwrap();

        // Create a minimal bake.yml with no recipes
        let bake_config = r#"
name: test_project
"#;
        fs::write(temp_dir.path().join("bake.yml"), bake_config).unwrap();

        let args = Args {
            recipe: None,
            path: Some(temp_dir.path().to_string_lossy().to_string()),
            show_plan: false,
            clean: false,
            verbose: 0,
            vars: vec![],
            regex: false,
            dry_run: false,
            fail_fast: false,
            jobs: None,
            check_updates: false,
            self_update: false,
            prerelease: false,
            list_templates: false,
            validate_templates: false,
            render: false,
            skip_cache: false,
        };

        // This should succeed but print "No recipes to bake"
        let result = run_bake(args).await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_args_struct_debug() {
        let args = Args {
            recipe: Some("test:recipe".to_string()),
            path: Some("/test/path".to_string()),
            show_plan: false,
            clean: false,
            verbose: 1,
            vars: vec![("key".to_string(), "value".to_string())],
            regex: false,
            dry_run: false,
            fail_fast: false,
            jobs: Some(4),
            check_updates: false,
            self_update: false,
            prerelease: false,
            list_templates: false,
            validate_templates: false,
            render: false,
            skip_cache: false,
        };

        // Test that Args implements Debug (this will compile if it does)
        let _debug_str = format!("{args:?}");
    }

    #[test]
    fn test_constants() {
        // Test that constants are accessible and have expected values
        // VERSION is populated from CARGO_PKG_VERSION at compile time
        assert_eq!(DEFAULT_LOG_LEVEL, "warn");
        assert!(WELCOME_MSG.contains("Let's Bake!"));
        // Verify VERSION is accessible (it's guaranteed to be non-empty by Cargo)
        let _ = VERSION;
    }
}
