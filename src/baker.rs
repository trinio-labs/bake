use std::{
    collections::BTreeMap,
    fs::File,
    io::Write,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use console::{style, Color};
use indicatif::{MultiProgress, ProgressBar};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, BufReader},
    process::{ChildStderr, ChildStdout},
    sync::mpsc,
    task::JoinSet,
    time,
};

use crate::{
    cache::{Cache, CacheResult},
    project::{BakeProject, Recipe, Status},
};

type RecipeQueue = Arc<Mutex<BTreeMap<String, Recipe>>>;

/// Bakes a project by running all recipes and their dependencies
///
/// # Arguments
/// * `project` - The project to bake
/// * `filter` - Optional recipe pattern to filter such as `foo:`
///
pub async fn bake(
    project: Arc<BakeProject>,
    cache: Cache,
    filter: Option<&str>,
) -> Result<(), String> {
    // Create .bake directories
    project.create_project_bake_dirs()?;

    let recipes = project.get_recipes(filter);
    let recipe_queue = RecipeQueue::new(Mutex::new(recipes));
    let (shutdown_tx, mut shutdown_rx) = mpsc::unbounded_channel();
    let mut join_set = JoinSet::new();
    let arc_cache = Arc::new(cache);

    let multi_progress = Arc::new(MultiProgress::new());

    (0..project.config.max_parallel).for_each(|_| {
        let shutdown_tx = shutdown_tx.clone();
        let arc_project = project.clone();
        let recipe_queue = recipe_queue.clone();
        let multi_progress = multi_progress.clone();
        let cache = arc_cache.clone();

        join_set.spawn(runner(
            arc_project,
            recipe_queue,
            cache,
            shutdown_tx,
            multi_progress,
        ));
    });

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            join_set.abort_all();
        },
        _ = shutdown_rx.recv() => {
            join_set.abort_all();
        },
        _ = async {
            // Wait for joinset to finish running
            while (join_set.join_next().await).is_some() {}
        } => {}
    }

    if recipe_queue
        .lock()
        .unwrap()
        .iter()
        .any(|(_, recipe)| matches!(recipe.run_status.status, Status::Error))
    {
        return Err("Some recipes failed to run".to_string());
    }

    Ok(())
}

/// Runners are spawned in parallel to run recipes that were added to the queue
///
/// runner also handles printing the progress bar to the console if needed
///
/// # Arguments
/// * `project` - The project to bake
/// * `recipe_queue` - The shared queue of recipes
/// * `status_map` - The shared status map
/// * `shutdown_tx` - The channel to send shutdown signals
/// * `multi_progress` - The multi progress bar
///
async fn runner(
    project: Arc<BakeProject>,
    recipe_queue: RecipeQueue,
    cache: Arc<Cache>,
    shutdown_tx: mpsc::UnboundedSender<()>,
    multi_progress: Arc<MultiProgress>,
) -> Result<(), String> {
    loop {
        let mut next_recipe_name: Option<String> = None;
        if let Ok(queue) = recipe_queue.lock() {
            // If there are no more recipes to process, quit runner loop
            if queue.is_empty() {
                break;
            }

            let result = queue.iter().find(|(_, recipe)| {
                if recipe.run_status.status == Status::Idle {
                    if let Some(dependencies) = recipe.dependencies.as_ref() {
                        let pending = dependencies.iter().any(|dep_name| {
                            // If the dependency isn't in the status map, allow it to "run" anyway as we will
                            // filter it later
                            if let Some(dep_rec) = queue.get(dep_name) {
                                matches!(dep_rec.run_status.status, Status::Running | Status::Idle)
                            } else {
                                false
                            }
                        });
                        !pending
                    } else {
                        true
                    }
                } else {
                    false
                }
            });

            if let Some((recipe_name, _)) = result {
                next_recipe_name = Some(recipe_name.clone());
            } else if queue
                .iter()
                .all(|(_, recipe)| matches!(recipe.run_status.status, Status::Done | Status::Error))
            {
                break;
            }
        }

        if let Some(next_recipe_name) = next_recipe_name {
            let mut progress_bar: Option<ProgressBar> = None;
            if !project.config.verbose {
                progress_bar = Some(
                    multi_progress.add(
                        ProgressBar::new_spinner()
                            .with_message(format!("Baking recipe {}...", next_recipe_name)),
                    ),
                );
            }
            // Run async tasks until one of them finishes
            tokio::select! {
                // Update progress bar if present
                _ = async {
                    loop {
                        if let Some(progress_bar) = progress_bar.as_ref() {
                            progress_bar.tick();
                        }
                        time::sleep(time::Duration::from_millis(100)).await;
                    }
                } => {},
                // Update status and run recipe asynchronously, awaiting for the result
                _ = async {
                    let next_recipe: Recipe;
                    {
                        let mut queue_mutex = recipe_queue.lock().unwrap();
                        let recipe = queue_mutex.get_mut(&next_recipe_name).unwrap();
                        if recipe.run_status.status == Status::Idle {
                            recipe.run_status.status = Status::Running;
                            next_recipe = recipe.clone();
                        } else {
                            return;
                        }
                    }

                    // let result = run_recipe(&next_recipe, project.get_recipe_log_path(&next_recipe.full_name()), project.config.verbose).await;
                    let result = match cache.get(&next_recipe.full_name()) {
                       CacheResult::Hit(_) => {
                            println!("{}: {} (cached)", next_recipe_name, console::style("✓").green());
                            Ok(())
                        },

                       CacheResult::Miss => {
                            run_recipe(&next_recipe, project.get_recipe_log_path(&next_recipe.full_name()), project.config.verbose).await
                        },
                    };

                    // let mut status_mutex = status_map.lock().unwrap();
                    // let status = status_mutex.get_mut(&next_recipe.full_name()).unwrap();
                    let mut queue_mutex = recipe_queue.lock().unwrap();
                    let recipe = queue_mutex.get_mut(&next_recipe_name).unwrap();

                    match result {
                        Ok(_) => {
                            recipe.run_status.status = Status::Done;
                            if let Some(progress_bar) = progress_bar.as_ref() {
                            progress_bar.finish_with_message(format!(
                                "Baking recipe {}... {}",
                                next_recipe_name,
                                console::style("✓").green()
                            ));
                            }
                        }
                        Err(err) => {
                            if let Some(progress_bar) = progress_bar.as_ref() {
                            progress_bar.finish_with_message(format!(
                                "Baking recipe {}... {}",
                                next_recipe_name,
                                console::style("✗").red()
                            ));
                            }
                            if project.config.fast_fail {
                                shutdown_tx.send(()).unwrap();
                            }
                            recipe.run_status.status = Status::Error;
                            recipe.run_status.output = err;
                        }
                    }
                } => {}
            }
        } else {
            time::sleep(time::Duration::from_millis(100)).await;
        }
    }

    Ok(())
}

/// Runs a single recipe as a system process and handles the output
///
/// # Arguments
/// * `recipe` - The recipe to run
/// * `project_root` - The root path of the project
/// * `verbose` - Whether to print verbose output
///
pub async fn run_recipe(
    recipe: &Recipe,
    log_file_path: PathBuf,
    verbose: bool,
) -> Result<(), String> {
    // TODO: Implement cache strategy
    let result = tokio::process::Command::new("sh")
        .current_dir(recipe.config_path.parent().unwrap())
        .arg("-c")
        .arg(recipe.run.clone())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn();

    match result {
        Ok(mut child) => {
            let stdout = child.stdout.take().unwrap();
            let stderr = child.stderr.take().unwrap();
            let process_handle = tokio::spawn(process_output(
                stdout,
                stderr,
                recipe.full_name(),
                log_file_path,
                verbose,
            ));
            if let Ok(exit_code) = child.wait().await {
                if !exit_code.success() {
                    return Err(format!(
                        "Recipe {} failed with exit code {}",
                        recipe.full_name(),
                        exit_code
                    ));
                }
            }
            if let Err(err) = process_handle.await {
                return Err(format!("Could wait for process output thread: {}", err));
            }
        }
        Err(err) => {
            return Err(format!("Could not spawn process: {}", err));
        }
    }

    Ok(())
}

fn name_to_term_color(string: &str) -> Color {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    string.hash(&mut hasher);
    let hash = hasher.finish();
    let mut color_num = hash % 229;

    // Remove dark and bright colors
    color_num = match color_num {
        0 => 1,
        15..=24 => color_num - 14,
        52..=56 => color_num - 51,
        _ => color_num,
    };

    Color::Color256(color_num as u8)
}

/// Processes the output of a process saving it to a file and printing to console if in verbose
/// mode
///
/// # Arguments
/// * `stdout` - The stdout of the process
/// * `stderr` - The stderr of the process
/// * `recipe_name` - The name of the recipe
/// * `project_root` - The root path of the project
/// * `verbose` - Whether to print verbose output
///
async fn process_output(
    stdout: ChildStdout,
    stderr: ChildStderr,
    recipe_name: String,
    log_file_path: PathBuf,
    verbose: bool,
) -> Result<(), String> {
    let mut join_set = JoinSet::new();
    let color = name_to_term_color(&recipe_name);
    let output_str = Arc::new(Mutex::new(String::new()));

    async fn collect_output<T: AsyncRead + Unpin>(
        output: T,
        recipe_name: String,
        color: Color,
        output_string: Arc<Mutex<String>>,
        verbose: bool,
    ) {
        let mut reader = BufReader::new(output).lines();
        while let Some(line) = reader.next_line().await.unwrap() {
            let formatted_line = format!("[{}]: {}", style(&recipe_name).fg(color), line);
            if verbose {
                println!("{formatted_line}");
            }
            output_string.lock().unwrap().push_str(&(line + "\n"));
        }
    }

    join_set.spawn(collect_output(
        stdout,
        recipe_name.clone(),
        color,
        output_str.clone(),
        verbose,
    ));

    join_set.spawn(collect_output(
        stderr,
        recipe_name.clone(),
        color,
        output_str.clone(),
        verbose,
    ));

    while (join_set.join_next().await).is_some() {}

    match File::create(log_file_path.clone()) {
        Ok(mut file) => {
            if let Err(err) = file.write_all(output_str.lock().unwrap().as_bytes()) {
                return Err(format!(
                    "could not write log file {}: {}",
                    log_file_path.display(),
                    err
                ));
            };
        }
        Err(err) => {
            return Err(format!(
                "could not create log file {}: {}",
                log_file_path.display(),
                err
            ));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, sync::Arc};

    use crate::{
        cache::{Cache, CacheResult, CacheResultData, CacheStrategy},
        project::BakeProject,
    };

    struct TestCacheStrategy {
        pub hit: bool,
    }
    impl CacheStrategy for TestCacheStrategy {
        fn get(&self, _: &str) -> CacheResult {
            if self.hit {
                CacheResult::Hit(CacheResultData {
                    stdout: "foo".to_string(),
                })
            } else {
                CacheResult::Miss
            }
        }
        fn put(&self, _: &str, _: PathBuf) -> Result<(), String> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn run_all_recipes() {
        let project = Arc::new(BakeProject::from(&PathBuf::from("resources/tests/valid")).unwrap());
        let mut cache = Cache::new(project.clone(), None);
        cache.strategies = vec![Box::new(TestCacheStrategy { hit: false })];
        let res = super::bake(project.clone(), cache, None).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn run_bar_recipes() {
        let mut project = BakeProject::from(&PathBuf::from("resources/tests/valid")).unwrap();
        project.config.verbose = false;
        let project = Arc::new(project);
        let mut cache = Cache::new(project.clone(), None);
        cache.strategies = vec![Box::new(TestCacheStrategy { hit: false })];
        let res = super::bake(project.clone(), cache, Some("bar:")).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn run_error_recipes() {
        let mut project = BakeProject::from(&PathBuf::from("resources/tests/valid")).unwrap();
        project.recipes.get_mut("bar:test").unwrap().run = String::from("ex12123123");
        let project = Arc::new(project);
        let mut cache = Cache::new(project.clone(), None);
        cache.strategies = vec![Box::new(TestCacheStrategy { hit: false })];
        let res = super::bake(project.clone(), cache, Some("bar:")).await;
        assert!(res.is_err());
    }
}
