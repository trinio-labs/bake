#![feature(coverage_attribute)]
mod baker;
mod cache;
mod project;
mod template;
mod update;

#[cfg(test)]
mod test_utils;

use anyhow::bail;
use indexmap::IndexMap;
use project::BakeProject;
use std::sync::Arc;

use clap::Parser;
use console::Term;
use env_logger::Env;

use crate::cache::CacheBuilder;
use crate::update::{check_for_updates, get_update_info, perform_self_update};

const VERSION: &str = env!("CARGO_PKG_VERSION");
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
    /// :<recipe>            - for all recipes in all cookbooks{n}
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
}

fn parse_key_val(s: &str) -> anyhow::Result<(String, String)> {
    match s.split_once('=') {
        Some((key, value)) => Ok((key.trim().to_owned(), value.trim().to_owned())),
        None => bail!("Variable Parse: Invalid variable format. Expected 'KEY=VALUE', but got '{}'. Ensure variables are passed using the --var NAME=VALUE syntax.", s),
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(Env::default().default_filter_or("warn")).init();

    let term = Term::stdout();
    let padded_version = format!("{VERSION:<8}");
    term.set_title("Bake");
    println!("{}", WELCOME_MSG.replace("xx.xx.xx", &padded_version));

    let args = Args::parse();

    // Handle update-specific commands first
    if args.update_info {
        match get_update_info() {
            Ok(info) => {
                println!("{}", info);
                return Ok(());
            }
            Err(e) => {
                eprintln!("Failed to get update info: {}", e);
                return Err(e);
            }
        }
    }

    if args.self_update {
        let prerelease = args.prerelease;
        let result = tokio::task::spawn_blocking(move || perform_self_update(prerelease))
            .await
            .unwrap();
        match result {
            Ok(_) => return Ok(()),
            Err(e) => {
                eprintln!("Self-update failed: {}", e);
                return Err(e);
            }
        }
    }

    if args.check_updates {
        let update_config = update::UpdateConfig {
            enabled: true,
            check_interval_days: 7,
            auto_update: false,
            prerelease: args.prerelease,
        };

        match check_for_updates(&update_config).await {
            Ok(Some(version)) => {
                println!("New version available: {}", version);
                return Ok(());
            }
            Ok(None) => {
                println!("No updates available");
                return Ok(());
            }
            Err(e) => {
                eprintln!("Failed to check for updates: {}", e);
                return Err(e);
            }
        }
    }

    let bake_path = if args.path.is_none() {
        std::env::current_dir().unwrap()
    } else {
        std::path::absolute(args.path.unwrap())?
    };

    println!("Loading project...");
    term.move_cursor_up(1)?;

    let override_variables =
        args.var
            .iter()
            .try_fold(IndexMap::new(), |mut acc, s| -> anyhow::Result<_> {
                let (k, v) = parse_key_val(s)?;
                acc.insert(k, v);
                Ok(acc)
            })?;

    match BakeProject::from(&bake_path, override_variables) {
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
                let _ = check_for_updates(&update_config).await;
            }

            // Build cache using project and Local, S3 and GCS strategies
            if args.skip_cache {
                println!("Skipping cache...");
                project.config.cache.local.enabled = false;
                project.config.cache.remotes = None;
            }
            let arc_project = Arc::new(project);

            // Get execution plan to determine which recipes need cache hashes
            let execution_plan = arc_project.get_recipes_for_execution(recipe_filter)?;
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
                Ok(_) => {}
                Err(err) => {
                    return Err(err);
                }
            }
        }
        Err(err) => {
            println!("Loading project... {}", console::style("✗").red());
            return Err(err);
        }
    }

    Ok(())
}
