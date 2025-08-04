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
}

fn parse_key_val(s: &str) -> anyhow::Result<(String, String)> {
    match s.split_once('=') {
        Some((key, value)) => Ok((key.trim().to_owned(), value.trim().to_owned())),
        None => bail!("Variable Parse: Invalid variable format. Expected 'KEY=VALUE', but got '{}'. Ensure variables are passed using the --var NAME=VALUE syntax.", s),
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

async fn run_bake(args: Args) -> anyhow::Result<()> {
    let bake_path = resolve_bake_path(&args.path)?;

    println!("Loading project...");
    let term = Term::stdout();
    term.move_cursor_up(1)?;

    let override_variables =
        args.var
            .iter()
            .try_fold(IndexMap::new(), |mut acc, s| -> anyhow::Result<_> {
                let (k, v) = parse_key_val(s)?;
                acc.insert(k, v);
                Ok(acc)
            })?;

    match BakeProject::from(&bake_path, override_variables, args.force_version_override) {
        Ok(mut project) => {
            println!("Loading project... {}", console::style("✓").green());
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
        Err(err) => {
            println!("Loading project... {}", console::style("✗").red());
            Err(err)
        }
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

    // Main baking logic
    run_bake(args).await
}
