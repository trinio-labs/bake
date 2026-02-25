// #![feature(coverage_attribute)]

// Re-export all modules for external use
pub mod baker;
pub mod cache;
pub mod execution_plan;
pub mod project;
pub mod template;
pub mod update;

// Test utilities module (available for both unit and integration tests)
#[cfg(test)]
pub mod test_utils;

// Re-export commonly used types for convenience
pub use project::BakeProject;
pub use update::check_for_updates;

use anyhow::{Context, bail};
use clap::{Parser, ValueEnum};
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

/// Cache strategy option
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum CacheStrategy {
    /// Use only local cache (disable remote)
    LocalOnly,
    /// Use only remote cache (disable local)
    RemoteOnly,
    /// Check local cache first, then remote (typical default)
    LocalFirst,
    /// Check remote cache first, then local
    RemoteFirst,
    /// Disable all caching
    Disabled,
}

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

    /// Verbose mode - show detailed output
    #[arg(short, long)]
    pub verbose: bool,

    /// Quiet mode - suppress non-essential output
    #[arg(short, long, conflicts_with = "verbose")]
    pub quiet: bool,

    /// Variables to override (key=value).
    /// Variables specified here override those defined at recipe, cookbook, or project level.
    #[arg(short = 'D', long = "var", visible_alias = "define", value_parser = parse_key_val, action = clap::ArgAction::Append)]
    pub vars: Vec<(String, String)>,

    /// Enable regex pattern matching for recipe filters
    #[arg(long)]
    pub regex: bool,

    /// Filter recipes by tags (comma-separated). Multiple tags are OR-ed (matches ANY tag).
    /// Example: --tags frontend,backend or --tags frontend --tags backend
    #[arg(short, long, value_delimiter = ',')]
    pub tags: Vec<String>,

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

    /// Include prerelease versions when checking for updates
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

    /// Skip using and saving to cache (disables both local and remote caches)
    #[arg(long)]
    pub skip_cache: bool,

    /// Override cache strategy (local-only, remote-only, local-first, remote-first, disabled)
    #[arg(long, value_enum)]
    pub cache: Option<CacheStrategy>,

    /// Environment for variable overrides (e.g., dev, prod, test)
    #[arg(long)]
    pub env: Option<String>,

    /// Force override version check - run even if project requires newer bake version
    #[arg(long)]
    pub force_version_override: bool,

    /// Generate shell completion script and print to stdout
    #[arg(long, value_name = "SHELL")]
    pub completions: Option<clap_complete::Shell>,

    /// List all recipe names (cookbook:recipe) for shell completion
    #[arg(long, hide = true)]
    pub list_recipe_names: bool,
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

/// Configuration for loading a bake project
pub struct ProjectLoadConfig {
    pub variables: IndexMap<String, String>,
    pub environment: Option<String>,
    pub verbose: bool,
    pub jobs: Option<usize>,
    pub fail_fast: bool,
    pub quiet: bool,
    pub force_version_override: bool,
}

impl ProjectLoadConfig {
    /// Create a ProjectLoadConfig from Args
    pub fn from_args(args: &Args, variables: IndexMap<String, String>) -> Self {
        Self {
            variables,
            environment: args.env.clone(),
            verbose: args.verbose,
            jobs: args.jobs,
            fail_fast: args.fail_fast,
            quiet: args.quiet,
            force_version_override: args.force_version_override,
        }
    }
}

pub async fn load_project_with_feedback(
    bake_path: &std::path::Path,
    config: ProjectLoadConfig,
) -> anyhow::Result<Arc<BakeProject>> {
    let term = Term::stderr();
    let loading_message = format!("Loading project from {}...", bake_path.display());

    // For initial loading UI, use the verbose parameter
    // (will be reconciled with config after project loads)
    if !config.verbose {
        term.write_line(&loading_message)?;
    }

    let mut project = match BakeProject::load(
        bake_path,
        config.environment.as_deref(),
        config.variables,
        config.force_version_override,
    ) {
        Ok(p) => p,
        Err(e) => {
            if !config.verbose {
                term.clear_line()?;
                term.move_cursor_up(1)?;
                term.clear_line()?;
            }
            bail!("Failed to load project: {}", e);
        }
    };

    // Apply configuration overrides only when flags are explicitly set
    if let Some(jobs) = config.jobs {
        project.config.max_parallel = jobs;
    }
    if config.fail_fast {
        project.config.fast_fail = true;
    }
    // Only override verbose config if CLI flags were provided
    if config.verbose {
        project.config.verbose = true;
    } else if config.quiet {
        project.config.verbose = false;
    }
    // Otherwise, keep the value from bake.yml config

    // Use the effective verbose setting (after applying overrides) for UI
    if !project.config.verbose {
        term.clear_line()?;
        term.move_cursor_up(1)?;
        term.clear_line()?;
    }

    // Check for updates if configured
    if project.config.update.enabled {
        let update_config = crate::update::UpdateConfig {
            enabled: project.config.update.enabled,
            check_interval_days: project.config.update.check_interval_days,
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

pub async fn handle_check_updates(prerelease: bool) -> anyhow::Result<()> {
    let config = update::UpdateConfig {
        enabled: true,
        check_interval_days: 0, // Always check
        prerelease,
    };

    match check_for_updates(&config, true).await? {
        Some(version) => {
            println!("New version available: {version}");
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
    let config = ProjectLoadConfig::from_args(args, variables);
    let project = load_project_with_feedback(&bake_path, config).await?;

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
    let config = ProjectLoadConfig::from_args(args, variables);
    let project = load_project_with_feedback(&bake_path, config).await?;

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
    let config = ProjectLoadConfig::from_args(args, variables.clone());
    let project = load_project_with_feedback(&bake_path, config).await?;

    // Unwrap Arc to get mutable access to project
    let mut project = Arc::try_unwrap(project).unwrap_or_else(|arc| (*arc).clone());

    // Build context for full loading
    let context = project.build_variable_context(&variables);

    // Get the execution plan to determine which recipes to show (triggers full loading)
    let recipe_filter = args.recipe.as_deref();
    let execution_plan = project.get_recipes_for_execution(
        recipe_filter,
        args.regex,
        &args.tags,
        args.env.as_deref(),
        &context,
    )?;

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
                        variables: cookbook.processed_variables.clone(),
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
        variables: project.processed_variables.clone(),
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
    let config = ProjectLoadConfig::from_args(&args, variables.clone());
    let project = load_project_with_feedback(&bake_path, config).await?;

    // Unwrap Arc to get mutable access to project
    let mut project = Arc::try_unwrap(project).unwrap_or_else(|arc| (*arc).clone());

    // Build context for full loading
    let context = project.build_variable_context(&variables);

    // Handle clean command
    if args.clean {
        let execution_plan = project.get_recipes_for_execution(
            args.recipe.as_deref(),
            args.regex,
            &args.tags,
            args.env.as_deref(),
            &context,
        )?;

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
        let _all_recipes: Vec<String> = execution_plan
            .iter()
            .flatten()
            .map(|recipe| format!("{}:{}", recipe.cookbook, recipe.name))
            .collect();

        // Clean outputs and cache directory
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

        // Clean CAS cache directory
        let cache_dir = project.get_project_bake_path().join("cache");
        if cache_dir.exists() {
            std::fs::remove_dir_all(&cache_dir).with_context(|| {
                format!("Failed to remove cache directory {}", cache_dir.display())
            })?;
            println!("Cleaned CAS cache directory: {}", cache_dir.display());
        }

        return Ok(());
    }

    // Get execution plan (triggers full loading)
    let execution_plan = project.get_recipes_for_execution(
        args.recipe.as_deref(),
        args.regex,
        &args.tags,
        args.env.as_deref(),
        &context,
    )?;

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

    // Handle cache override options
    if args.skip_cache {
        println!("Skipping cache...");
        project.config.cache.local.enabled = false;
        if let Some(ref mut remotes) = project.config.cache.remotes {
            remotes.enabled = false;
        }
    } else if let Some(cache_strategy) = args.cache {
        match cache_strategy {
            CacheStrategy::LocalOnly => {
                println!("Cache strategy: local-only");
                project.config.cache.local.enabled = true;
                // Disable remote caches but keep configuration
                if let Some(ref mut remotes) = project.config.cache.remotes {
                    remotes.enabled = false;
                }
                project.config.cache.order = vec!["local".to_string()];
            }
            CacheStrategy::RemoteOnly => {
                println!("Cache strategy: remote-only");
                project.config.cache.local.enabled = false;
                if let Some(ref mut remotes) = project.config.cache.remotes {
                    remotes.enabled = true;
                    // Set order to remote strategies only
                    let mut remote_order = Vec::new();
                    if remotes.s3.is_some() {
                        remote_order.push("s3".to_string());
                    }
                    if remotes.gcs.is_some() {
                        remote_order.push("gcs".to_string());
                    }
                    project.config.cache.order = remote_order;
                } else {
                    println!("Warning: Remote caches are not configured in bake.yml");
                }
            }
            CacheStrategy::LocalFirst => {
                println!("Cache strategy: local-first");
                project.config.cache.local.enabled = true;
                // Enable remote caches if configured
                if let Some(ref mut remotes) = project.config.cache.remotes {
                    remotes.enabled = true;
                }
                // Build order: local, then remotes
                let mut order = vec!["local".to_string()];
                if let Some(ref remotes) = project.config.cache.remotes {
                    if remotes.s3.is_some() {
                        order.push("s3".to_string());
                    }
                    if remotes.gcs.is_some() {
                        order.push("gcs".to_string());
                    }
                }
                project.config.cache.order = order;
            }
            CacheStrategy::RemoteFirst => {
                println!("Cache strategy: remote-first");
                project.config.cache.local.enabled = true;
                // Enable remote caches if configured
                if let Some(ref mut remotes) = project.config.cache.remotes {
                    remotes.enabled = true;
                }
                // Build order: remotes first, then local
                let mut order = Vec::new();
                if let Some(ref remotes) = project.config.cache.remotes {
                    if remotes.s3.is_some() {
                        order.push("s3".to_string());
                    }
                    if remotes.gcs.is_some() {
                        order.push("gcs".to_string());
                    }
                }
                order.push("local".to_string());
                project.config.cache.order = order;
            }
            CacheStrategy::Disabled => {
                println!("Cache strategy: disabled");
                project.config.cache.local.enabled = false;
                if let Some(ref mut remotes) = project.config.cache.remotes {
                    remotes.enabled = false;
                }
            }
        }
    }

    // Wrap project back in Arc for execution
    let project = Arc::new(project);

    // Build cache for recipes
    let _all_recipes: Vec<String> = execution_plan
        .iter()
        .flatten()
        .map(|recipe| format!("{}:{}", recipe.cookbook, recipe.name))
        .collect();

    // Create CAS cache based on configuration
    let cache = if args.skip_cache || matches!(args.cache, Some(CacheStrategy::Disabled)) {
        // Cache is explicitly disabled
        cache::Cache::disabled()
    } else {
        // Determine cache strategy
        let strategy = args.cache.unwrap_or_else(|| {
            // Default strategy based on configuration
            if project.config.cache.local.enabled {
                if project.config.cache.remotes.is_some() {
                    // Both local and remote - use local-first
                    CacheStrategy::LocalFirst
                } else {
                    // Only local
                    CacheStrategy::LocalOnly
                }
            } else {
                CacheStrategy::Disabled
            }
        });

        // Check for disabled again (from default strategy)
        if matches!(strategy, CacheStrategy::Disabled) {
            cache::Cache::disabled()
        } else {
            // Convert CLI CacheStrategy to cache::CacheStrategy
            let cache_strategy = match strategy {
                CacheStrategy::LocalOnly => cache::CacheStrategy::LocalOnly,
                CacheStrategy::RemoteOnly => cache::CacheStrategy::RemoteOnly,
                CacheStrategy::LocalFirst => cache::CacheStrategy::LocalFirst,
                CacheStrategy::RemoteFirst => cache::CacheStrategy::RemoteFirst,
                CacheStrategy::Disabled => unreachable!("Already handled above"),
            };

            // Create multi-tier cache
            let cache_root = project.get_project_bake_path().join("cache");
            cache::Cache::with_strategy(
                cache_root,
                project.root_path.clone(),
                cache::CacheConfig::default(),
                cache_strategy,
                &project.config.cache,
            )
            .await?
        }
    };

    // Execute recipes
    baker::bake(project, cache, execution_plan, false).await
}

/// Write recipe names to an arbitrary writer (for testability)
pub async fn handle_list_recipe_names_to_writer(
    bake_path: &std::path::Path,
    writer: &mut dyn std::io::Write,
) -> anyhow::Result<()> {
    let project = BakeProject::load(bake_path, None, IndexMap::new(), false)?;
    let mut unique_recipes = std::collections::BTreeSet::new();
    for (cookbook_name, cookbook) in &project.cookbooks {
        for (recipe_name, recipe) in &cookbook.recipes {
            unique_recipes.insert(recipe_name.clone());
            if let Some(desc) = &recipe.description {
                writeln!(writer, "{cookbook_name}:{recipe_name}\t{desc}")?;
            } else {
                writeln!(writer, "{cookbook_name}:{recipe_name}")?;
            }
        }
    }
    // Output :recipe entries for the cross-cookbook shorthand syntax
    for recipe_name in &unique_recipes {
        writeln!(writer, ":{recipe_name}")?;
    }
    Ok(())
}

async fn handle_list_recipe_names(bake_path: &std::path::Path) -> anyhow::Result<()> {
    handle_list_recipe_names_to_writer(bake_path, &mut std::io::stdout()).await
}

fn generate_bash_completion() -> String {
    r#"_bake_completions() {
    local cur prev
    cur="${COMP_WORDS[COMP_CWORD]}"
    prev="${COMP_WORDS[COMP_CWORD-1]}"

    # Options that take a value — complete the value, not a new flag
    case "$prev" in
        -p|--path)
            COMPREPLY=($(compgen -d -- "$cur"))
            return
            ;;
        -D|--var|--define)
            return
            ;;
        --cache)
            COMPREPLY=($(compgen -W "local-only remote-only local-first remote-first disabled" -- "$cur"))
            return
            ;;
        --env)
            return
            ;;
        --completions)
            COMPREPLY=($(compgen -W "bash zsh fish" -- "$cur"))
            return
            ;;
        --jobs|-j)
            return
            ;;
        --tags|-t)
            return
            ;;
    esac

    # Handle :recipe syntax — bash splits on colon due to COMP_WORDBREAKS
    if [[ "$prev" == ":" ]]; then
        local bake_path=""
        for ((i=1; i < COMP_CWORD; i++)); do
            if [[ "${COMP_WORDS[i]}" == "-p" || "${COMP_WORDS[i]}" == "--path" ]]; then
                bake_path="${COMP_WORDS[i+1]}"
                break
            fi
        done
        local recipes
        if [[ -n "$bake_path" ]]; then
            recipes=$(bake --list-recipe-names --path "$bake_path" 2>/dev/null | cut -f1)
        else
            recipes=$(bake --list-recipe-names 2>/dev/null | cut -f1)
        fi
        # Extract unique recipe names from :recipe entries
        local recipe_names
        recipe_names=$(echo "$recipes" | grep "^:" | sed "s/^://")
        COMPREPLY=($(compgen -W "$recipe_names" -- "$cur"))
        return
    fi

    # Flags
    if [[ "$cur" == -* ]]; then
        local flags="--path --show-plan --explain --clean --verbose --quiet --var --define --regex --tags --dry-run --fail-fast --jobs --check-updates --prerelease --list-templates --validate-templates --render --skip-cache --cache --env --force-version-override --completions --help --version"
        COMPREPLY=($(compgen -W "$flags" -- "$cur"))
        return
    fi

    # Recipe completions (cookbook:recipe)
    local bake_path=""
    for ((i=1; i < COMP_CWORD; i++)); do
        if [[ "${COMP_WORDS[i]}" == "-p" || "${COMP_WORDS[i]}" == "--path" ]]; then
            bake_path="${COMP_WORDS[i+1]}"
            break
        fi
    done

    local recipes
    if [[ -n "$bake_path" ]]; then
        recipes=$(bake --list-recipe-names --path "$bake_path" 2>/dev/null | cut -f1)
    else
        recipes=$(bake --list-recipe-names 2>/dev/null | cut -f1)
    fi

    if [[ -z "$recipes" ]]; then
        return
    fi

    # Colon-aware completion
    if [[ "$cur" == *:* ]]; then
        local cookbook="${cur%%:*}"
        local partial="${cur#*:}"
        local matching
        matching=$(echo "$recipes" | grep "^${cookbook}:" | sed "s/^${cookbook}://")
        COMPREPLY=($(compgen -W "$matching" -- "$partial"))
        # Prepend the cookbook: prefix back
        COMPREPLY=("${COMPREPLY[@]/#/${cookbook}:}")
    else
        COMPREPLY=($(compgen -W "$recipes" -- "$cur"))
    fi

    # Prevent bash from splitting on colons
    __ltrim_colon_completions "$cur" 2>/dev/null
}

complete -o nospace -F _bake_completions bake
"#
    .to_string()
}

fn generate_zsh_completion() -> String {
    r##"#compdef bake

_bake() {
    local -a recipes
    local bake_path=""

    # Extract --path / -p value from existing args
    for ((i=1; i < ${#words[@]}; i++)); do
        if [[ "${words[i]}" == "-p" || "${words[i]}" == "--path" ]]; then
            bake_path="${words[i+1]}"
            break
        fi
    done

    _bake_recipes() {
        local cmd="bake --list-recipe-names"
        if [[ -n "$bake_path" ]]; then
            cmd="$cmd --path $bake_path"
        fi
        local recipe_list
        recipe_list=$(eval "$cmd" 2>/dev/null)
        if [[ -n "$recipe_list" ]]; then
            local -a completions
            local line name desc escaped
            while IFS=$'\t' read -r name desc; do
                # Escape colons in the name so _describe doesn't split on them
                escaped="${name//:/\\:}"
                if [[ -n "$desc" ]]; then
                    completions+=("${escaped}:${desc}")
                else
                    completions+=("${escaped}")
                fi
            done <<< "$recipe_list"
            _describe 'recipe' completions
        fi
    }

    _arguments -s \
        '1:recipe:_bake_recipes' \
        '(-p --path)'{-p,--path}'[Path to config file or directory]:path:_directories' \
        '(-e --show-plan --explain)'{-e,--show-plan,--explain}'[Show execution plan only]' \
        '(-c --clean)'{-c,--clean}'[Clean outputs and caches]' \
        '(-v --verbose)'{-v,--verbose}'[Verbose mode]' \
        '(-q --quiet)'{-q,--quiet}'[Quiet mode]' \
        '*'{-D,--var,--define}'[Variable override (key=value)]:variable:' \
        '--regex[Enable regex pattern matching]' \
        '*'{-t,--tags}'[Filter by tags]:tags:' \
        '(-n --dry-run)'{-n,--dry-run}'[Dry run mode]' \
        '(-f --fail-fast)'{-f,--fail-fast}'[Fail fast on first error]' \
        '(-j --jobs)'{-j,--jobs}'[Max concurrent recipes]:jobs:' \
        '--check-updates[Check for updates]' \
        '--prerelease[Include prerelease versions]' \
        '--list-templates[List available templates]' \
        '--validate-templates[Validate all templates]' \
        '--render[Print rendered cookbooks]' \
        '--skip-cache[Skip cache]' \
        '--cache[Cache strategy]:strategy:(local-only remote-only local-first remote-first disabled)' \
        '--env[Environment for variable overrides]:environment:' \
        '--force-version-override[Force version check override]' \
        '--completions[Generate shell completions]:shell:(bash zsh fish)' \
        '(- *)--help[Show help]' \
        '(- *)--version[Show version]'
}

_bake "$@"
"##
    .to_string()
}

fn generate_fish_completion() -> String {
    r#"# Disable file completions by default
complete -c bake -f

# Helper function for recipe completions
function __bake_recipes
    set -l bake_path ""
    set -l tokens (commandline -opc)
    for i in (seq 1 (count $tokens))
        if test "$tokens[$i]" = "-p" -o "$tokens[$i]" = "--path"
            set -l next (math $i + 1)
            if test $next -le (count $tokens)
                set bake_path $tokens[$next]
            end
        end
    end

    if test -n "$bake_path"
        bake --list-recipe-names --path "$bake_path" 2>/dev/null
    else
        bake --list-recipe-names 2>/dev/null
    end
end

# Recipe completions (only when not completing a flag)
complete -c bake -n 'not string match -q -- "-*" (commandline -ct)' -a '(__bake_recipes)'

# Flag completions
complete -c bake -s p -l path -d 'Path to config file or directory' -r -F
complete -c bake -s e -l show-plan -d 'Show execution plan only'
complete -c bake -s c -l clean -d 'Clean outputs and caches'
complete -c bake -s v -l verbose -d 'Verbose mode'
complete -c bake -s q -l quiet -d 'Quiet mode'
complete -c bake -s D -l var -d 'Variable override (key=value)' -r
complete -c bake -l regex -d 'Enable regex pattern matching'
complete -c bake -s t -l tags -d 'Filter by tags' -r
complete -c bake -s n -l dry-run -d 'Dry run mode'
complete -c bake -s f -l fail-fast -d 'Fail fast on first error'
complete -c bake -s j -l jobs -d 'Max concurrent recipes' -r
complete -c bake -l check-updates -d 'Check for updates'
complete -c bake -l prerelease -d 'Include prerelease versions'
complete -c bake -l list-templates -d 'List available templates'
complete -c bake -l validate-templates -d 'Validate all templates'
complete -c bake -l render -d 'Print rendered cookbooks'
complete -c bake -l skip-cache -d 'Skip cache'
complete -c bake -l cache -d 'Cache strategy' -r -a 'local-only remote-only local-first remote-first disabled'
complete -c bake -l env -d 'Environment for variable overrides' -r
complete -c bake -l force-version-override -d 'Force version check override'
complete -c bake -l completions -d 'Generate shell completions' -r -a 'bash zsh fish'
complete -c bake -l help -d 'Show help'
complete -c bake -l version -d 'Show version'
"#
    .to_string()
}

fn print_completions(shell: clap_complete::Shell) {
    let script = match shell {
        clap_complete::Shell::Bash => generate_bash_completion(),
        clap_complete::Shell::Zsh => generate_zsh_completion(),
        clap_complete::Shell::Fish => generate_fish_completion(),
        _ => {
            eprintln!("Unsupported shell: {shell}. Supported shells: bash, zsh, fish");
            std::process::exit(1);
        }
    };
    print!("{script}");
}

/// Main entry point for the library - initializes logging and runs the application
pub async fn run() -> anyhow::Result<()> {
    env_logger::Builder::from_env(Env::default().default_filter_or(DEFAULT_LOG_LEVEL)).init();

    let args = Args::parse();

    // Handle completion-related flags before printing any UI
    if let Some(shell) = args.completions {
        print_completions(shell);
        return Ok(());
    }

    if args.list_recipe_names {
        let bake_path = resolve_bake_path(&args.path)?;
        return handle_list_recipe_names(&bake_path).await;
    }

    // Print welcome banner
    let term = Term::stdout();
    let padded_version = format!("{VERSION:<8}");
    term.set_title("Bake");
    println!("{}", WELCOME_MSG.replace("xx.xx.xx", &padded_version));

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
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Invalid key=value pair")
        );
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
            verbose: false,
            quiet: false,
            vars: vec![],
            regex: false,
            tags: vec![],
            dry_run: false,
            fail_fast: false,
            jobs: None,
            check_updates: false,
            prerelease: false,
            list_templates: true,
            validate_templates: false,
            render: false,
            skip_cache: false,
            cache: None,
            env: None,
            force_version_override: false,
            completions: None,
            list_recipe_names: false,
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
            verbose: false,
            quiet: false,
            vars: vec![],
            regex: false,
            tags: vec![],
            dry_run: false,
            fail_fast: false,
            jobs: None,
            check_updates: false,
            prerelease: false,
            list_templates: false,
            validate_templates: true,
            render: false,
            skip_cache: false,
            cache: None,
            env: None,
            force_version_override: false,
            completions: None,
            list_recipe_names: false,
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
            verbose: false,
            quiet: false,
            vars: vec![],
            regex: false,
            tags: vec![],
            dry_run: false,
            fail_fast: false,
            jobs: None,
            check_updates: false,
            prerelease: false,
            list_templates: false,
            validate_templates: false,
            render: false,
            skip_cache: false,
            cache: None,
            env: None,
            force_version_override: false,
            completions: None,
            list_recipe_names: false,
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
            verbose: true,
            quiet: false,
            vars: vec![("key".to_string(), "value".to_string())],
            regex: false,
            tags: vec![],
            dry_run: false,
            fail_fast: false,
            jobs: Some(4),
            check_updates: false,
            prerelease: false,
            list_templates: false,
            validate_templates: false,
            render: false,
            skip_cache: false,
            cache: None,
            env: None,
            force_version_override: false,
            completions: None,
            list_recipe_names: false,
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

    #[tokio::test]
    async fn test_handle_list_recipe_names() {
        let temp_dir = tempdir().unwrap();

        let bake_config = r#"
name: test_project
cookbooks:
  - path: ./app
"#;
        fs::write(temp_dir.path().join("bake.yml"), bake_config).unwrap();

        fs::create_dir_all(temp_dir.path().join("app")).unwrap();
        let cookbook_config = r#"
name: app
recipes:
  build:
    description: Build the project
    run: echo build
  test:
    run: echo test
"#;
        fs::write(temp_dir.path().join("app/cookbook.yml"), cookbook_config).unwrap();

        let mut buf = Vec::new();
        handle_list_recipe_names_to_writer(temp_dir.path(), &mut buf)
            .await
            .unwrap();

        let output = String::from_utf8(buf).unwrap();
        let lines: Vec<&str> = output.lines().collect();
        assert!(lines.contains(&"app:build\tBuild the project"));
        assert!(lines.contains(&"app:test"));
        assert!(lines.contains(&":build"));
        assert!(lines.contains(&":test"));
        assert_eq!(lines.len(), 4);
    }

    #[test]
    fn test_generate_completions_produces_output() {
        use clap::CommandFactory;
        let mut buf = Vec::new();
        let mut cmd = Args::command();
        clap_complete::generate(clap_complete::Shell::Bash, &mut cmd, "bake", &mut buf);
        let output = String::from_utf8(buf).unwrap();
        assert!(!output.is_empty());
        assert!(output.contains("bake"));
    }

    #[test]
    fn test_bash_completion_script_contains_key_elements() {
        let script = generate_bash_completion();
        assert!(script.contains("_bake_completions"));
        assert!(script.contains("--list-recipe-names"));
        assert!(script.contains("complete -o nospace -F _bake_completions bake"));
    }

    #[test]
    fn test_zsh_completion_script_contains_key_elements() {
        let script = generate_zsh_completion();
        assert!(script.contains("#compdef bake"));
        assert!(script.contains("--list-recipe-names"));
    }

    #[test]
    fn test_fish_completion_script_contains_key_elements() {
        let script = generate_fish_completion();
        assert!(script.contains("complete -c bake"));
        assert!(script.contains("--list-recipe-names"));
    }
}
