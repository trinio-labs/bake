#![feature(coverage_attribute)]
mod baker;
mod cache;
mod project;
mod template;

#[cfg(test)]
mod test_utils;

use anyhow::bail;
use indexmap::IndexMap;
use project::BakeProject;
use std::{path::PathBuf, sync::Arc};

use clap::Parser;
use console::Term;
use env_logger::Env;

use crate::cache::CacheBuilder;

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
}

fn parse_key_val(s: &str) -> anyhow::Result<(String, String)> {
    match s.split_once('=') {
        Some((key, value)) => Ok((key.trim().to_owned(), value.trim().to_owned())),
        None => bail!("Expected key=value, got {}", s),
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(Env::default().default_filter_or("warn")).init();

    let term = Term::stdout();
    let padded_version = format!("{:<8}", VERSION);
    term.set_title("Bake");
    println!("{}", WELCOME_MSG.replace("xx.xx.xx", &padded_version));

    let args = Args::parse();
    let bake_path = if args.path.is_none() {
        std::env::current_dir().unwrap()
    } else {
        PathBuf::from(args.path.unwrap())
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
        Ok(project) => {
            println!("Loading project... {}", console::style("✓").green());
            let recipe_filter = args.recipe.as_deref();
            let arc_project = Arc::new(project);

            // Build cache using project and Local, S3 and GCS strategies
            let mut cache_builder = CacheBuilder::new(arc_project.clone());
            if let Some(recipe_filter) = recipe_filter {
                cache_builder.filter(recipe_filter);
            }

            let cache = match cache_builder.default_strategies().build().await {
                Ok(cache) => cache,
                Err(err) => {
                    println!("Error creating cache: {}", err);
                    return Err(err);
                }
            };

            match baker::bake(arc_project.clone(), cache, args.recipe.as_deref()).await {
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
