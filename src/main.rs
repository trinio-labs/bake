#![feature(coverage_attribute)]
mod baker;
mod cache;
mod project;
mod template;
mod update;

#[cfg(test)]
mod test_utils;

use anyhow::{bail, Context};
use indexmap::IndexMap;
use project::BakeProject;
use std::sync::Arc;

use clap::Parser;
use console::Term;
use env_logger::Env;

use crate::cache::CacheBuilder;
use crate::update::{check_for_updates, get_update_info, perform_self_update};

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
struct Args {
    /// Recipe to bake. Use:{n}{n}
    /// <cookbook>:<recipe>  - for a cookbook's recipe{n}
    /// <cookbook>:          - for all recipes in a cookbook{n}
    /// :<recipe>            - for all recipes with that name across all cookbooks{n}
    /// By default, cookbook and recipe names are matched exactly.{n}
    /// Use --regex flag to enable regex pattern matching.{n}
    recipe: Option<String>,

    /// Path fo config file or directory containing a bake.yml file
    #[arg(short, long)]
    path: Option<String>,

    /// Pass variable values
    #[arg(long, num_args = 1, value_name = "VAR>=<VALUE")]
    var: Vec<String>,

    /// Skip using and saving to cache
    #[arg(long)]
    skip_cache: bool,

    /// Check for updates
    #[arg(long)]
    check_updates: bool,

    /// Perform self-update
    #[arg(long)]
    self_update: bool,

    /// Include prereleases when checking for updates
    #[arg(long)]
    prerelease: bool,

    /// Show update information
    #[arg(long)]
    update_info: bool,

    /// Update the bake version in the project configuration to the current version
    #[arg(long)]
    update_version: bool,

    /// Force running even if the config version is newer than the current version
    #[arg(long)]
    force_version_override: bool,

    /// Use regex patterns for cookbook and recipe matching
    #[arg(long)]
    regex: bool,

    /// List all available recipe templates
    #[arg(long)]
    list_templates: bool,

    /// Validate all recipe templates
    #[arg(long)]
    validate_templates: bool,

    /// Print rendered cookbooks with all variables and templates resolved
    #[arg(long)]
    render: bool,
}

fn parse_key_val(s: &str) -> anyhow::Result<(String, String)> {
    match s.split_once('=') {
        Some((key, value)) => Ok((key.trim().to_owned(), value.trim().to_owned())),
        None => bail!("Variable Parse: Invalid variable format. Expected 'KEY=VALUE', but got '{}'. Ensure variables are passed using the --var NAME=VALUE syntax.", s),
    }
}

fn parse_variables(vars: &[String]) -> anyhow::Result<IndexMap<String, String>> {
    vars.iter()
        .try_fold(IndexMap::new(), |mut acc, s| -> anyhow::Result<_> {
            let (k, v) = parse_key_val(s)?;
            acc.insert(k, v);
            Ok(acc)
        })
}

async fn load_project_with_feedback(
    path: &Option<String>,
    variables: &[String],
    force_version_override: bool,
) -> anyhow::Result<BakeProject> {
    let bake_path = resolve_bake_path(path)?;
    
    println!("Loading project...");
    let term = Term::stdout();
    term.move_cursor_up(1)?;
    
    let override_variables = parse_variables(variables)?;
    
    match BakeProject::from(&bake_path, override_variables, force_version_override) {
        Ok(project) => {
            println!("Loading project... {}", console::style("✓").green());
            Ok(project)
        }
        Err(err) => {
            println!("Loading project... {}", console::style("✗").red());
            Err(err)
        }
    }
}

/// Resolves the bake project path from command line argument or current directory.
/// Returns an absolute path.
fn resolve_bake_path(path_arg: &Option<String>) -> anyhow::Result<std::path::PathBuf> {
    let path = match path_arg {
        Some(path) => std::path::absolute(path)?,
        None => std::env::current_dir()?,
    };

    Ok(path)
}

async fn handle_update_info() -> anyhow::Result<()> {
    get_update_info()
        .map(|info| println!("{info}"))
        .with_context(|| {
            "Failed to retrieve update information. Check your internet connection and try again."
        })
}

async fn handle_update_version(args: &Args) -> anyhow::Result<()> {
    let bake_path = resolve_bake_path(&args.path)?;

    let mut project = BakeProject::from(&bake_path, IndexMap::new(), args.force_version_override)
        .with_context(|| {
        format!("Failed to load project from path: {}", bake_path.display())
    })?;

    let old_version = project.config.min_version.clone();
    project.update_min_version();

    project.save_configuration().with_context(|| {
        "Failed to save updated configuration. Check file permissions and try again."
    })?;

    if let Some(old_ver) = old_version {
        println!(
            "✓ Updated bake version from v{} to v{}",
            old_ver,
            env!("CARGO_PKG_VERSION")
        );
    } else {
        println!("✓ Set bake version to v{}", env!("CARGO_PKG_VERSION"));
    }

    Ok(())
}

async fn handle_self_update(prerelease: bool) -> anyhow::Result<()> {
    tokio::task::spawn_blocking(move || perform_self_update(prerelease))
        .await
        .with_context(|| "Self-update task failed to complete")?
        .with_context(|| {
            "Self-update failed. Check your internet connection and permissions, then try again."
        })
}

async fn handle_check_updates(prerelease: bool) -> anyhow::Result<()> {
    let update_config = update::UpdateConfig {
        enabled: true,
        check_interval_days: 7,
        auto_update: false,
        prerelease,
    };

    match check_for_updates(&update_config, true)
        .await
        .with_context(|| {
            "Failed to check for updates. Check your internet connection and try again."
        })? {
        Some(version) => {
            println!("New version available: {version}");
            Ok(())
        }
        None => {
            println!("No updates available");
            Ok(())
        }
    }
}

async fn handle_list_templates(args: &Args) -> anyhow::Result<()> {
    let project = load_project_with_feedback(&args.path, &[], args.force_version_override).await?;
    
    if project.template_registry.is_empty() {
        println!("No templates found in {}", project.get_project_templates_path().display());
        println!("Create template files in .bake/templates/ directory to get started.");
        return Ok(());
    }

    println!("\nAvailable Templates:");
    println!("{}", "=".repeat(50));
    
    for (name, template) in &project.template_registry {
        println!("\n📋 Template: {}", console::style(name).bold().cyan());
        if let Some(description) = &template.description {
            println!("   Description: {description}");
        }
        
        if !template.parameters.is_empty() {
            println!("   Parameters:");
            for (param_name, param_def) in &template.parameters {
                let required = if param_def.required { " (required)" } else { "" };
                let default = if let Some(default) = &param_def.default {
                    format!(" [default: {}]", serde_yaml::to_string(default).unwrap_or_default().trim())
                } else {
                    String::new()
                };
                println!("     • {}: {:?}{}{}", param_name, param_def.parameter_type, required, default);
                if let Some(desc) = &param_def.description {
                    println!("       {desc}");
                }
            }
        }
        
        println!("   File: {}", template.template_path.display());
    }
    
    println!("\nTotal: {} template(s)", project.template_registry.len());
    Ok(())
}

async fn handle_validate_templates(args: &Args) -> anyhow::Result<()> {
    let project = load_project_with_feedback(&args.path, &[], args.force_version_override).await?;
    
    if project.template_registry.is_empty() {
        println!("No templates found in {}", project.get_project_templates_path().display());
        return Ok(());
    }

    println!("\nValidating Templates:");
    println!("{}", "=".repeat(50));
    
    let mut validation_errors = 0;
    
    for (name, template) in &project.template_registry {
        print!("📋 {name}: ");
        
        // Basic validation - template instantiation would catch most issues
        let mut errors = Vec::new();
        
        // Check if template has required run command
        if template.template.run.trim().is_empty() {
            errors.push("Template has no run command defined".to_string());
        }
        
        // Check parameter definitions
        for (param_name, param_def) in &template.parameters {
            if param_def.required && param_def.default.is_some() {
                errors.push(format!("Parameter '{param_name}' is marked as required but has a default value"));
            }
            
            // Validate regex patterns if present
            if let Some(pattern) = &param_def.pattern {
                if regex::Regex::new(pattern).is_err() {
                    errors.push(format!("Parameter '{param_name}' has invalid regex pattern: {pattern}"));
                }
            }
        }
        
        if errors.is_empty() {
            println!("{}", console::style("✓ Valid").green());
        } else {
            println!("{}", console::style("✗ Invalid").red());
            for error in errors {
                println!("   • {error}");
            }
            validation_errors += 1;
        }
    }
    
    println!("\nValidation Summary:");
    if validation_errors == 0 {
        println!("{} All {} template(s) are valid!", console::style("✓").green(), project.template_registry.len());
    } else {
        println!("{} {} template(s) have validation errors", console::style("✗").red(), validation_errors);
    }
    
    if validation_errors > 0 {
        std::process::exit(1);
    }
    
    Ok(())
}

async fn handle_render(args: &Args) -> anyhow::Result<()> {
    let project = load_project_with_feedback(&args.path, &args.var, args.force_version_override).await?;
    
    // Get the execution plan to determine which recipes to show
    let recipe_filter = args.recipe.as_deref();
    let execution_plan = project.get_recipes_for_execution(recipe_filter, args.regex)?;
    
    // Create a set of all recipe FQNs that should be included
    let included_recipes: std::collections::HashSet<String> = execution_plan
        .iter()
        .flatten()
        .map(|recipe| recipe.full_name())
        .collect();
    
    // Output header with pretty styling
    println!("\n{}", console::style("🍰 Rendered Bake Configuration").bold().cyan());
    println!("{}", console::style("✨ All variables and templates resolved").dim());
    if let Some(filter) = recipe_filter {
        println!("{} {}", console::style("🎯 Filter:").bold().yellow(), console::style(filter).bright().white());
    }
    println!("{}", "━".repeat(50));
    
    // Create serializable structures for cookbooks and recipes
    #[derive(serde::Serialize)]
    struct RenderedCookbook {
        name: String,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        environment: Vec<String>,
        #[serde(skip_serializing_if = "IndexMap::is_empty")]
        variables: IndexMap<String, String>,
        recipes: std::collections::BTreeMap<String, RenderedRecipe>,
    }
    
    #[derive(serde::Serialize)]
    struct RenderedRecipe {
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[serde(skip_serializing_if = "IndexMap::is_empty")]
        variables: IndexMap<String, String>,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        environment: Vec<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        dependencies: Option<Vec<String>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        cache: Option<crate::project::RecipeCacheConfig>,
        run: String,
    }
    
    // Convert project to renderable structure, filtering by included recipes
    let cookbooks: std::collections::BTreeMap<String, RenderedCookbook> = project.cookbooks
        .iter()
        .filter_map(|(name, cookbook)| {
            // Only include recipes that are in the execution plan
            let filtered_recipes: std::collections::BTreeMap<String, RenderedRecipe> = cookbook.recipes
                .iter()
                .filter(|(_recipe_name, recipe)| {
                    // If no filter specified, include all recipes
                    if included_recipes.is_empty() {
                        true
                    } else {
                        included_recipes.contains(&recipe.full_name())
                    }
                })
                .map(|(recipe_name, recipe)| {
                    (recipe_name.clone(), RenderedRecipe {
                        description: recipe.description.clone(),
                        variables: recipe.variables.clone(),
                        environment: recipe.environment.clone(),
                        dependencies: recipe.dependencies.clone(),
                        cache: recipe.cache.clone(),
                        run: recipe.run.clone(),
                    })
                })
                .collect();
            
            // Only include cookbooks that have at least one recipe to show
            if filtered_recipes.is_empty() {
                None
            } else {
                Some((name.clone(), RenderedCookbook {
                    name: cookbook.name.clone(),
                    environment: cookbook.environment.clone(),
                    variables: cookbook.variables.clone(),
                    recipes: filtered_recipes,
                }))
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
        variables: IndexMap<String, String>,
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
    println!("\n{}", console::style("📋 Project Information").bold().blue());
    println!("{}{}", 
        console::style("└─").blue(),
        console::style("─".repeat(25)).blue()
    );
    match serde_yaml::to_string(&project_info) {
        Ok(yaml) => {
            println!("{}", yaml.trim());
        }
        Err(err) => {
            eprintln!("Error serializing project info to YAML: {}", err);
            return Err(err.into());
        }
    }
    
    // Display each cookbook separately with pretty headers
    for (cookbook_name, cookbook) in &cookbooks {
        println!("\n\n{} {}", 
            console::style("📚").green(),
            console::style(&format!("Cookbook: {}", cookbook_name)).bold().green()
        );
        println!("{}{}", 
            console::style("└─").green(),
            console::style("─".repeat(12 + cookbook_name.len())).green()
        );
        
        match serde_yaml::to_string(cookbook) {
            Ok(yaml) => {
                println!("{}", yaml.trim());
            }
            Err(err) => {
                eprintln!("Error serializing cookbook '{}' to YAML: {}", cookbook_name, err);
                return Err(err.into());
            }
        }
    }
    
    Ok(())
}

async fn run_bake(args: Args) -> anyhow::Result<()> {
    let mut project = load_project_with_feedback(&args.path, &args.var, args.force_version_override).await?;
    let recipe_filter = args.recipe.as_deref();

    // Check for updates if enabled in config
    if project.config.update.enabled {
        let update_config = update::UpdateConfig {
            enabled: project.config.update.enabled,
            check_interval_days: project.config.update.check_interval_days,
            auto_update: project.config.update.auto_update,
            prerelease: project.config.update.prerelease,
        };

        // Run update check in background to not block the main workflow
        let _ = check_for_updates(&update_config, false).await;
    }

    // Build cache using project and Local, S3 and GCS strategies
    if args.skip_cache {
        println!("Skipping cache...");
        project.config.cache.local.enabled = false;
        project.config.cache.remotes = None;
    }
    let arc_project = Arc::new(project);

    // Get execution plan to determine which recipes need cache hashes
    let execution_plan =
        arc_project.get_recipes_for_execution(recipe_filter, args.regex)?;
    let recipes_to_execute: Vec<String> = execution_plan
        .iter()
        .flatten()
        .map(|recipe| recipe.full_name())
        .collect();

    let mut cache_builder = CacheBuilder::new(arc_project.clone());

    let cache = match cache_builder
        .default_strategies()
        .build_for_recipes(&recipes_to_execute)
        .await
    {
        Ok(cache) => cache,
        Err(err) => {
            eprintln!("Cache Initialization Error: Failed to create cache: {err}. Check cache configuration (local, S3, GCS) and connectivity.");
            return Err(err);
        }
    };

    match baker::bake(arc_project.clone(), cache, execution_plan).await {
        Ok(_) => Ok(()),
        Err(err) => Err(err),
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(Env::default().default_filter_or(DEFAULT_LOG_LEVEL)).init();

    let term = Term::stdout();
    let padded_version = format!("{VERSION:<8}");
    term.set_title("Bake");
    println!("{}", WELCOME_MSG.replace("xx.xx.xx", &padded_version));

    let args = Args::parse();

    // Handle update-specific commands first
    if args.update_info {
        return handle_update_info().await;
    }

    if args.update_version {
        return handle_update_version(&args).await;
    }

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
